use std::{cmp::min, str::FromStr};

use calc_rs::{
    math::checked_mul,
    msg::{
        ExchangeExecuteMsg, ExchangeQueryMsg, ManagerExecuteMsg, SchedulerExecuteMsg,
        SchedulerQueryMsg,
    },
    types::{
        Condition, ConditionFilter, Contract, ContractError, ContractResult, DcaSchedule,
        DcaStatistics, DcaStrategyConfig, Destination, DomainEvent, Executable, Status,
        StrategyConfig, StrategyStatistics, Trigger,
    },
};
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo,
    QuerierWrapper, Reply, Response, StdError, StdResult, SubMsg, SubMsgResult, Uint128, WasmMsg,
};
use prost::{DecodeError, EncodeError, Message};
use rujira_rs::{
    proto::types::{QueryNetworkRequest, QueryNetworkResponse},
    Asset, CallbackData, NativeAsset,
};
use thiserror::Error;

use crate::{state::CONFIG, types::Runnable};

pub const EXECUTE_REPLY_ID: u64 = 1;
pub const SCHEDULE_REPLY_ID: u64 = 2;

#[derive(Error, Debug)]
pub enum QueryError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Encode(#[from] EncodeError),

    #[error("{0}")]
    Decode(#[from] DecodeError),
}

fn query_chain<T: Message + Default, U: Message + Default>(
    querier: QuerierWrapper,
    path: String,
    req: U,
) -> Result<T, QueryError> {
    let mut buf = Vec::new();
    req.encode(&mut buf)?;
    let res = querier.query_grpc(path, Binary::from(buf))?.to_vec();
    Ok(T::decode(&*res)?)
}

fn get_swap_amount_after_execution_fee(
    deps: Deps,
    env: Env,
    strategy: &DcaStrategyConfig,
) -> StdResult<Coin> {
    let execution_fee = strategy.get_execution_fee(deps, env.clone())?;

    let balance = deps.querier.query_balance(
        env.contract.address.clone(),
        strategy.swap_amount.denom.clone(),
    )?;

    Ok(Coin {
        denom: strategy.swap_amount.denom.clone(),
        amount: min(
            balance.amount.checked_sub(execution_fee.amount)?,
            strategy.swap_amount.amount,
        ),
    })
}

fn get_gas_fee_amount(deps: Deps, env: Env, strategy: &DcaStrategyConfig) -> StdResult<Coin> {
    let network = query_chain::<QueryNetworkResponse, QueryNetworkRequest>(
        deps.querier,
        "/types.Query/Network".to_string(),
        QueryNetworkRequest {
            height: env.block.height.to_string(),
        },
    )
    .map_err(|e| StdError::generic_err(format!("Unable to fetch network params: {:?}", e)))?;

    let rune_price = Decimal::from_str(&network.rune_price_in_tor)?;
    let native_tx_fee_rune = Decimal::from_str(&network.native_tx_fee_rune)?;

    let gas_fee_in_usd = rune_price.checked_mul(native_tx_fee_rune)?;

    let asset_price = deps.querier.query_wasm_smart::<Decimal>(
        strategy.exchange_contract.clone(),
        &ExchangeQueryMsg::GetUsdPrice {
            asset: Asset::Native(NativeAsset::new(&strategy.swap_amount.denom)),
        },
    )?;

    let swap_amount = get_swap_amount_after_execution_fee(deps, env.clone(), strategy)?;

    let swap_amount_in_usd =
        asset_price.checked_mul(Decimal::from_ratio(swap_amount.amount, Uint128::one()))?;

    let gas_fee_amount = checked_mul(
        swap_amount.amount,
        gas_fee_in_usd
            .checked_div(swap_amount_in_usd)
            .map_err(|e| {
                StdError::generic_err(format!(
                    "Failed to calculate fee ratio from swap amount: {:?}",
                    e
                ))
            })?,
    )
    .map_err(|e| {
        StdError::generic_err(format!(
            "Failed to calculate fee amount from fee ratio: {:?}",
            e
        ))
    })?;

    Ok(Coin {
        denom: strategy.swap_amount.denom.clone(),
        amount: gas_fee_amount,
    })
}

fn get_schedule_msg(strategy: DcaStrategyConfig, deps: DepsMut, env: Env) -> StdResult<SubMsg> {
    let condition = match strategy.schedule {
        DcaSchedule::Blocks { interval, previous } => Condition::BlockHeight {
            height: previous.unwrap_or(env.block.height) + interval,
        },
        DcaSchedule::Time { duration, previous } => Condition::Timestamp {
            timestamp: previous
                .unwrap_or(env.block.time)
                .plus_seconds(duration.as_secs()),
        },
    };

    CONFIG.save(
        deps.storage,
        &StrategyConfig::Dca(DcaStrategyConfig {
            conditions: vec![condition.clone()],
            ..strategy.clone()
        }),
    )?;

    let create_trigger_msg = Contract(strategy.scheduler_contract.clone()).call(
        to_json_binary(&SchedulerExecuteMsg::CreateTrigger {
            condition: condition.clone(),
            to: strategy.manager_contract.clone(),
            callback: CallbackData(to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                contract_address: env.contract.address,
            })?),
        })?,
        vec![],
    )?;

    Ok(SubMsg::reply_always(create_trigger_msg, SCHEDULE_REPLY_ID))
}

impl Runnable for DcaStrategyConfig {
    fn initialize(&self, deps: DepsMut, env: Env, info: MessageInfo) -> ContractResult {
        deps.api.addr_validate(&self.owner.clone().into_string())?;

        if info.funds.len() > 1 {
            return Err(ContractError::Std(StdError::generic_err(
                "Cannot deposit multiple coins to a DCA strategy",
            )));
        }

        let destinations = self
            .mutable_destinations
            .iter()
            .chain(self.immutable_destinations.iter())
            .collect::<Vec<_>>();

        if destinations.is_empty() {
            return Err(ContractError::Std(StdError::generic_err(
                "Must provide at least one destination",
            )));
        }

        if destinations.len() > 20 {
            return Err(ContractError::Std(StdError::generic_err(
                "Cannot provide more than 20 total destinations",
            )));
        }

        for destination in destinations.clone() {
            deps.api.addr_validate(&destination.address.to_string())?;
        }

        let asset_usd_price = deps.querier.query_wasm_smart::<Decimal>(
            self.exchange_contract.to_string(),
            &ExchangeQueryMsg::GetUsdPrice {
                asset: Asset::Native(NativeAsset::new(&self.swap_amount.denom)),
            },
        )?;

        if asset_usd_price.is_zero() {
            return Err(ContractError::Std(StdError::generic_err(
                "Asset USD price cannot be zero",
            )));
        }

        let fee_destination = Destination {
            address: self.fee_collector.clone(),
            shares: checked_mul(
                destinations
                    .into_iter()
                    .fold(Uint128::zero(), |acc, d| acc + d.shares),
                Decimal::permille(25),
            )?,
            label: Some("DCA Fee Collection".to_string()),
        };

        let config = StrategyConfig::Dca(DcaStrategyConfig {
            immutable_destinations: [vec![fee_destination], self.immutable_destinations.clone()]
                .concat(),
            ..self.clone()
        });

        CONFIG.save(deps.storage, &config)?;

        Ok(Response::default().add_event(DomainEvent::StrategyCreated {
            contract_address: env.contract.address,
            config,
        }))
    }

    fn can_execute(&self, deps: Deps, env: Env) -> StdResult<()> {
        let swap_amount = get_swap_amount_after_execution_fee(deps, env.clone(), self)?;

        if swap_amount.amount.is_zero() {
            return Err(StdError::generic_err(format!(
                "Insufficient swap amount of {} ({}) to cover gas fees",
                self.swap_amount.denom, swap_amount.amount
            )));
        }

        let triggers = deps.querier.query_wasm_smart::<Vec<Trigger>>(
            self.scheduler_contract.clone(),
            &SchedulerQueryMsg::Get {
                filter: ConditionFilter::Owner {
                    address: env.contract.address.clone(),
                },
                limit: None,
            },
        )?;

        for trigger in triggers {
            if !trigger.can_execute(env.clone()) {
                return Err(StdError::generic_err(format!(
                    "Condition for execution not met: {:?}",
                    trigger.condition
                )));
            }
        }

        Ok(())
    }

    fn get_execution_fee(&self, deps: Deps, env: Env) -> StdResult<Coin> {
        Ok(get_gas_fee_amount(deps, env.clone(), self)?)
    }

    fn execute(&mut self, deps: DepsMut, env: Env) -> ContractResult {
        let mut sub_messages: Vec<SubMsg> = vec![];
        let mut events: Vec<DomainEvent> = vec![];

        match self.can_execute(deps.as_ref(), env.clone()) {
            Ok(_) => {
                let swap_amount =
                    get_swap_amount_after_execution_fee(deps.as_ref(), env.clone(), self)?;

                let swap_msg = Contract(self.exchange_contract.clone()).call(
                    to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: self.minimum_receive_amount.clone(),
                        route: None,
                        callback: None,
                    })?,
                    vec![swap_amount],
                )?;

                sub_messages.push(SubMsg::reply_always(swap_msg, EXECUTE_REPLY_ID));
            }
            Err(err) => {
                sub_messages.push(get_schedule_msg(self.clone(), deps, env.clone())?);

                events.push(DomainEvent::ExecutionSkipped {
                    contract_address: env.contract.address,
                    reason: err.to_string(),
                });
            }
        }

        Ok(Response::new()
            .add_submessages(sub_messages)
            .add_events(events))
    }

    fn handle_reply(&mut self, deps: DepsMut, env: Env, reply: Reply) -> ContractResult {
        let mut sub_messages: Vec<SubMsg> = vec![];
        let mut messages: Vec<CosmosMsg> = vec![];
        let mut events: Vec<DomainEvent> = vec![];

        match reply.id {
            EXECUTE_REPLY_ID => {
                match reply.result {
                    SubMsgResult::Ok(_) => {
                        let target_denom_balance = deps.querier.query_balance(
                            env.contract.address.clone(),
                            self.minimum_receive_amount.denom.clone(),
                        )?;

                        let destinations = self
                            .mutable_destinations
                            .iter()
                            .chain(self.immutable_destinations.iter());

                        let total_shares = destinations
                            .clone()
                            .fold(Uint128::zero(), |acc, d| acc + d.shares);

                        let send_messages = &mut destinations
                            .map(|d| {
                                BankMsg::Send {
                                    to_address: d.address.to_string(),
                                    amount: vec![Coin {
                                        denom: target_denom_balance.denom.clone(),
                                        amount: checked_mul(
                                            target_denom_balance.amount,
                                            Decimal::from_ratio(d.shares, total_shares),
                                        )
                                        .unwrap_or(Uint128::zero()),
                                    }],
                                }
                                .into()
                            })
                            .collect::<Vec<CosmosMsg>>();

                        messages.append(send_messages);

                        let swap_denom_balance = deps.querier.query_balance(
                            env.contract.address.clone(),
                            self.swap_amount.denom.clone(),
                        )?;

                        self.statistics = DcaStatistics {
                            amount_swapped: Coin {
                                denom: swap_denom_balance.denom,
                                amount: self.statistics.amount_swapped.amount.checked_add(
                                    self.statistics
                                        .amount_deposited
                                        .amount
                                        .checked_sub(self.statistics.amount_swapped.amount)?
                                        .checked_sub(swap_denom_balance.amount)?,
                                )?,
                            },
                            amount_received: Coin {
                                denom: target_denom_balance.denom,
                                amount: self
                                    .statistics
                                    .amount_received
                                    .amount
                                    .checked_add(target_denom_balance.amount)?,
                            },
                            ..self.statistics.clone()
                        };

                        CONFIG.save(deps.storage, &StrategyConfig::Dca(self.clone()))?;

                        events.push(DomainEvent::ExecutionSucceeded {
                            contract_address: env.contract.address.clone(),
                            statistics: StrategyStatistics::Dca(self.statistics.clone()),
                        });
                    }
                    SubMsgResult::Err(reason) => {
                        events.push(DomainEvent::ExecutionFailed {
                            contract_address: env.contract.address.clone(),
                            reason,
                        });
                    }
                }

                match self.can_execute(deps.as_ref(), env.clone()) {
                    Ok(_) => {
                        sub_messages.push(get_schedule_msg(self.clone(), deps, env.clone())?);
                    }
                    Err(_) => {
                        let pause_strategy_msg = Contract(self.manager_contract.clone()).call(
                            to_json_binary(&ManagerExecuteMsg::UpdateStatus {
                                status: Status::Paused,
                            })?,
                            vec![],
                        )?;

                        messages.push(pause_strategy_msg);

                        let strategy_paused_event = DomainEvent::StrategyPaused {
                            contract_address: env.contract.address.clone(),
                            reason: "Insufficient balance to reschedule".into(),
                        };

                        let scheduling_skipped_event = DomainEvent::SchedulingSkipped {
                            contract_address: env.contract.address.clone(),
                            reason: "Insufficient balance to reschedule".to_string(),
                        };

                        events.append(&mut vec![scheduling_skipped_event, strategy_paused_event])
                    }
                }

                Ok(Response::new()
                    .add_submessages(sub_messages)
                    .add_messages(messages)
                    .add_events(events))
            }
            SCHEDULE_REPLY_ID => {
                let mut events: Vec<DomainEvent> = vec![];

                match reply.result {
                    SubMsgResult::Ok(_) => {
                        events.push(DomainEvent::SchedulingSucceeded {
                            contract_address: env.contract.address.clone(),
                            conditions: self.conditions.clone(),
                        });
                    }
                    SubMsgResult::Err(reason) => {
                        CONFIG.save(
                            deps.storage,
                            &StrategyConfig::Dca(DcaStrategyConfig {
                                conditions: vec![],
                                ..self.clone()
                            }),
                        )?;

                        events.push(DomainEvent::SchedulingFailed {
                            contract_address: env.contract.address.clone(),
                            reason,
                        });
                    }
                }

                Ok(Response::new().add_events(events))
            }
            _ => Err(ContractError::Std(StdError::generic_err(
                "invalid reply id",
            ))),
        }
    }

    fn withdraw(&self, deps: Deps, env: Env, amounts: Vec<Coin>) -> ContractResult {
        let funds = amounts
            .iter()
            .map(|amount| {
                deps.querier
                    .query_balance(env.contract.address.clone(), amount.denom.clone())
            })
            .collect::<StdResult<Vec<Coin>>>()?;

        let send_assets_msg = BankMsg::Send {
            to_address: self.owner.to_string(),
            amount: funds
                .iter()
                .filter(|c| !c.amount.is_zero())
                .cloned()
                .collect(),
        };

        let funds_withdrawn_event = DomainEvent::FundsWithdrawn {
            contract_address: env.contract.address,
            to: self.owner.clone(),
            funds,
        };

        Ok(Response::default()
            .add_message(send_assets_msg)
            .add_event(funds_withdrawn_event))
    }

    fn pause(&self, _deps: Deps, env: Env) -> ContractResult {
        let delete_conditions_msg = WasmMsg::Execute {
            contract_addr: self.scheduler_contract.to_string(),
            msg: to_json_binary(&SchedulerExecuteMsg::DeleteTriggers {})?,
            funds: vec![],
        };

        let pause_strategy_msg = WasmMsg::Execute {
            contract_addr: self.manager_contract.clone().to_string(),
            msg: to_json_binary(&ManagerExecuteMsg::UpdateStatus {
                status: Status::Paused,
            })?,
            funds: vec![],
        };

        let strategy_paused_event = DomainEvent::StrategyPaused {
            contract_address: env.contract.address,
            reason: "User requested pause".into(),
        };

        Ok(Response::default()
            .add_messages(vec![delete_conditions_msg, pause_strategy_msg])
            .add_event(strategy_paused_event))
    }

    fn statistics(&self) -> StrategyStatistics {
        StrategyStatistics::Dca(self.statistics.clone())
    }
}
