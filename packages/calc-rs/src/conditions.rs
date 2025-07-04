use std::{time::Duration, u8, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Deps, Env, StdError, StdResult, Timestamp};
use rujira_rs::fin::{OrderResponse, Price, QueryMsg, Side};

use crate::{
    exchanger::{ExchangeQueryMsg, ExpectedReceiveAmount, Route},
    manager::{ManagerQueryMsg, Strategy, StrategyStatus},
};

#[cw_serde]
pub enum LogicalOperator {
    And,
    Or,
}

#[cw_serde]
pub enum Schedule {
    Blocks {
        interval: u64,
        previous: Option<u64>,
    },
    Time {
        duration: Duration,
        previous: Option<Timestamp>,
    },
}

impl Schedule {
    pub fn is_due(&self, env: &Env) -> bool {
        match self {
            Schedule::Blocks { interval, previous } => {
                previous.is_none_or(|previous| env.block.height > previous + interval)
            }
            Schedule::Time { duration, previous } => previous.is_none_or(|previous| {
                env.block.time.seconds() > previous.seconds() + duration.as_secs()
            }),
        }
    }

    pub fn into_condition(&self, env: &Env) -> Condition {
        match self {
            Schedule::Blocks { interval, previous } => Condition::BlocksCompleted(
                previous.map_or(env.block.height, |previous| previous + interval),
            ),
            Schedule::Time { duration, previous } => {
                Condition::TimeElapsed(previous.map_or(env.block.time, |previous| {
                    previous.plus_seconds(duration.as_secs())
                }))
            }
        }
    }

    pub fn next(&self, env: &Env) -> Self {
        match self {
            Schedule::Blocks { interval, previous } => Schedule::Blocks {
                interval: *interval,
                previous: Some(previous.map_or(env.block.height, |previous| {
                    let next = previous + *interval;
                    if next < env.block.height {
                        let blocks_completed = env.block.height - previous;
                        env.block.height + blocks_completed % interval
                    } else {
                        next
                    }
                })),
            },
            Schedule::Time { duration, previous } => Schedule::Time {
                duration: *duration,
                previous: Some(previous.map_or(env.block.time, |previous| {
                    let duration = duration.as_secs();
                    let next = previous.plus_seconds(duration);
                    if next < env.block.time {
                        let time_elapsed = env.block.time.seconds() - previous.seconds();
                        env.block.time.plus_seconds(time_elapsed % duration)
                    } else {
                        next
                    }
                })),
            },
        }
    }
}

#[cw_serde]
pub enum Condition {
    TimeElapsed(Timestamp),
    BlocksCompleted(u64),
    ScheduleIsDue(Schedule),
    CanSwap {
        exchanger_contract: Addr,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
        maximum_slippage_bps: u128,
        route: Option<Route>,
    },
    LimitOrderFilled {
        pair_address: Addr,
        owner: Addr,
        side: Side,
        price: Price,
    },
    BalanceAvailable {
        address: Addr,
        amount: Coin,
    },
    StrategyStatus {
        manager_contract: Addr,
        contract_address: Addr,
        status: StrategyStatus,
    },
    Compound {
        conditions: Vec<Condition>,
        operator: LogicalOperator,
    },
}

impl Condition {
    pub fn check(&self, deps: Deps, env: &Env) -> StdResult<()> {
        match self {
            Condition::TimeElapsed(timestamp) => {
                if env.block.time >= *timestamp {
                    return Ok(());
                }

                Err(StdError::generic_err(format!(
                    "Timestamp not elapsed: current timestamp ({}) is before required timestamp ({})",
                    env.block.time, timestamp
                )))
            }
            Condition::BlocksCompleted(height) => {
                if env.block.height >= *height {
                    return Ok(());
                }

                Err(StdError::generic_err(format!(
                    "Blocks not completed: current height ({}) is before required height ({})",
                    env.block.height, height
                )))
            }
            Condition::ScheduleIsDue(schedule) => {
                if schedule.is_due(env) {
                    Ok(())
                } else {
                    Err(StdError::generic_err(
                        schedule.into_condition(env).description(env),
                    ))
                }
            }
            Condition::LimitOrderFilled {
                pair_address,
                owner,
                side,
                price,
            } => {
                let order = deps
                    .querier
                    .query_wasm_smart::<OrderResponse>(
                        pair_address,
                        &QueryMsg::Order((owner.to_string(), side.clone(), price.clone())),
                    )
                    .map_err(|e| {
                        StdError::generic_err(format!(
                            "Failed to query order ({owner:?} {side:?} {price:?}): {e}"
                        ))
                    })?;

                if order.remaining.is_zero() {
                    return Ok(());
                }

                Err(StdError::generic_err(format!(
                    "Limit order not filled ({} remaining)",
                    order.remaining
                )))
            }
            Condition::CanSwap {
                exchanger_contract,
                swap_amount,
                minimum_receive_amount,
                maximum_slippage_bps,
                route,
            } => {
                let expected_receive_amount =
                    deps.querier.query_wasm_smart::<ExpectedReceiveAmount>(
                        exchanger_contract,
                        &ExchangeQueryMsg::ExpectedReceiveAmount {
                            swap_amount: swap_amount.clone(),
                            target_denom: minimum_receive_amount.denom.clone(),
                            route: route.clone(),
                        },
                    )?;

                if expected_receive_amount.receive_amount.amount < minimum_receive_amount.amount {
                    return Err(StdError::generic_err(format!(
                        "Expected receive amount {} is less than minimum receive amount {}",
                        expected_receive_amount.receive_amount.amount,
                        minimum_receive_amount.amount
                    )));
                }

                if expected_receive_amount.slippage_bps > *maximum_slippage_bps {
                    return Err(StdError::generic_err(format!(
                        "Slippage basis points {} exceeds maximum allowed slippage basis points {}",
                        expected_receive_amount.slippage_bps, maximum_slippage_bps
                    )));
                }

                Ok(())
            }
            Condition::BalanceAvailable { address, amount } => {
                let balance = deps.querier.query_balance(address, amount.denom.clone())?;

                if balance.amount >= amount.amount {
                    return Ok(());
                }

                Err(StdError::generic_err(format!(
                    "Balance available for {} ({}) is less than required ({})",
                    address, balance.amount, amount.amount
                )))
            }
            Condition::StrategyStatus {
                manager_contract,
                contract_address,
                status,
            } => {
                let strategy = deps.querier.query_wasm_smart::<Strategy>(
                    manager_contract,
                    &ManagerQueryMsg::Strategy {
                        address: contract_address.clone(),
                    },
                )?;

                if strategy.status == *status {
                    return Ok(());
                }

                Err(StdError::generic_err(format!(
                    "Strategy not in required status: expected {:?}, got {:?}",
                    status, strategy.status
                )))
            }
            Condition::Compound {
                conditions,
                operator,
            } => match operator {
                LogicalOperator::And => {
                    for condition in conditions {
                        condition.check(deps, env)?;
                    }
                    Ok(())
                }
                LogicalOperator::Or => {
                    for condition in conditions {
                        if condition.check(deps, env).is_ok() {
                            return Ok(());
                        }
                    }
                    Err(StdError::generic_err(format!(
                        "No compound conditions met in: {}",
                        conditions
                            .iter()
                            .map(|c| c.description(env))
                            .collect::<Vec<_>>()
                            .join(",\n")
                    )))
                }
            },
        }
    }

    pub fn description(&self, env: &Env) -> String {
        match self {
            Condition::TimeElapsed(timestamp) => format!("timestamp elapsed: {timestamp}"),
            Condition::BlocksCompleted(height) => format!("blocks completed: {height}"),
            Condition::ScheduleIsDue(schedule) => format!(
                "schedule is due: {}",
                schedule.into_condition(env).description(env)
            ),
            Condition::CanSwap {
                swap_amount,
                minimum_receive_amount,
                maximum_slippage_bps,
                ..
            } => format!(
                "exchange liquidity provided: swap_amount={swap_amount}, minimum_receive_amount={minimum_receive_amount}, maximum_slippage_bps={maximum_slippage_bps}"
            ),
            Condition::LimitOrderFilled {
                pair_address,
                owner,
                side,
                price,
            } => format!(
                "limit order filled: pair_address={pair_address}, owner={owner}, side={side:?}, price={price}"
            ),
            Condition::BalanceAvailable { address, amount } => format!(
                "balance available: address={address}, amount={amount}"
            ),
            Condition::StrategyStatus {
                contract_address,
                status,
                ..
            } => format!(
                "strategy ({contract_address}) is in status: {status:?}"
            ),
            Condition::Compound { conditions, operator } => {
                match operator {
                    LogicalOperator::And => format!(
                        "All the following conditions are met: [\n\t{}\n]",
                        conditions
                            .iter()
                            .map(|c| c.description(env))
                            .collect::<Vec<_>>()
                            .join(",\n\t")
                    ),
                    LogicalOperator::Or => format!(
                        "Any of the following conditions are met: [\n\t{}\n]",
                        conditions
                            .iter()
                            .map(|c| c.description(env))
                            .collect::<Vec<_>>()
                            .join(",\n\t")
                    ),
                }
            }
        }
    }

    pub fn next(&self, env: &Env) -> Condition {
        match self {
            Condition::ScheduleIsDue(schedule) => Condition::ScheduleIsDue(schedule.next(env)),
            _ => self.clone(),
        }
    }
}

#[cfg(test)]
mod schedule_tests {
    use super::*;

    use std::time::Duration;

    use cosmwasm_std::testing::mock_env;

    #[test]
    fn updates_to_next_scheduled_block() {
        let env = mock_env();

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: None
            }
            .next(&env),
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height)
            }
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
            .next(&env),
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5 + 10)
            }
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 15)
            }
            .next(&env),
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height + 5)
            }
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 155)
            }
            .next(&env),
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height + 5)
            }
        );
    }

    #[test]
    fn updates_to_next_scheduled_time() {
        let env = mock_env();

        assert_eq!(
            Schedule::Time {
                duration: std::time::Duration::from_secs(10),
                previous: None
            }
            .next(&env),
            Schedule::Time {
                duration: std::time::Duration::from_secs(10),
                previous: Some(env.block.time)
            }
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .next(&env),
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.plus_seconds(5))
            }
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(15))
            }
            .next(&env),
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.plus_seconds(5))
            }
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(155))
            }
            .next(&env),
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.plus_seconds(5))
            }
        );
    }

    #[test]
    fn gets_next_block_condition() {
        let env = mock_env();

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: None
            }
            .into_condition(&env),
            Condition::BlocksCompleted(env.block.height)
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height)
            }
            .into_condition(&env),
            Condition::BlocksCompleted(env.block.height + 10)
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
            .into_condition(&env),
            Condition::BlocksCompleted(env.block.height - 5 + 10)
        );
    }

    #[test]
    fn gets_next_time_condition() {
        let env = mock_env();

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: None
            }
            .into_condition(&env),
            Condition::TimeElapsed(env.block.time)
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time)
            }
            .into_condition(&env),
            Condition::TimeElapsed(env.block.time.plus_seconds(10))
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .into_condition(&env),
            Condition::TimeElapsed(env.block.time.plus_seconds(10 - 5))
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(155))
            }
            .into_condition(&env),
            Condition::TimeElapsed(env.block.time.minus_seconds(155 - 10))
        );
    }

    #[test]
    fn block_schedule_is_due() {
        let env = mock_env();

        assert!(
            Schedule::Blocks {
                interval: 10,
                previous: None
            }
            .is_due(&env)
        );

        assert!(
            !Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
            .is_due(&env)
        );

        assert!(
            Schedule::Blocks {
                interval: 5,
                previous: Some(env.block.height - 6)
            }
            .is_due(&env)
        );

        assert!(
            !Schedule::Blocks {
                interval: 5,
                previous: Some(env.block.height - 5)
            }
            .is_due(&env)
        );
    }

    #[test]
    fn time_schedule_is_due() {
        let env = mock_env();

        assert!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: None
            }
            .is_due(&env)
        );

        assert!(
            !Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .is_due(&env)
        );

        assert!(
            Schedule::Time {
                duration: Duration::from_secs(5),
                previous: Some(env.block.time.minus_seconds(6))
            }
            .is_due(&env)
        );

        assert!(
            !Schedule::Time {
                duration: Duration::from_secs(5),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .is_due(&env)
        );
    }
}

#[cfg(test)]
mod conditions_tests {
    use super::*;
    use std::str::FromStr;

    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        to_json_binary, Addr, Coin, ContractResult, Decimal, StdError, SystemResult, Timestamp,
        Uint128,
    };
    use rujira_rs::fin::{OrderResponse, Price, Side};

    use crate::{exchanger::ExpectedReceiveAmount, manager::Strategy, manager::StrategyStatus};

    #[test]
    fn timestamp_elapsed_check() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::TimeElapsed(Timestamp::from_seconds(0))
            .check(deps.as_ref(), &env)
            .is_ok());

        assert!(Condition::TimeElapsed(env.block.time)
            .check(deps.as_ref(), &env)
            .is_ok());

        assert!(Condition::TimeElapsed(env.block.time.plus_seconds(1))
            .check(deps.as_ref(), &env)
            .is_err());
    }

    #[test]
    fn blocks_completed_check() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::BlocksCompleted(0)
            .check(deps.as_ref(), &env)
            .is_ok());

        assert!(Condition::BlocksCompleted(env.block.height)
            .check(deps.as_ref(), &env)
            .is_ok());

        assert!(Condition::BlocksCompleted(env.block.height + 1)
            .check(deps.as_ref(), &env)
            .is_err());
    }

    #[test]
    fn balance_available_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(0u128, "rune"),
        }
        .check(deps.as_ref(), &env)
        .is_ok());

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(1u128, "rune"),
        }
        .check(deps.as_ref(), &env)
        .is_err());

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![Coin::new(100u128, "rune")],
        );

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(99u128, "rune"),
        }
        .check(deps.as_ref(), &env)
        .is_ok());

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(100u128, "rune"),
        }
        .check(deps.as_ref(), &env)
        .is_ok());

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(101u128, "rune"),
        }
        .check(deps.as_ref(), &env)
        .is_err());
    }

    #[test]
    fn exchange_liquidity_provided_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&ExpectedReceiveAmount {
                    receive_amount: Coin::new(100u128, "rune"),
                    slippage_bps: 10,
                })
                .unwrap(),
            ))
        });

        assert!(Condition::CanSwap {
            exchanger_contract: env.contract.address.clone(),
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(101u128, "rune"),
            maximum_slippage_bps: 10,
            route: None,
        }
        .check(deps.as_ref(), &env)
        .is_err());

        assert!(Condition::CanSwap {
            exchanger_contract: env.contract.address.clone(),
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(100u128, "rune"),
            maximum_slippage_bps: 9,
            route: None,
        }
        .check(deps.as_ref(), &env)
        .is_err());

        assert!(Condition::CanSwap {
            exchanger_contract: env.contract.address.clone(),
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(100u128, "rune"),
            maximum_slippage_bps: 10,
            route: None,
        }
        .check(deps.as_ref(), &env)
        .is_ok());
    }

    #[test]
    fn limit_order_filled_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&OrderResponse {
                    remaining: Uint128::new(100),
                    filled: Uint128::new(100),
                    owner: "owner".to_string(),
                    side: Side::Base,
                    price: Price::Fixed(Decimal::from_str("1.0").unwrap()),
                    rate: Decimal::from_str("1.0").unwrap(),
                    updated_at: Timestamp::from_seconds(env.block.time.seconds()),
                    offer: Uint128::new(21029),
                })
                .unwrap(),
            ))
        });

        assert_eq!(
            Condition::LimitOrderFilled {
                owner: Addr::unchecked("owner"),
                pair_address: Addr::unchecked("pair"),
                side: Side::Base,
                price: Price::Fixed(Decimal::from_str("1.0").unwrap()),
            }
            .check(deps.as_ref(), &env)
            .unwrap_err(),
            StdError::generic_err("Limit order not filled (100 remaining)",)
        );

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&OrderResponse {
                    remaining: Uint128::new(0),
                    filled: Uint128::new(100),
                    owner: "owner".to_string(),
                    side: Side::Base,
                    price: Price::Fixed(Decimal::from_str("1.0").unwrap()),
                    rate: Decimal::from_str("1.0").unwrap(),
                    updated_at: Timestamp::from_seconds(env.block.time.seconds()),
                    offer: Uint128::new(21029),
                })
                .unwrap(),
            ))
        });

        assert!(Condition::LimitOrderFilled {
            owner: Addr::unchecked("owner"),
            pair_address: Addr::unchecked("pair"),
            side: Side::Base,
            price: Price::Fixed(Decimal::from_str("1.0").unwrap()),
        }
        .check(deps.as_ref(), &env)
        .is_ok());
    }

    #[test]
    fn strategy_status_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&Strategy {
                    id: 1,
                    contract_address: Addr::unchecked("strategy"),
                    status: StrategyStatus::Active,
                    owner: Addr::unchecked("owner"),
                    created_at: 0,
                    updated_at: 0,
                    label: "label".to_string(),
                    affiliates: vec![],
                })
                .unwrap(),
            ))
        });

        let strategy_address = Addr::unchecked("strategy");

        assert!(Condition::StrategyStatus {
            manager_contract: Addr::unchecked("manager"),
            contract_address: strategy_address.clone(),
            status: StrategyStatus::Active,
        }
        .check(deps.as_ref(), &env)
        .is_ok());

        assert!(Condition::StrategyStatus {
            manager_contract: Addr::unchecked("manager"),
            contract_address: strategy_address.clone(),
            status: StrategyStatus::Paused,
        }
        .check(deps.as_ref(), &env)
        .is_err());
    }
}
