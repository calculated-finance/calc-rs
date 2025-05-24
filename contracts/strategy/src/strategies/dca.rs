use calc_rs::{
    math::checked_mul,
    msg::{ExchangeExecuteMsg, FactoryExecuteMsg, SchedulerExecuteMsg, StrategyExecuteMsg},
    types::{
        Condition, Contract, ContractError, ContractResult, DcaStrategy, Destination, DomainEvent,
        Owned, Status, StrategyConfig, StrategyStatistics,
    },
};
use cosmwasm_std::{
    to_json_binary, BankMsg, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Reply,
    Response, StdError, StdResult, SubMsg, SubMsgResult, Uint128, WasmMsg,
};
use rujira_rs::CallbackData;

use crate::{
    state::{CONFIG, FACTORY},
    types::Runnable,
};

impl Runnable for DcaStrategy {
    fn initialize(&self, deps: DepsMut, env: Env, info: MessageInfo) -> ContractResult {
        deps.api.addr_validate(&self.owner.clone().into_string())?;

        if info.funds.len() > 1 {
            return ContractResult::Err(ContractError::Std(StdError::generic_err(
                "Cannot deposit multiple coins to a DCA strategy",
            )));
        }

        let fee_destinations = vec![Destination {
            address: self.fee_collector.clone(),
            shares: checked_mul(
                self.mutable_destinations
                    .iter()
                    .chain(self.immutable_destinations.iter())
                    .fold(Uint128::zero(), |acc, d| acc + d.shares),
                Decimal::permille(25),
            )?,
            label: Some("fee_collector".to_string()),
        }];

        let config = StrategyConfig::Dca(DcaStrategy {
            immutable_destinations: [fee_destinations, self.immutable_destinations.clone()]
                .concat(),
            ..self.clone()
        });

        CONFIG.save(deps.storage, &config)?;

        Ok(Response::default().add_event(DomainEvent::StrategyCreated {
            contract_address: env.contract.address,
            config,
        }))
    }

    fn can_execute(&self, deps: Deps, env: Env) -> Result<(), String> {
        match deps
            .querier
            .query_balance(env.contract.address.clone(), self.swap_amount.denom.clone())
        {
            Ok(balance) => {
                if balance.amount < self.swap_amount.amount {
                    return Err(format!(
                        "Insufficient balance of {}: {}",
                        balance.denom, balance.amount
                    ));
                }
                Ok(())
            }
            Err(e) => Err(format!(
                "Error querying balance for denom {}: {}",
                self.swap_amount.denom, e
            )),
        }
    }

    fn execute(&self, deps: Deps, env: Env) -> ContractResult {
        let mut sub_messages: Vec<SubMsg> = vec![];
        let mut messages: Vec<WasmMsg> = vec![];
        let mut events: Vec<DomainEvent> = vec![];

        match self.can_execute(deps, env.clone()) {
            Ok(_) => {
                let swap_msg = SubMsg::reply_always(
                    Contract(self.exchange_contract.clone()).call(
                        to_json_binary(&ExchangeExecuteMsg::Swap {
                            minimum_receive_amount: self.minimum_receive_amount.clone(),
                            route: None,
                            callback: None,
                        })?,
                        vec![self.swap_amount.clone()],
                    )?,
                    0,
                );

                sub_messages.push(swap_msg);

                let schedule_msg = WasmMsg::Execute {
                    contract_addr: env.contract.address.to_string(),
                    msg: to_json_binary(&StrategyExecuteMsg::Schedule {})?,
                    funds: vec![],
                };

                messages.push(schedule_msg);
            }
            Err(reason) => {
                events.push(DomainEvent::ExecutionSkipped {
                    contract_address: env.contract.address,
                    reason,
                });
            }
        }

        Ok(Response::new()
            .add_submessages(sub_messages)
            .add_messages(messages)
            .add_events(events))
    }

    fn handle_execute_reply(&self, deps: DepsMut, env: Env, reply: Reply) -> ContractResult {
        let mut messages: Vec<BankMsg> = vec![];
        let mut events: Vec<DomainEvent> = vec![];

        match reply.result {
            SubMsgResult::Ok(_) => {
                let balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), self.swap_amount.denom.clone())?;

                let destinations = self
                    .mutable_destinations
                    .iter()
                    .chain(self.immutable_destinations.iter());

                let total_shares = destinations
                    .clone()
                    .fold(Uint128::zero(), |acc, d| acc + d.shares);

                let send_messages = &mut destinations
                    .map(|d| BankMsg::Send {
                        to_address: d.address.to_string(),
                        amount: vec![Coin {
                            denom: balance.denom.clone(),
                            amount: checked_mul(
                                balance.amount,
                                Decimal::from_ratio(d.shares, total_shares),
                            )
                            .unwrap_or(Uint128::zero())
                            .into(),
                        }],
                    })
                    .collect::<Vec<BankMsg>>();

                messages.append(send_messages);

                events.push(DomainEvent::ExecutionSucceeded {
                    contract_address: env.contract.address.clone(),
                });
            }
            SubMsgResult::Err(reason) => {
                events.push(DomainEvent::ExecutionFailed {
                    contract_address: env.contract.address.clone(),
                    reason,
                });
            }
        }

        Ok(Response::new()
            .add_message(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_json_binary(&StrategyExecuteMsg::Schedule {})?,
                funds: vec![],
            })
            .add_events(events))
    }

    fn can_schedule(&self, deps: Deps, env: Env) -> Result<(), String> {
        match deps
            .querier
            .query_balance(env.contract.address.clone(), self.swap_amount.denom.clone())
        {
            Ok(balance) => {
                if balance.amount < self.swap_amount.amount {
                    return Err(format!(
                        "Insufficient balance of {}: {}",
                        balance.denom, balance.amount
                    ));
                }
                Ok(())
            }
            Err(e) => Err(format!(
                "Error querying balance for denom {}: {}",
                self.swap_amount.denom, e
            )),
        }
    }

    fn schedule(&self, deps: DepsMut, env: Env) -> ContractResult {
        let mut messages: Vec<WasmMsg> = vec![];

        match self.can_schedule(deps.as_ref(), env.clone()) {
            Ok(_) => {
                let create_trigger_msg = WasmMsg::Execute {
                    contract_addr: self.scheduler_contract.to_string(),
                    msg: to_json_binary(&SchedulerExecuteMsg::CreateTrigger {
                        condition: Condition::BlockHeight {
                            height: env.block.height + self.interval_blocks,
                        },
                        to: env.contract.address.clone(),
                        callback: CallbackData(to_json_binary(&StrategyExecuteMsg::Execute {})?),
                    })?,
                    funds: vec![],
                };

                messages.push(create_trigger_msg);
            }
            Err(reason) => {
                CONFIG.save(
                    deps.storage,
                    &StrategyConfig::Dca(DcaStrategy {
                        conditions: vec![],
                        ..self.clone()
                    }),
                )?;

                let pause_strategy_msg = WasmMsg::Execute {
                    contract_addr: FACTORY.load(deps.storage)?.into_string(),
                    msg: to_json_binary(&FactoryExecuteMsg::UpdateStatus {
                        status: Status::Paused,
                        reason: reason.clone(),
                    })?,
                    funds: vec![],
                };

                messages.push(pause_strategy_msg);
            }
        };

        Ok(Response::new().add_messages(messages))
    }

    fn withdraw(&self, deps: Deps, env: Env, denoms: Vec<String>) -> ContractResult {
        let send_assets_msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: CONFIG.load(deps.storage)?.owner().to_string(),
            amount: denoms
                .iter()
                .map(|denom| {
                    deps.querier
                        .query_balance(env.contract.address.clone(), denom.clone())
                })
                .collect::<StdResult<Vec<_>>>()?,
        });

        let pause_strategy_msg = WasmMsg::Execute {
            contract_addr: FACTORY.load(deps.storage)?.into_string(),
            msg: to_json_binary(&FactoryExecuteMsg::UpdateStatus {
                status: Status::Paused,
                reason: "User requested withdrawal".into(),
            })?,
            funds: vec![],
        };

        Ok(Response::default()
            .add_message(send_assets_msg)
            .add_message(pause_strategy_msg))
    }

    fn pause(&self, deps: Deps, env: Env) -> ContractResult {
        // delete triggers

        let pause_strategy_msg = WasmMsg::Execute {
            contract_addr: FACTORY.load(deps.storage)?.into_string(),
            msg: to_json_binary(&FactoryExecuteMsg::UpdateStatus {
                status: Status::Paused,
                reason: "User requested pause".into(),
            })?,
            funds: vec![],
        };

        Ok(Response::default().add_message(pause_strategy_msg))
    }

    fn statistics(&self) -> StrategyStatistics {
        StrategyStatistics::Dca(self.statistics.clone())
    }
}
