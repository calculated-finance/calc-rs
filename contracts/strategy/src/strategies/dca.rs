use std::cmp::min;

use calc_rs::types::{
    Affiliate, Condition, ConditionFilter, Contract, ContractError, ContractResult, CreateTrigger,
    DcaSchedule, DcaStatistics, DcaStrategyConfig, Destination, DomainEvent, ExchangeExecuteMsg,
    ManagerExecuteMsg, ManagerQueryMsg, SchedulerExecuteMsg, SchedulerQueryMsg, Strategy,
    StrategyConfig, StrategyExecuteMsg, StrategyStatistics, StrategyStatus, Trigger,
};
use cosmwasm_std::{
    from_json, to_json_binary, BankMsg, Coin, CosmosMsg, Decimal, Deps, Env, MessageInfo, Reply,
    Response, StdError, StdResult, SubMsg, SubMsgResult, Uint128,
};

use crate::{
    state::{FEE_COLLECTOR, MANAGER},
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
    );

    Ok(SubMsg::reply_always(set_triggers_msg, SCHEDULE_REPLY_ID))
}

impl Runnable for DcaStrategyConfig {
    fn instantiate(&mut self, deps: Deps, env: &Env, _info: &MessageInfo) -> ContractResult {
        let total_shares = self
            .mutable_destinations
            .iter()
            .chain(self.immutable_destinations.iter())
            .into_iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        let total_shares_with_fee = total_shares.mul_ceil(Decimal::permille(10_000 + BASE_FEE_BPS));

        let fee_destinations = match self.affiliate_code.clone() {
            Some(code) => {
                let affiliate = deps.querier.query_wasm_smart::<Affiliate>(
                    MANAGER.load(deps.storage)?,
                    &ManagerQueryMsg::Affiliate { code },
                )?;

                vec![
                    Destination {
                        address: FEE_COLLECTOR.load(deps.storage)?,
                        shares: total_shares_with_fee
                            .mul_ceil(Decimal::permille(BASE_FEE_BPS - affiliate.bps)),
                        msg: None,
                        label: Some("CALC".to_string()),
                    },
                    Destination {
                        address: affiliate.address,
                        shares: total_shares_with_fee.mul_floor(Decimal::permille(affiliate.bps)),
                        msg: None,
                        label: Some(format!("Affiliate: {}", affiliate.code).to_string()),
                    },
                ]
            }
            None => vec![Destination {
                address: FEE_COLLECTOR.load(deps.storage)?,
                shares: total_shares_with_fee.mul_ceil(Decimal::permille(BASE_FEE_BPS)),
                msg: None,
                label: Some("CALC".to_string()),
            }],
        };

        self.immutable_destinations =
            [fee_destinations, self.immutable_destinations.clone()].concat();

        let strategy_instantiated_event = DomainEvent::StrategyInstantiated {
            contract_address: env.contract.address.clone(),
            config: StrategyConfig::Dca(self.clone()),
        };

        let mut messages: Vec<CosmosMsg> = vec![];

        if self.can_execute(deps, &env).is_ok() {
            messages.push(
                Contract(env.contract.address.clone())
                    .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, vec![]),
            );
        }

        Ok(Response::default()
            .add_messages(messages)
            .add_event(strategy_instantiated_event))
    }

    fn validate(&self, deps: Deps) -> StdResult<()> {
        deps.api
            .addr_validate(&self.owner.clone().into_string())
            .map_err(|_| {
                StdError::generic_err(format!(
                    "Invalid owner address: {}",
                    self.owner.clone().into_string()
                ))
            })?;

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
            deps.api
                .addr_validate(&destination.address.to_string())
                .map_err(|_| {
                    StdError::generic_err(format!(
                        "Invalid destination address: {}",
                        destination.address
                    ))
                })?;
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
            let affiliate = deps
                .querier
                .query_wasm_smart::<Affiliate>(
                    MANAGER.load(deps.storage)?,
                    &ManagerQueryMsg::Affiliate { code: code.clone() },
                )
                .map_err(|_| StdError::generic_err(format!("Invalid affiliate code: {}", code)))?;

            if affiliate.bps > 7 {
                return Err(StdError::generic_err(
                    "Affiliate BPS cannot be greater than 7",
                ));
            }
        }

        Ok(())
    }

    fn update(&mut self, deps: Deps, env: &Env, update: StrategyConfig) -> ContractResult {
        match update {
            StrategyConfig::Dca(update) => {
                if (update.swap_amount.denom != self.swap_amount.denom)
                    || (update.minimum_receive_amount.denom != self.minimum_receive_amount.denom)
                {
                    return Err(ContractError::Std(StdError::generic_err(
                        "Cannot change swap or receive denom",
                    )));
                }

                let mutable_shares_old = self
                    .mutable_destinations
                    .iter()
                    .fold(Uint128::zero(), |acc, d| acc + d.shares);

                let mutable_shares_new = update
                    .mutable_destinations
                    .iter()
                    .fold(Uint128::zero(), |acc, d| acc + d.shares);

                if mutable_shares_new != mutable_shares_old {
                    return Err(ContractError::Std(StdError::generic_err(format!(
                        "Updated total shares ({}) must match the original total shares ({})",
                        mutable_shares_new, mutable_shares_old
                    ))));
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
                    contract_address: env.contract.address.clone(),
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

        if strategy.status != StrategyStatus::Active {
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
                can_execute: Some(false),
            },
        )?;

        if !triggers.is_empty() {
            return Err(StdError::generic_err(format!(
                "Condition for execution not met: {:?}",
                triggers[0].condition
            )));
        }

        Ok(())
    }

    fn execute(&mut self, deps: Deps, env: &Env) -> ContractResult {
        let mut sub_messages: Vec<SubMsg> = vec![];
        let mut events: Vec<DomainEvent> = vec![];

        match self.can_execute(deps, env) {
            Ok(_) => {
                let swap_amount = get_swap_amount(deps, env, &self)?;

                let swap_msg = Contract(self.exchange_contract.clone()).call(
                    to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: self.minimum_receive_amount.clone(),
                        recipient: None,
                    })?,
                    vec![swap_amount.clone()],
                );

                sub_messages.push(
                    SubMsg::reply_always(swap_msg, EXECUTE_REPLY_ID)
                        .with_payload(to_json_binary(&swap_amount)?),
                );
            }
            Err(reason) => {
                events.push(DomainEvent::ExecutionSkipped {
                    contract_address: env.contract.address.clone(),
                    reason: reason.to_string(),
                });
            }
        }

        Ok(Response::new()
            .add_submessages(sub_messages)
            .add_events(events))
    }

    fn handle_reply(&mut self, deps: Deps, env: &Env, reply: Reply) -> ContractResult {
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
                                        amount: target_denom_balance
                                            .amount
                                            .mul_floor(Decimal::from_ratio(d.shares, total_shares)),
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
                                denom: swap_denom_balance.denom.clone(),
                                amount: self.statistics.amount_swapped.amount.checked_add(
                                    from_json(&reply.payload)
                                        .unwrap_or(Coin::new(0u128, swap_denom_balance.denom))
                                        .amount,
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
                    Err(reason) => {
                        let pause_strategy_msg = Contract(MANAGER.load(deps.storage)?).call(
                            to_json_binary(&ManagerExecuteMsg::UpdateStatus {
                                status: StrategyStatus::Paused,
                            })?,
                            vec![],
                        );

                        messages.push(pause_strategy_msg);

                        let scheduling_skipped_event = DomainEvent::SchedulingSkipped {
                            contract_address: env.contract.address.clone(),
                            reason: reason.to_string(),
                        };

                        let strategy_paused_event = DomainEvent::StrategyPaused {
                            contract_address: env.contract.address.clone(),
                            reason: reason.to_string(),
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
                                can_execute: None,
                            },
                        )?;

                        events.push(DomainEvent::SchedulingSucceeded {
                            contract_address: env.contract.address.clone(),
                            conditions: triggers.iter().map(|t| t.condition.clone()).collect(),
                        });
                    }
                    SubMsgResult::Err(reason) => {
                        messages.push(Contract(MANAGER.load(deps.storage)?).call(
                            to_json_binary(&ManagerExecuteMsg::UpdateStatus {
                                status: StrategyStatus::Paused,
                            })?,
                            vec![],
                        ));

                        events.push(DomainEvent::SchedulingFailed {
                            contract_address: env.contract.address.clone(),
                            reason: reason.clone(),
                        });

                        events.push(DomainEvent::StrategyPaused {
                            contract_address: env.contract.address.clone(),
                            reason,
                        });
                    }
                }

                Ok(Response::new().add_messages(messages).add_events(events))
            }
            _ => Err(ContractError::Std(StdError::generic_err(
                "invalid reply id",
            ))),
        }
    }

    fn deposit(&mut self, _deps: Deps, env: &Env, info: &MessageInfo) -> ContractResult {
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
            contract_address: env.contract.address.clone(),
            from: info.sender.clone(),
            funds: info.funds.clone(),
        };

        Ok(Response::default().add_event(funds_deposited_event))
    }

    fn pause(&mut self, deps: Deps, env: &Env) -> ContractResult {
        let delete_conditions_msg = Contract(self.scheduler_contract.clone()).call(
            to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![]))?,
            vec![],
        );

        let pause_strategy_msg = Contract(MANAGER.load(deps.storage)?).call(
            to_json_binary(&ManagerExecuteMsg::UpdateStatus {
                status: StrategyStatus::Paused,
            })?,
            vec![],
        );

        let strategy_paused_event = DomainEvent::StrategyPaused {
            contract_address: env.contract.address.clone(),
            reason: "User requested pause".into(),
        };

        Ok(Response::default()
            .add_messages(vec![delete_conditions_msg, pause_strategy_msg])
            .add_event(strategy_paused_event))
    }

    fn resume(&mut self, deps: Deps, env: &Env) -> ContractResult {
        let mut sub_messages: Vec<SubMsg> = vec![];

        match self.can_execute(deps, env) {
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
                status: StrategyStatus::Active,
            })?,
            vec![],
        );

        let strategy_resumed_event = DomainEvent::StrategyResumed {
            contract_address: env.contract.address.clone(),
        };

        Ok(Response::default()
            .add_submessages(sub_messages)
            .add_message(resume_strategy_msg)
            .add_event(strategy_resumed_event))
    }

    fn withdraw(&mut self, deps: Deps, env: &Env, amounts: Vec<Coin>) -> ContractResult {
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
            contract_address: env.contract.address.clone(),
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

#[cfg(test)]
fn default_strategy_config() -> DcaStrategyConfig {
    use cosmwasm_std::testing::mock_dependencies;

    let deps = mock_dependencies();
    DcaStrategyConfig {
        owner: deps.api.addr_make("owner"),
        swap_amount: Coin {
            denom: "rune".to_string(),
            amount: Uint128::new(1000),
        },
        minimum_receive_amount: Coin {
            denom: "btc-btc".to_string(),
            amount: Uint128::new(900),
        },
        schedule: DcaSchedule::Blocks {
            interval: 10,
            previous: None,
        },
        mutable_destinations: vec![Destination {
            address: deps.api.addr_make("mutable_destination"),
            shares: Uint128::new(5000),
            msg: None,
            label: Some("Mutable Destination".to_string()),
        }],
        immutable_destinations: vec![Destination {
            address: deps.api.addr_make("immutable_destination"),
            shares: Uint128::new(5000),
            msg: None,
            label: Some("Immutable Destination".to_string()),
        }],
        execution_rebate: Coin {
            denom: "rune".to_string(),
            amount: Uint128::new(10),
        },
        affiliate_code: None,
        statistics: DcaStatistics {
            amount_swapped: Coin {
                denom: "rune".to_string(),
                amount: Uint128::zero(),
            },
            amount_received: Coin {
                denom: "btc-btc".to_string(),
                amount: Uint128::zero(),
            },
            amount_deposited: Coin {
                denom: "rune".to_string(),
                amount: Uint128::zero(),
            },
        },
        exchange_contract: deps.api.addr_make("exchange"),
        scheduler_contract: deps.api.addr_make("scheduler"),
    }
}

#[cfg(test)]
mod get_swap_amount_tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env};

    #[test]
    fn returns_0_when_balance_is_empty() {
        let deps = mock_dependencies();
        let env = mock_env();

        let strategy = default_strategy_config();

        assert_eq!(
            get_swap_amount(deps.as_ref(), &env, &strategy).unwrap(),
            Coin {
                denom: strategy.swap_amount.denom,
                amount: Uint128::zero()
            }
        );
    }

    #[test]
    fn returns_swap_amount_when_balance_is_larger() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let strategy = default_strategy_config();

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![Coin {
                denom: strategy.swap_amount.denom.clone(),
                amount: strategy.swap_amount.amount * Uint128::new(2),
            }],
        );

        assert_eq!(
            get_swap_amount(deps.as_ref(), &env, &strategy).unwrap(),
            strategy.swap_amount
        );
    }

    #[test]
    fn returns_balance_when_swap_amount_is_larger() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let strategy = default_strategy_config();

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![Coin {
                denom: strategy.swap_amount.denom.clone(),
                amount: strategy.swap_amount.amount.mul_floor(Decimal::percent(50)),
            }],
        );

        assert_eq!(
            get_swap_amount(deps.as_ref(), &env, &strategy).unwrap(),
            Coin {
                denom: strategy.swap_amount.denom,
                amount: strategy.swap_amount.amount.mul_floor(Decimal::percent(50))
            }
        );
    }
}

#[cfg(test)]
mod get_schedule_msg_tests {
    use std::time::Duration;

    use super::*;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Timestamp,
    };
    use rstest::rstest;

    #[rstest]
    #[case(0, None)]
    #[case(0, Some(0))]
    #[case(0, Some(10000000))]
    #[case(1, None)]
    #[case(1, Some(0))]
    #[case(1, Some(10000000))]
    #[case(10000000, None)]
    #[case(10000000, Some(0))]
    #[case(10000000, Some(10000000))]
    fn returns_block_height_condition(#[case] interval: u64, #[case] previous: Option<u64>) {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager = deps.api.addr_make("manager");
        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        let strategy = DcaStrategyConfig {
            schedule: DcaSchedule::Blocks { interval, previous },
            ..default_strategy_config()
        };

        let msg = get_schedule_msg(&strategy, deps.as_ref(), &env).unwrap();

        assert_eq!(
            msg,
            SubMsg::reply_always(
                Contract(strategy.scheduler_contract.clone()).call(
                    to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
                        condition: Condition::BlockHeight {
                            height: previous.unwrap_or(env.block.height) + interval,
                        },
                        to: MANAGER.load(deps.as_ref().storage).unwrap(),
                        msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                            contract_address: env.contract.address.clone(),
                        })
                        .unwrap(),
                    }]))
                    .unwrap(),
                    vec![strategy.execution_rebate.clone()]
                ),
                SCHEDULE_REPLY_ID
            )
        )
    }

    #[rstest]
    #[case(Duration::from_secs(0), None)]
    #[case(Duration::from_secs(0), Some(Timestamp::from_seconds(0)))]
    #[case(Duration::from_secs(0), Some(Timestamp::from_seconds(10000000)))]
    #[case(Duration::from_secs(1), None)]
    #[case(Duration::from_secs(1), Some(Timestamp::from_seconds(0)))]
    #[case(Duration::from_secs(1), Some(Timestamp::from_seconds(10000000)))]
    #[case(Duration::from_secs(10000000), None)]
    #[case(Duration::from_secs(10000000), Some(Timestamp::from_seconds(0)))]
    #[case(Duration::from_secs(10000000), Some(Timestamp::from_seconds(10000000)))]
    fn returns_time_condition(#[case] duration: Duration, #[case] previous: Option<Timestamp>) {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager = deps.api.addr_make("manager");
        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        let strategy = DcaStrategyConfig {
            schedule: DcaSchedule::Time { duration, previous },
            ..default_strategy_config()
        };

        let msg = get_schedule_msg(&strategy, deps.as_ref(), &env).unwrap();

        assert_eq!(
            msg,
            SubMsg::reply_always(
                Contract(strategy.scheduler_contract.clone()).call(
                    to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
                        condition: Condition::Timestamp {
                            timestamp: previous
                                .unwrap_or(env.block.time)
                                .plus_seconds(duration.as_secs())
                        },
                        to: MANAGER.load(deps.as_ref().storage).unwrap(),
                        msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                            contract_address: env.contract.address.clone(),
                        })
                        .unwrap(),
                    }]))
                    .unwrap(),
                    vec![strategy.execution_rebate.clone()]
                ),
                SCHEDULE_REPLY_ID
            )
        )
    }
}

#[cfg(test)]
mod instantiate_tests {
    use super::*;
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        ContractResult, Event, SystemResult, WasmQuery,
    };

    #[test]
    fn succeeds_with_valid_config() {
        let mut deps = mock_dependencies();

        let manager = deps.api.addr_make("manager");
        let fee_collector = deps.api.addr_make("fee_collector");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();
        FEE_COLLECTOR
            .save(deps.as_mut().storage, &fee_collector)
            .unwrap();

        let info = message_info(&manager, &[]);

        let response = default_strategy_config()
            .instantiate(deps.as_ref(), &mock_env(), &info)
            .unwrap();

        assert!(response.messages.is_empty());
    }

    #[test]
    fn adds_fee_taker_destination() {
        let mut deps = mock_dependencies();

        let manager = deps.api.addr_make("manager");
        let fee_collector = deps.api.addr_make("fee_collector");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();
        FEE_COLLECTOR
            .save(deps.as_mut().storage, &fee_collector)
            .unwrap();

        let info = message_info(&manager, &[]);

        let mut strategy = DcaStrategyConfig {
            mutable_destinations: vec![Destination {
                address: deps.api.addr_make("mutable_destination"),
                shares: Uint128::new(20_000),
                msg: None,
                label: Some("Mutable Destination".to_string()),
            }],
            immutable_destinations: vec![],
            ..default_strategy_config()
        };

        strategy
            .instantiate(deps.as_ref(), &mock_env(), &info)
            .unwrap();

        assert_eq!(
            strategy.immutable_destinations,
            vec![Destination {
                address: fee_collector,
                shares: Uint128::new(3005),
                msg: None,
                label: Some("CALC".to_string()),
            }]
        );
    }

    #[test]
    fn adds_affiliate_destination() {
        let mut deps = mock_dependencies();

        let manager = deps.api.addr_make("manager");
        let fee_collector = deps.api.addr_make("fee_collector");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();
        FEE_COLLECTOR
            .save(deps.as_mut().storage, &fee_collector)
            .unwrap();

        let info = message_info(&manager, &[]);

        let affiliate_code = "affiliate_code".to_string();
        let affiliate_address = deps.api.addr_make("affiliate");

        let affiliate = Affiliate {
            address: affiliate_address.clone(),
            bps: 7,
            code: affiliate_code.clone(),
        };

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(to_json_binary(&affiliate).unwrap()))
        });

        let mut strategy = DcaStrategyConfig {
            affiliate_code: Some(affiliate_code),
            mutable_destinations: vec![Destination {
                address: deps.api.addr_make("mutable_destination"),
                shares: Uint128::new(20_000),
                msg: None,
                label: Some("Mutable Destination".to_string()),
            }],
            immutable_destinations: vec![],
            ..default_strategy_config()
        };

        strategy
            .instantiate(deps.as_ref(), &mock_env(), &info)
            .unwrap();

        assert_eq!(
            strategy.immutable_destinations,
            vec![
                Destination {
                    address: fee_collector,
                    shares: Uint128::new(1603),
                    msg: None,
                    label: Some("CALC".to_string()),
                },
                Destination {
                    address: affiliate_address,
                    shares: Uint128::new(1402),
                    msg: None,
                    label: Some("Affiliate: affiliate_code".to_string()),
                }
            ]
        );
    }

    #[test]
    fn publishes_strategy_instantiated_event() {
        let mut deps = mock_dependencies();

        let manager = deps.api.addr_make("manager");
        let fee_collector = deps.api.addr_make("fee_collector");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();
        FEE_COLLECTOR
            .save(deps.as_mut().storage, &fee_collector)
            .unwrap();

        let env = mock_env();
        let info = message_info(&manager, &[]);

        let mut strategy = default_strategy_config();

        let response = strategy.instantiate(deps.as_ref(), &env, &info).unwrap();

        assert_eq!(
            response.events[0],
            Event::from(DomainEvent::StrategyInstantiated {
                contract_address: env.contract.address.clone(),
                config: StrategyConfig::Dca(strategy),
            })
        );
    }

    #[test]
    fn adds_execute_message() {
        let mut deps = mock_dependencies();

        let manager_address = deps.api.addr_make("manager");
        let fee_collector = deps.api.addr_make("fee_collector");

        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();
        FEE_COLLECTOR
            .save(deps.as_mut().storage, &fee_collector)
            .unwrap();

        let mut strategy = default_strategy_config();

        let env = mock_env();
        let info = message_info(&manager_address, &[]);

        let strategy_address = env.contract.address.clone();
        let scheduler_address = strategy.scheduler_contract.clone();

        deps.querier
            .bank
            .update_balance(strategy_address.clone(), vec![strategy.swap_amount.clone()]);

        deps.querier.update_wasm(move |query| match query {
            WasmQuery::Smart { contract_addr, .. } => {
                if contract_addr == &manager_address.to_string() {
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary(&Strategy {
                            status: StrategyStatus::Active,
                            owner: default_strategy_config().owner.clone(),
                            contract_address: strategy_address.clone(),
                            created_at: env.block.time.seconds(),
                            updated_at: env.block.time.seconds(),
                            executions: 0,
                            label: "Test Strategy".to_string(),
                            affiliates: vec![],
                        })
                        .unwrap(),
                    ))
                } else if contract_addr == &scheduler_address.to_string() {
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary::<Vec<Trigger>>(&vec![]).unwrap(),
                    ))
                } else {
                    panic!("Unexpected contract address: {}", contract_addr);
                }
            }
            _ => panic!("Unexpected query: {:?}", query),
        });

        let response = strategy.instantiate(deps.as_ref(), &env, &info).unwrap();

        assert_eq!(
            response.messages[0].msg,
            Contract(env.contract.address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::Execute {}).unwrap(),
                vec![]
            )
        );
    }
}

#[cfg(test)]
mod validate_tests {
    use super::*;
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies},
        Addr, ContractResult, SystemResult,
    };

    #[test]
    fn invalid_owner_fails() {
        let deps = mock_dependencies();

        let info = message_info(&Addr::unchecked("owner"), &[]);
        let strategy = DcaStrategyConfig {
            owner: info.sender,
            ..default_strategy_config()
        };

        assert_eq!(
            strategy.validate(deps.as_ref()).unwrap_err().to_string(),
            "Generic error: Invalid owner address: owner"
        );
    }

    #[test]
    fn no_destinations_fails() {
        let deps = mock_dependencies();

        let strategy = DcaStrategyConfig {
            mutable_destinations: vec![],
            immutable_destinations: vec![],
            ..default_strategy_config()
        };

        assert_eq!(
            strategy.validate(deps.as_ref()).unwrap_err().to_string(),
            "Generic error: Must provide at least one destination"
        );
    }

    #[test]
    fn too_many_destinations_fails() {
        let deps = mock_dependencies();

        let destinations: Vec<Destination> = (0..21)
            .map(|i| Destination {
                address: deps.api.addr_make(&format!("destination{}", i)),
                shares: Uint128::new(10000),
                msg: None,
                label: Some(format!("Destination {}", i)),
            })
            .collect();

        let strategy = DcaStrategyConfig {
            mutable_destinations: destinations.clone(),
            ..default_strategy_config()
        };

        assert_eq!(
            strategy.validate(deps.as_ref()).unwrap_err().to_string(),
            "Generic error: Cannot provide more than 20 total destinations"
        );
    }

    #[test]
    fn invalid_destination_address_fails() {
        let deps = mock_dependencies();

        let strategy = DcaStrategyConfig {
            mutable_destinations: vec![Destination {
                address: Addr::unchecked("invalid_address"),
                shares: Uint128::new(10000),
                msg: None,
                label: Some("Invalid Destination".to_string()),
            }],
            ..default_strategy_config()
        };

        assert_eq!(
            strategy.validate(deps.as_ref()).unwrap_err().to_string(),
            "Generic error: Invalid destination address: invalid_address"
        );
    }

    #[test]
    fn total_shares_below_minimum_fails() {
        let deps = mock_dependencies();

        let strategy = DcaStrategyConfig {
            mutable_destinations: vec![Destination {
                address: deps.api.addr_make("destination"),
                shares: Uint128::new(5000),
                msg: None,
                label: Some("Destination".to_string()),
            }],
            immutable_destinations: vec![],
            ..default_strategy_config()
        };

        assert_eq!(
            strategy.validate(deps.as_ref()).unwrap_err().to_string(),
            "Generic error: Total shares must be at least 10,000"
        );
    }

    #[test]
    fn missing_affiliate_code_fails() {
        let mut deps = mock_dependencies();

        let strategy = DcaStrategyConfig {
            affiliate_code: Some("invalid_code".to_string()),
            ..default_strategy_config()
        };

        let manager = deps.api.addr_make("manager");
        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Err("Affiliate code not found".to_string()))
        });

        assert_eq!(
            strategy.validate(deps.as_ref()).unwrap_err().to_string(),
            "Generic error: Invalid affiliate code: invalid_code"
        );
    }

    #[test]
    fn affiliate_bps_too_high_fails() {
        let mut deps = mock_dependencies();
        let strategy = DcaStrategyConfig {
            affiliate_code: Some("high_bps_code".to_string()),
            ..default_strategy_config()
        };

        let manager = deps.api.addr_make("manager");
        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&Affiliate {
                    address: deps.api.addr_make("affiliate"),
                    bps: 8,
                    code: "high_bps_code".to_string(),
                })
                .unwrap(),
            ))
        });

        assert_eq!(
            strategy.validate(deps.as_ref()).unwrap_err().to_string(),
            "Generic error: Affiliate BPS cannot be greater than 7"
        );
    }

    #[test]
    fn valid_strategy_passes() {
        let mut deps = mock_dependencies();

        let manager = deps.api.addr_make("manager");
        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        let affiliate_code = "affiliate".to_string();

        let affiliate = Affiliate {
            address: deps.api.addr_make("affiliate"),
            bps: 7,
            code: affiliate_code.clone(),
        };

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(to_json_binary(&affiliate).unwrap()))
        });

        let strategy = DcaStrategyConfig {
            affiliate_code: Some(affiliate_code),
            ..default_strategy_config()
        };

        assert!(strategy.validate(deps.as_ref()).is_ok());
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Event,
    };

    #[test]
    fn fails_to_update_swap_denom() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager = deps.api.addr_make("manager");
        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        let mut strategy = default_strategy_config();

        let update = DcaStrategyConfig {
            swap_amount: Coin {
                denom: "new-denom".to_string(),
                amount: Uint128::new(1000),
            },
            ..strategy.clone()
        };

        assert_eq!(
            strategy
                .update(deps.as_ref(), &env, StrategyConfig::Dca(update))
                .unwrap_err()
                .to_string(),
            "Generic error: Cannot change swap or receive denom"
        );
    }

    #[test]
    fn fails_to_update_receive_denom() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager = deps.api.addr_make("manager");
        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        let mut strategy = default_strategy_config();

        let update = DcaStrategyConfig {
            minimum_receive_amount: Coin {
                denom: "new-denom".to_string(),
                amount: Uint128::new(1000),
            },
            ..strategy.clone()
        };

        assert_eq!(
            strategy
                .update(deps.as_ref(), &env, StrategyConfig::Dca(update))
                .unwrap_err()
                .to_string(),
            "Generic error: Cannot change swap or receive denom"
        );
    }

    #[test]
    fn fails_to_update_mutable_shares() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager = deps.api.addr_make("manager");
        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        let mut strategy = default_strategy_config();

        let update = DcaStrategyConfig {
            mutable_destinations: vec![Destination {
                address: deps.api.addr_make("mutable_destination_updated"),
                shares: Uint128::new(823764283),
                msg: None,
                label: Some("Updated Mutable Destination".to_string()),
            }],
            ..strategy.clone()
        };

        assert_eq!(
            strategy
                .update(deps.as_ref(), &env, StrategyConfig::Dca(update.clone()))
                .unwrap_err()
                .to_string(),
            format!(
                "Generic error: Updated total shares ({}) must match the original total shares ({})",
                update
                    .mutable_destinations
                    .iter()
                    .map(|d| d.shares)
                    .sum::<Uint128>(),
                strategy
                    .mutable_destinations
                    .iter()
                    .map(|d| d.shares)
                    .sum::<Uint128>()
            ),
        );
    }

    #[test]
    fn update_succeeds_with_valid_config() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager = deps.api.addr_make("manager");
        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        let mut strategy = default_strategy_config();

        let update = DcaStrategyConfig {
            swap_amount: Coin {
                denom: "rune".to_string(),
                amount: Uint128::new(378269234),
            },
            minimum_receive_amount: Coin {
                denom: "btc-btc".to_string(),
                amount: Uint128::new(23742342),
            },
            schedule: DcaSchedule::Blocks {
                interval: 128123,
                previous: None,
            },
            mutable_destinations: vec![Destination {
                address: deps.api.addr_make("mutable_destination_updated"),
                shares: Uint128::new(5000),
                msg: None,
                label: Some("Updated Mutable Destination".to_string()),
            }],
            execution_rebate: Coin {
                denom: "usdc".to_string(),
                amount: Uint128::new(834794),
            },
            ..strategy.clone()
        };

        let response = strategy
            .update(
                deps.as_ref(),
                &mock_env(),
                StrategyConfig::Dca(update.clone()),
            )
            .unwrap();

        assert_eq!(strategy, update);
        assert_eq!(
            response,
            Response::default()
                .add_submessage(SubMsg::reply_always(
                    Contract(strategy.scheduler_contract.clone()).call(
                        to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
                            condition: Condition::BlockHeight {
                                height: env.block.height + 128123
                            },
                            to: manager.clone(),
                            msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                                contract_address: env.contract.address.clone(),
                            })
                            .unwrap(),
                        }]))
                        .unwrap(),
                        vec![strategy.execution_rebate.clone()],
                    ),
                    SCHEDULE_REPLY_ID,
                ))
                .add_event(Event::from(DomainEvent::StrategyUpdated {
                    contract_address: env.contract.address.clone(),
                    old_config: StrategyConfig::Dca(default_strategy_config().clone()),
                    new_config: StrategyConfig::Dca(strategy.clone()),
                }))
        );
    }
}

#[cfg(test)]
mod can_execute_tests {
    use std::vec;

    use super::*;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        ContractResult, SystemResult, WasmQuery,
    };

    #[test]
    fn cannot_execute_with_insufficient_balance() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let strategy = default_strategy_config();

        deps.querier
            .bank
            .update_balance(env.contract.address.clone(), vec![]);

        assert_eq!(
            strategy
                .can_execute(deps.as_ref(), &env)
                .unwrap_err()
                .to_string(),
            format!(
                "Generic error: Insufficient swap amount of {} ({}) to cover gas fees",
                strategy.swap_amount.denom, 0
            )
        );
    }

    #[test]
    fn cannot_execute_paused_strategy() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();
        let strategy_owner = strategy.owner.clone();

        deps.querier
            .bank
            .update_balance(strategy_address.clone(), vec![strategy.swap_amount.clone()]);

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&Strategy {
                    status: StrategyStatus::Paused,
                    owner: strategy_owner.clone(),
                    contract_address: strategy_address.clone(),
                    created_at: env.block.time.seconds(),
                    updated_at: env.block.time.seconds(),
                    executions: 0,
                    label: "Test Strategy".to_string(),
                    affiliates: vec![],
                })
                .unwrap(),
            ))
        });

        assert_eq!(
            strategy
                .can_execute(deps.as_ref(), &env)
                .unwrap_err()
                .to_string(),
            "Generic error: Strategy is not active, current status: Paused",
        );
    }

    #[test]
    fn cannot_execute_archived_strategy() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();
        let strategy_owner = strategy.owner.clone();

        deps.querier
            .bank
            .update_balance(strategy_address.clone(), vec![strategy.swap_amount.clone()]);

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&Strategy {
                    status: StrategyStatus::Archived,
                    owner: strategy_owner.clone(),
                    contract_address: strategy_address.clone(),
                    created_at: env.block.time.seconds(),
                    updated_at: env.block.time.seconds(),
                    executions: 0,
                    label: "Test Strategy".to_string(),
                    affiliates: vec![],
                })
                .unwrap(),
            ))
        });

        assert_eq!(
            strategy
                .can_execute(deps.as_ref(), &env)
                .unwrap_err()
                .to_string(),
            "Generic error: Strategy is not active, current status: Archived",
        );
    }

    #[test]
    fn cannot_execute_with_unmet_trigger_condition() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();
        let scheduler_address = strategy.scheduler_contract.clone();

        deps.querier
            .bank
            .update_balance(strategy_address.clone(), vec![strategy.swap_amount.clone()]);

        deps.querier.update_wasm(move |query| match query {
            WasmQuery::Smart { contract_addr, .. } => {
                let default_strategy = default_strategy_config();
                if contract_addr == &manager_address.to_string() {
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary(&Strategy {
                            status: StrategyStatus::Active,
                            owner: default_strategy.owner.clone(),
                            contract_address: strategy_address.clone(),
                            created_at: env.block.time.seconds(),
                            updated_at: env.block.time.seconds(),
                            executions: 0,
                            label: "Test Strategy".to_string(),
                            affiliates: vec![],
                        })
                        .unwrap(),
                    ))
                } else if contract_addr == &scheduler_address.to_string() {
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary::<Vec<Trigger>>(&vec![Trigger {
                            condition: Condition::BlockHeight {
                                height: env.block.height + 1,
                            },
                            to: manager_address.clone(),
                            msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                                contract_address: strategy_address.clone(),
                            })
                            .unwrap(),
                            id: 1,
                            owner: strategy_address.clone(),
                            execution_rebate: vec![default_strategy.execution_rebate.clone()],
                        }])
                        .unwrap(),
                    ))
                } else {
                    panic!("Unexpected contract address: {}", contract_addr);
                }
            }
            _ => panic!("Unexpected query: {:?}", query),
        });

        assert_eq!(
            strategy
                .can_execute(deps.as_ref(), &env)
                .unwrap_err()
                .to_string(),
            format!(
                "Generic error: Condition for execution not met: {:?}",
                Condition::BlockHeight {
                    height: env.block.height + 1
                }
            )
        );
    }

    #[test]
    fn can_execute_with_sufficient_balance() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();
        let scheduler_address = strategy.scheduler_contract.clone();

        deps.querier
            .bank
            .update_balance(strategy_address.clone(), vec![strategy.swap_amount.clone()]);

        deps.querier.update_wasm(move |query| match query {
            WasmQuery::Smart { contract_addr, .. } => {
                if contract_addr == &manager_address.to_string() {
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary(&Strategy {
                            status: StrategyStatus::Active,
                            owner: default_strategy_config().owner.clone(),
                            contract_address: strategy_address.clone(),
                            created_at: env.block.time.seconds(),
                            updated_at: env.block.time.seconds(),
                            executions: 0,
                            label: "Test Strategy".to_string(),
                            affiliates: vec![],
                        })
                        .unwrap(),
                    ))
                } else if contract_addr == &scheduler_address.to_string() {
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary::<Vec<Trigger>>(&vec![]).unwrap(),
                    ))
                } else {
                    panic!("Unexpected contract address: {}", contract_addr);
                }
            }
            _ => panic!("Unexpected query: {:?}", query),
        });

        assert!(strategy.can_execute(deps.as_ref(), &env).is_ok());
    }

    #[test]
    fn can_execute_with_satisfied_trigger_condition() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();
        let scheduler_address = strategy.scheduler_contract.clone();

        deps.querier
            .bank
            .update_balance(strategy_address.clone(), vec![strategy.swap_amount.clone()]);

        deps.querier.update_wasm(move |query| match query {
            WasmQuery::Smart { contract_addr, msg } => {
                let default_strategy = default_strategy_config();
                if contract_addr == &manager_address.to_string() {
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary(&Strategy {
                            status: StrategyStatus::Active,
                            owner: default_strategy.owner.clone(),
                            contract_address: strategy_address.clone(),
                            created_at: env.block.time.seconds(),
                            updated_at: env.block.time.seconds(),
                            executions: 0,
                            label: "Test Strategy".to_string(),
                            affiliates: vec![],
                        })
                        .unwrap(),
                    ))
                } else if contract_addr == &scheduler_address.to_string() {
                    SystemResult::Ok(ContractResult::Ok(match from_json(msg).unwrap() {
                        SchedulerQueryMsg::Triggers { .. } => {
                            { to_json_binary::<Vec<Trigger>>(&vec![]) }.unwrap()
                        }
                        SchedulerQueryMsg::CanExecute { .. } => to_json_binary(&true).unwrap(),
                    }))
                } else {
                    panic!("Unexpected contract address: {}", contract_addr);
                }
            }
            _ => panic!("Unexpected query: {:?}", query),
        });

        assert!(strategy.can_execute(deps.as_ref(), &env).is_ok());
    }
}

#[cfg(test)]
mod execute_tests {
    use super::*;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        ContractResult, Event, SystemResult, WasmQuery,
    };

    #[test]
    fn publishes_execution_skipped_event_if_cannot_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        let response = strategy.execute(deps.as_ref(), &env).unwrap();

        assert_eq!(
            response.events[0],
            Event::from(DomainEvent::ExecutionSkipped {
                contract_address: env.contract.address,
                reason: "Generic error: Insufficient swap amount of rune (0) to cover gas fees"
                    .to_string()
            })
        );
    }

    #[test]
    fn adds_swap_msg_if_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();
        let scheduler_address = strategy.scheduler_contract.clone();

        deps.querier
            .bank
            .update_balance(strategy_address.clone(), vec![strategy.swap_amount.clone()]);

        deps.querier.update_wasm(move |query| match query {
            WasmQuery::Smart { contract_addr, .. } => {
                if contract_addr == &manager_address.to_string() {
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary(&Strategy {
                            status: StrategyStatus::Active,
                            owner: default_strategy_config().owner.clone(),
                            contract_address: strategy_address.clone(),
                            created_at: env.block.time.seconds(),
                            updated_at: env.block.time.seconds(),
                            executions: 0,
                            label: "Test Strategy".to_string(),
                            affiliates: vec![],
                        })
                        .unwrap(),
                    ))
                } else if contract_addr == &scheduler_address.to_string() {
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary::<Vec<Trigger>>(&vec![]).unwrap(),
                    ))
                } else {
                    panic!("Unexpected contract address: {}", contract_addr);
                }
            }
            _ => panic!("Unexpected query: {:?}", query),
        });

        let response = strategy.execute(deps.as_ref(), &mock_env()).unwrap();

        assert_eq!(
            response.messages[0],
            SubMsg::reply_always(
                Contract(strategy.exchange_contract.clone()).call(
                    to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: strategy.minimum_receive_amount.clone(),
                        recipient: None,
                    })
                    .unwrap(),
                    vec![strategy.swap_amount.clone()],
                ),
                EXECUTE_REPLY_ID
            )
            .with_payload(to_json_binary(&strategy.swap_amount).unwrap()),
        );
    }
}

#[cfg(test)]
mod handle_reply_tests {
    use super::*;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Binary, ContractResult, Event, SubMsgResponse, SystemResult, WasmQuery,
    };

    #[test]
    fn distributes_receive_amount_to_destinations_after_swap_succeeds() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();

        deps.querier.bank.update_balance(
            strategy_address.clone(),
            vec![strategy.minimum_receive_amount.clone()],
        );

        let destinations = strategy
            .mutable_destinations
            .clone()
            .into_iter()
            .chain(strategy.immutable_destinations.clone().into_iter());

        let total_shares = destinations
            .clone()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        #[allow(deprecated)]
        let response = strategy
            .handle_reply(
                deps.as_ref(),
                &env,
                Reply {
                    id: EXECUTE_REPLY_ID,
                    payload: to_json_binary(&strategy.swap_amount.clone()).unwrap(),
                    gas_used: 0,
                    result: SubMsgResult::Ok(SubMsgResponse {
                        events: vec![],
                        data: None,
                        msg_responses: vec![],
                    }),
                },
            )
            .unwrap();

        for destination in destinations {
            let share = Decimal::from_ratio(destination.shares, total_shares);

            assert!(response
                .messages
                .contains(&SubMsg::reply_never(BankMsg::Send {
                    to_address: destination.address.to_string(),
                    amount: vec![Coin {
                        amount: strategy.minimum_receive_amount.amount.mul_floor(share),
                        denom: strategy.minimum_receive_amount.denom.clone(),
                    }],
                })));
        }
    }

    #[test]
    fn publishes_execution_succeeded_event_after_swap_succeeds() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();

        deps.querier.bank.update_balance(
            strategy_address.clone(),
            vec![strategy.minimum_receive_amount.clone()],
        );

        #[allow(deprecated)]
        let response = strategy
            .handle_reply(
                deps.as_ref(),
                &env,
                Reply {
                    id: EXECUTE_REPLY_ID,
                    payload: to_json_binary(&strategy.swap_amount.clone()).unwrap(),
                    gas_used: 0,
                    result: SubMsgResult::Ok(SubMsgResponse {
                        events: vec![],
                        data: None,
                        msg_responses: vec![],
                    }),
                },
            )
            .unwrap();

        assert_eq!(
            response.events[0],
            Event::from(DomainEvent::ExecutionSucceeded {
                contract_address: strategy_address,
                statistics: strategy.statistics()
            })
        );
    }

    #[test]
    fn updates_statistics_after_swap_succeeds() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();

        deps.querier.bank.update_balance(
            strategy_address.clone(),
            vec![strategy.minimum_receive_amount.clone()],
        );

        assert_eq!(
            strategy.statistics,
            DcaStatistics {
                amount_deposited: Coin {
                    denom: strategy.swap_amount.denom.clone(),
                    amount: Uint128::zero()
                },
                amount_swapped: Coin {
                    denom: strategy.swap_amount.denom.clone(),
                    amount: Uint128::zero()
                },
                amount_received: Coin {
                    denom: strategy.minimum_receive_amount.denom.clone(),
                    amount: Uint128::zero()
                }
            }
        );

        #[allow(deprecated)]
        strategy
            .handle_reply(
                deps.as_ref(),
                &env,
                Reply {
                    id: EXECUTE_REPLY_ID,
                    payload: to_json_binary(&strategy.swap_amount.clone()).unwrap(),
                    gas_used: 0,
                    result: SubMsgResult::Ok(SubMsgResponse {
                        events: vec![],
                        data: None,
                        msg_responses: vec![],
                    }),
                },
            )
            .unwrap();

        assert_eq!(
            strategy.statistics,
            DcaStatistics {
                amount_deposited: Coin {
                    denom: strategy.swap_amount.denom.clone(),
                    amount: Uint128::zero()
                },
                amount_swapped: Coin {
                    denom: strategy.swap_amount.denom.clone(),
                    amount: strategy.swap_amount.amount
                },
                amount_received: Coin {
                    denom: strategy.minimum_receive_amount.denom.clone(),
                    amount: strategy.minimum_receive_amount.amount
                }
            }
        );
    }

    #[test]
    fn publishes_execution_failed_event_if_swap_fails() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();

        #[allow(deprecated)]
        let response = strategy
            .handle_reply(
                deps.as_ref(),
                &env,
                Reply {
                    id: EXECUTE_REPLY_ID,
                    payload: to_json_binary(&strategy.swap_amount.clone()).unwrap(),
                    gas_used: 0,
                    result: SubMsgResult::Err("Swap failed".to_string()),
                },
            )
            .unwrap();

        assert_eq!(
            response.events[0],
            Event::from(DomainEvent::ExecutionFailed {
                contract_address: strategy_address,
                reason: "Swap failed".to_string()
            })
        );
    }

    #[test]
    fn adds_schedule_msg_if_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = DcaStrategyConfig {
            schedule: DcaSchedule::Blocks {
                interval: 100,
                previous: None,
            },
            ..default_strategy_config()
        };

        let strategy_address = env.contract.address.clone();
        let scheduler_address = strategy.scheduler_contract.clone();

        deps.querier
            .bank
            .update_balance(strategy_address.clone(), vec![strategy.swap_amount.clone()]);

        deps.querier.update_wasm(move |query| match query {
            WasmQuery::Smart { contract_addr, .. } => {
                if contract_addr == &manager_address.to_string() {
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary(&Strategy {
                            status: StrategyStatus::Active,
                            owner: default_strategy_config().owner.clone(),
                            contract_address: strategy_address.clone(),
                            created_at: env.block.time.seconds(),
                            updated_at: env.block.time.seconds(),
                            executions: 0,
                            label: "Test Strategy".to_string(),
                            affiliates: vec![],
                        })
                        .unwrap(),
                    ))
                } else if contract_addr == &scheduler_address.clone().to_string() {
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary::<Vec<Trigger>>(&vec![]).unwrap(),
                    ))
                } else {
                    panic!("Unexpected contract address: {}", contract_addr);
                }
            }
            _ => panic!("Unexpected query: {:?}", query),
        });

        #[allow(deprecated)]
        let response = strategy
            .handle_reply(
                deps.as_ref(),
                &env,
                Reply {
                    id: EXECUTE_REPLY_ID,
                    payload: to_json_binary(&strategy.swap_amount.clone()).unwrap(),
                    gas_used: 0,
                    result: SubMsgResult::Ok(SubMsgResponse {
                        events: vec![],
                        data: None,
                        msg_responses: vec![],
                    }),
                },
            )
            .unwrap();

        assert!(response.messages.contains(&SubMsg::reply_always(
            Contract(strategy.scheduler_contract.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
                    condition: Condition::BlockHeight {
                        height: env.block.height + 100
                    },
                    to: MANAGER.load(&deps.storage).unwrap().clone(),
                    msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                        contract_address: env.contract.address.clone(),
                    })
                    .unwrap(),
                }]))
                .unwrap(),
                vec![strategy.execution_rebate.clone()],
            ),
            SCHEDULE_REPLY_ID,
        )));
    }

    #[test]
    fn adds_pause_msg_if_cannot_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        #[allow(deprecated)]
        let response = strategy
            .handle_reply(
                deps.as_ref(),
                &env,
                Reply {
                    id: EXECUTE_REPLY_ID,
                    payload: to_json_binary(&strategy.swap_amount.clone()).unwrap(),
                    gas_used: 0,
                    result: SubMsgResult::Ok(SubMsgResponse {
                        events: vec![],
                        data: None,
                        msg_responses: vec![],
                    }),
                },
            )
            .unwrap();

        assert_eq!(
            response.messages[2],
            SubMsg::reply_never(
                Contract(manager_address.clone()).call(
                    to_json_binary(&ManagerExecuteMsg::UpdateStatus {
                        status: StrategyStatus::Paused
                    })
                    .unwrap(),
                    vec![],
                ),
            )
        );
    }

    #[test]
    fn publishes_strategy_paused_event_if_cannot_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        #[allow(deprecated)]
        let response = strategy
            .handle_reply(
                deps.as_ref(),
                &env,
                Reply {
                    id: EXECUTE_REPLY_ID,
                    payload: to_json_binary(&strategy.swap_amount.clone()).unwrap(),
                    gas_used: 0,
                    result: SubMsgResult::Ok(SubMsgResponse {
                        events: vec![],
                        data: None,
                        msg_responses: vec![],
                    }),
                },
            )
            .unwrap();

        assert_eq!(
            response.events[2],
            Event::from(DomainEvent::StrategyPaused {
                contract_address: env.contract.address.clone(),
                reason: "Generic error: Insufficient swap amount of rune (0) to cover gas fees"
                    .to_string(),
            })
        );
    }

    #[test]
    fn publishes_scheduling_skipped_event_if_cannot_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        #[allow(deprecated)]
        let response = strategy
            .handle_reply(
                deps.as_ref(),
                &env,
                Reply {
                    id: EXECUTE_REPLY_ID,
                    payload: to_json_binary(&strategy.swap_amount.clone()).unwrap(),
                    gas_used: 0,
                    result: SubMsgResult::Ok(SubMsgResponse {
                        events: vec![],
                        data: None,
                        msg_responses: vec![],
                    }),
                },
            )
            .unwrap();

        assert_eq!(
            response.events[1],
            Event::from(DomainEvent::SchedulingSkipped {
                contract_address: env.contract.address.clone(),
                reason: "Generic error: Insufficient swap amount of rune (0) to cover gas fees"
                    .to_string(),
            })
        );
    }

    #[test]
    fn publishes_scheduling_succeeded_event_if_scheduling_succeeded() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();
        let scheduler_address = strategy.scheduler_contract.clone();

        deps.querier
            .bank
            .update_balance(strategy_address.clone(), vec![strategy.swap_amount.clone()]);

        deps.querier.update_wasm(move |query| match query {
            WasmQuery::Smart { contract_addr, .. } => {
                if contract_addr == &scheduler_address.to_string() {
                    SystemResult::Ok(ContractResult::Ok(
                        to_json_binary::<Vec<Trigger>>(&vec![Trigger {
                            id: 1,
                            owner: strategy_address.clone(),
                            condition: Condition::BlockHeight {
                                height: env.block.height + 128123,
                            },
                            msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                                contract_address: strategy_address.clone(),
                            })
                            .unwrap(),
                            to: manager_address.clone(),
                            execution_rebate: vec![],
                        }])
                        .unwrap(),
                    ))
                } else {
                    panic!("Unexpected contract address: {}", contract_addr);
                }
            }
            _ => panic!("Unexpected query: {:?}", query),
        });

        #[allow(deprecated)]
        let response = strategy
            .handle_reply(
                deps.as_ref(),
                &env,
                Reply {
                    id: SCHEDULE_REPLY_ID,
                    payload: Binary::default(),
                    gas_used: 0,
                    result: SubMsgResult::Ok(SubMsgResponse {
                        events: vec![],
                        data: None,
                        msg_responses: vec![],
                    }),
                },
            )
            .unwrap();

        assert_eq!(
            response.events[0],
            Event::from(DomainEvent::SchedulingSucceeded {
                contract_address: env.contract.address.clone(),
                conditions: vec![Condition::BlockHeight {
                    height: env.block.height + 128123
                }],
            })
        );
    }

    #[test]
    fn publishes_scheduling_failed_event_if_scheduling_failed() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();

        deps.querier
            .bank
            .update_balance(strategy_address.clone(), vec![strategy.swap_amount.clone()]);

        #[allow(deprecated)]
        let response = strategy
            .handle_reply(
                deps.as_ref(),
                &env,
                Reply {
                    id: SCHEDULE_REPLY_ID,
                    payload: Binary::default(),
                    gas_used: 0,
                    result: SubMsgResult::Err("Scheduling failed".to_string()),
                },
            )
            .unwrap();

        assert_eq!(
            response.events[0],
            Event::from(DomainEvent::SchedulingFailed {
                contract_address: env.contract.address.clone(),
                reason: "Scheduling failed".to_string(),
            })
        );
    }

    #[test]
    fn publishes_scheduling_paused_event_if_scheduling_failed() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        let strategy_address = env.contract.address.clone();

        deps.querier
            .bank
            .update_balance(strategy_address.clone(), vec![strategy.swap_amount.clone()]);

        #[allow(deprecated)]
        let response = strategy
            .handle_reply(
                deps.as_ref(),
                &env,
                Reply {
                    id: SCHEDULE_REPLY_ID,
                    payload: Binary::default(),
                    gas_used: 0,
                    result: SubMsgResult::Err("Scheduling failed".to_string()),
                },
            )
            .unwrap();

        assert_eq!(
            response.events[1],
            Event::from(DomainEvent::StrategyPaused {
                contract_address: env.contract.address.clone(),
                reason: "Scheduling failed".to_string()
            })
        );
    }

    #[test]
    fn adds_pause_strategy_msg_if_scheduling_fails() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let manager_address = deps.api.addr_make("manager");
        MANAGER
            .save(deps.as_mut().storage, &manager_address)
            .unwrap();

        let mut strategy = default_strategy_config();

        #[allow(deprecated)]
        let response = strategy
            .handle_reply(
                deps.as_ref(),
                &env,
                Reply {
                    id: SCHEDULE_REPLY_ID,
                    payload: Binary::default(),
                    gas_used: 0,
                    result: SubMsgResult::Err("Scheduling failed".to_string()),
                },
            )
            .unwrap();

        assert_eq!(
            response.messages[0],
            SubMsg::reply_never(
                Contract(manager_address.clone()).call(
                    to_json_binary(&ManagerExecuteMsg::UpdateStatus {
                        status: StrategyStatus::Paused
                    })
                    .unwrap(),
                    vec![],
                ),
            )
        );
    }
}
