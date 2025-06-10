use std::cmp::min;

use calc_rs::{
    math::checked_mul,
    msg::{
        CreateTrigger, ExchangeExecuteMsg, ManagerExecuteMsg, ManagerQueryMsg, SchedulerExecuteMsg,
        SchedulerQueryMsg, StrategyExecuteMsg,
    },
    types::{
        Affiliate, Condition, ConditionFilter, Contract, ContractError, ContractResult,
        DcaSchedule, DcaStatistics, DcaStrategyConfig, Destination, DomainEvent, Executable,
        Status, Strategy, StrategyConfig, StrategyStatistics, Trigger,
    },
};
use cosmwasm_std::{
    to_json_binary, BankMsg, Coin, CosmosMsg, Decimal, Deps, Env, MessageInfo, Reply, Response,
    StdError, StdResult, SubMsg, SubMsgResult, Uint128,
};

use crate::{
    state::{FEE_COLLECTOR, IS_EXECUTING, MANAGER},
    types::Runnable,
};

pub const BASE_FEE_BPS: u64 = 15;

pub const EXECUTE_REPLY_ID: u64 = 1;
pub const SCHEDULE_REPLY_ID: u64 = 2;

fn get_swap_amount(deps: Deps, env: &Env, strategy: &DcaStrategyConfig) -> StdResult<Coin> {
    let balance = deps.querier.query_balance(
        env.contract.address.clone(),
        strategy.swap_amount.denom.clone(),
    )?;

    Ok(Coin {
        denom: strategy.swap_amount.denom.clone(),
        amount: min(balance.amount, strategy.swap_amount.amount),
    })
}

fn get_swap_message(strategy: &DcaStrategyConfig, deps: Deps, env: &Env) -> StdResult<SubMsg> {
    let swap_amount = get_swap_amount(deps, env, &strategy)?;

    let swap_msg = Contract(strategy.exchange_contract.clone()).call(
        to_json_binary(&ExchangeExecuteMsg::Swap {
            minimum_receive_amount: strategy.minimum_receive_amount.clone(),
            route: None,
        })?,
        vec![swap_amount],
    )?;

    Ok(SubMsg::reply_always(swap_msg, EXECUTE_REPLY_ID))
}

fn can_reschedule(strategy: &DcaStrategyConfig, deps: Deps, env: &Env) -> StdResult<bool> {
    let swap_amount = get_swap_amount(deps, env, strategy)?;

    if swap_amount.amount.is_zero() {
        return Err(StdError::generic_err(format!(
            "No remaining balance of {} to swap",
            strategy.swap_amount.denom
        )));
    }

    let rebate_balance = deps.querier.query_balance(
        env.contract.address.clone(),
        strategy.execution_rebate.denom.clone(),
    )?;

    if rebate_balance.amount < strategy.execution_rebate.amount {
        return Err(StdError::generic_err(format!(
            "Insufficient balance of {} ({}) to cover execution rebate",
            strategy.execution_rebate.denom, rebate_balance.amount
        )));
    }

    let triggers = deps.querier.query_wasm_smart::<Vec<Trigger>>(
        strategy.scheduler_contract.clone(),
        &SchedulerQueryMsg::Triggers {
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

    Ok(true)
}

fn get_schedule_msg(strategy: &DcaStrategyConfig, deps: Deps, env: &Env) -> StdResult<SubMsg> {
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

    let set_triggers_msg = Contract(strategy.scheduler_contract.clone()).call(
        to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
            condition: condition.clone(),
            to: MANAGER.load(deps.storage)?,
            msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                contract_address: env.contract.address.clone(),
            })?,
        }]))?,
        vec![strategy.execution_rebate.clone()],
    )?;

    Ok(SubMsg::reply_always(set_triggers_msg, SCHEDULE_REPLY_ID))
}

impl Runnable for DcaStrategyConfig {
    fn validate(&self, deps: Deps) -> StdResult<()> {
        deps.api.addr_validate(&self.owner.clone().into_string())?;

        let destinations = self
            .mutable_destinations
            .iter()
            .chain(self.immutable_destinations.iter())
            .collect::<Vec<_>>();

        if destinations.is_empty() {
            return Err(StdError::generic_err(
                "Must provide at least one destination",
            ));
        }

        if destinations.len() > 20 {
            return Err(StdError::generic_err(
                "Cannot provide more than 20 total destinations",
            ));
        }

        for destination in destinations.clone() {
            deps.api.addr_validate(&destination.address.to_string())?;
        }

        let total_shares = destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        if total_shares < Uint128::new(10_000) {
            return Err(StdError::generic_err(
                "Total shares must be at least 10,000",
            ));
        }

        if let Some(code) = self.affiliate_code.clone() {
            let affiliate = deps.querier.query_wasm_smart::<Affiliate>(
                MANAGER.load(deps.storage)?,
                &ManagerQueryMsg::Affiliate { code },
            )?;

            if affiliate.bps > 7 {
                return Err(StdError::generic_err(
                    "Affiliate BPS cannot be greater than 7",
                ));
            }
        }

        Ok(())
    }

    fn instantiate(&mut self, deps: Deps, env: Env, _info: MessageInfo) -> ContractResult {
        let total_shares = self
            .mutable_destinations
            .iter()
            .chain(self.immutable_destinations.iter())
            .into_iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        let total_shares_with_fee =
            checked_mul(total_shares, Decimal::permille(10_000 + BASE_FEE_BPS))?;

        let fee_destinations = match self.affiliate_code.clone() {
            Some(code) => {
                let affiliate = deps.querier.query_wasm_smart::<Affiliate>(
                    MANAGER.load(deps.storage)?,
                    &ManagerQueryMsg::Affiliate { code },
                )?;

                vec![
                    Destination {
                        address: FEE_COLLECTOR.load(deps.storage)?,
                        shares: checked_mul(
                            total_shares_with_fee,
                            Decimal::permille(BASE_FEE_BPS - affiliate.bps),
                        )?,
                        msg: None,
                        label: Some("CALC".to_string()),
                    },
                    Destination {
                        address: affiliate.address,
                        shares: checked_mul(
                            total_shares_with_fee,
                            Decimal::permille(affiliate.bps),
                        )?,
                        msg: None,
                        label: Some(format!("Affiliate: {}", affiliate.code).to_string()),
                    },
                ]
            }
            None => vec![Destination {
                address: FEE_COLLECTOR.load(deps.storage)?,
                shares: checked_mul(total_shares_with_fee, Decimal::permille(BASE_FEE_BPS))?,
                msg: None,
                label: Some("CALC".to_string()),
            }],
        };

        self.immutable_destinations =
            [fee_destinations, self.immutable_destinations.clone()].concat();

        self.validate(deps)?;

        let strategy_created_event = DomainEvent::StrategyCreated {
            contract_address: env.contract.address.clone(),
            config: StrategyConfig::Dca(self.clone()),
        };

        let mut messages: Vec<CosmosMsg> = vec![];

        if self.can_execute(deps, &env).is_ok() {
            messages.push(
                Contract(env.contract.address)
                    .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, vec![])?,
            );
        }

        Ok(Response::default()
            .add_messages(messages)
            .add_event(strategy_created_event))
    }

    fn update(&mut self, deps: Deps, env: Env, update: StrategyConfig) -> ContractResult {
        match update {
            StrategyConfig::Dca(update) => {
                let mutable_shares_old = self
                    .mutable_destinations
                    .iter()
                    .fold(Uint128::zero(), |acc, d| acc + d.shares);

                let mutable_shares_new = update
                    .mutable_destinations
                    .iter()
                    .fold(Uint128::zero(), |acc, d| acc + d.shares);

                if mutable_shares_new != mutable_shares_old {
                    return Err(ContractError::Generic(
                        "Updated total shares must match the original total shares",
                    ));
                }

                let previous_config = self.clone();

                self.swap_amount = update.swap_amount;
                self.minimum_receive_amount = update.minimum_receive_amount;
                self.schedule = update.schedule;
                self.mutable_destinations = update.mutable_destinations;
                self.execution_rebate = update.execution_rebate;

                self.validate(deps)?;

                let mut sub_messages: Vec<SubMsg> = vec![];

                if previous_config.schedule != self.schedule {
                    let schedule_msg = get_schedule_msg(self, deps, &env)?;

                    sub_messages.push(schedule_msg);
                }

                let strategy_updated_event = DomainEvent::StrategyUpdated {
                    contract_address: env.contract.address,
                    old_config: StrategyConfig::Dca(previous_config),
                    new_config: StrategyConfig::Dca(self.clone()),
                };

                Ok(Response::default()
                    .add_submessages(sub_messages)
                    .add_event(strategy_updated_event))
            }
            _ => Err(ContractError::Std(StdError::generic_err(
                "Invalid update type for DCA strategy",
            ))),
        }
    }

    fn can_execute(&self, deps: Deps, env: &Env) -> StdResult<()> {
        if IS_EXECUTING.load(deps.storage)? {
            return Err(StdError::generic_err(
                "Strategy is already executing, cannot execute again",
            ));
        }

        let swap_amount = get_swap_amount(deps, env, self)?;

        if swap_amount.amount.is_zero() {
            return Err(StdError::generic_err(format!(
                "Insufficient swap amount of {} ({}) to cover gas fees",
                self.swap_amount.denom, swap_amount.amount
            )));
        }

        let strategy = deps.querier.query_wasm_smart::<Strategy>(
            MANAGER.load(deps.storage)?,
            &ManagerQueryMsg::Strategy {
                address: env.contract.address.clone(),
            },
        )?;

        if strategy.status != Status::Active {
            return Err(StdError::generic_err(format!(
                "Strategy is not active, current status: {:?}",
                strategy.status
            )));
        }

        let triggers = deps.querier.query_wasm_smart::<Vec<Trigger>>(
            self.scheduler_contract.clone(),
            &SchedulerQueryMsg::Triggers {
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

    fn execute(&mut self, deps: Deps, env: Env) -> ContractResult {
        let mut sub_messages: Vec<SubMsg> = vec![];
        let mut events: Vec<DomainEvent> = vec![];

        match self.can_execute(deps, &env) {
            Ok(_) => {
                sub_messages.push(get_swap_message(self, deps, &env)?);
            }
            Err(err) => {
                if can_reschedule(&self, deps, &env)? {
                    sub_messages.push(get_schedule_msg(self, deps, &env)?);
                }

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

    fn handle_reply(&mut self, deps: Deps, env: Env, reply: Reply) -> ContractResult {
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

                match self.can_execute(deps, &env) {
                    Ok(_) => {
                        sub_messages.push(get_schedule_msg(self, deps, &env)?);
                    }
                    Err(_) => {
                        let pause_strategy_msg = Contract(MANAGER.load(deps.storage)?).call(
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
                let mut messages: Vec<CosmosMsg> = vec![];

                match reply.result {
                    SubMsgResult::Ok(_) => {
                        let triggers = deps.querier.query_wasm_smart::<Vec<Trigger>>(
                            self.scheduler_contract.clone(),
                            &SchedulerQueryMsg::Triggers {
                                filter: ConditionFilter::Owner {
                                    address: env.contract.address.clone(),
                                },
                                limit: None,
                            },
                        )?;

                        events.push(DomainEvent::SchedulingSucceeded {
                            contract_address: env.contract.address.clone(),
                            conditions: triggers.iter().map(|t| t.condition.clone()).collect(),
                        });
                    }
                    SubMsgResult::Err(reason) => {
                        messages.push(Contract(self.scheduler_contract.clone()).call(
                            to_json_binary(&ManagerExecuteMsg::UpdateStatus {
                                status: Status::Paused,
                            })?,
                            vec![],
                        )?);

                        events.push(DomainEvent::SchedulingFailed {
                            contract_address: env.contract.address.clone(),
                            reason,
                        });

                        events.push(DomainEvent::StrategyPaused {
                            contract_address: env.contract.address.clone(),
                            reason: "Failed to schedule next execution".into(),
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

    fn deposit(&mut self, _deps: Deps, env: Env, info: MessageInfo) -> ContractResult {
        if info.funds.is_empty() {
            return Err(ContractError::Std(StdError::generic_err(
                "Must provide at least one coin to deposit",
            )));
        }

        if info.funds.len() > 1 {
            return Err(ContractError::Std(StdError::generic_err(
                "Must provide exactly one coin to deposit",
            )));
        }

        let amount = info.funds[0].amount;

        if amount.is_zero() {
            return Err(ContractError::Std(StdError::generic_err(
                "Must provide a non-zero amount to deposit",
            )));
        }

        self.statistics.amount_deposited = Coin {
            denom: self.statistics.amount_deposited.denom.clone(),
            amount: self.statistics.amount_deposited.amount + info.funds[0].amount,
        };

        let funds_deposited_event = DomainEvent::FundsDeposited {
            contract_address: env.contract.address,
            from: info.sender,
            funds: info.funds,
        };

        Ok(Response::default().add_event(funds_deposited_event))
    }

    fn pause(&mut self, deps: Deps, env: Env) -> ContractResult {
        let delete_conditions_msg = Contract(self.scheduler_contract.clone()).call(
            to_json_binary(&SchedulerExecuteMsg::DeleteTriggers {})?,
            vec![],
        )?;

        let pause_strategy_msg = Contract(MANAGER.load(deps.storage)?).call(
            to_json_binary(&ManagerExecuteMsg::UpdateStatus {
                status: Status::Paused,
            })?,
            vec![],
        )?;

        let strategy_paused_event = DomainEvent::StrategyPaused {
            contract_address: env.contract.address,
            reason: "User requested pause".into(),
        };

        Ok(Response::default()
            .add_messages(vec![delete_conditions_msg, pause_strategy_msg])
            .add_event(strategy_paused_event))
    }

    fn resume(&mut self, deps: Deps, env: Env) -> ContractResult {
        let mut sub_messages: Vec<SubMsg> = vec![];

        match self.can_execute(deps, &env) {
            Ok(_) => {
                sub_messages.push(get_schedule_msg(self, deps, &env)?);
            }
            Err(err) => {
                return Err(ContractError::Std(StdError::generic_err(format!(
                    "Cannot resume strategy: {}",
                    err
                ))));
            }
        }

        let resume_strategy_msg = Contract(MANAGER.load(deps.storage)?).call(
            to_json_binary(&ManagerExecuteMsg::UpdateStatus {
                status: Status::Active,
            })?,
            vec![],
        )?;

        let strategy_resumed_event = DomainEvent::StrategyResumed {
            contract_address: env.contract.address,
        };

        Ok(Response::default()
            .add_submessages(sub_messages)
            .add_message(resume_strategy_msg)
            .add_event(strategy_resumed_event))
    }

    fn withdraw(&mut self, deps: Deps, env: Env, amounts: Vec<Coin>) -> ContractResult {
        let funds = amounts
            .iter()
            .filter_map(|amount| {
                match deps
                    .querier
                    .query_balance(env.contract.address.clone(), amount.denom.clone())
                {
                    Ok(balance) if !balance.amount.is_zero() => Some(Ok(balance)),
                    Ok(_) => None,
                    Err(e) => Some(Err(e)),
                }
            })
            .collect::<StdResult<Vec<Coin>>>()?;

        let send_assets_msg = BankMsg::Send {
            to_address: self.owner.to_string(),
            amount: funds.clone(),
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

    fn statistics(&self) -> StrategyStatistics {
        StrategyStatistics::Dca(self.statistics.clone())
    }
}
