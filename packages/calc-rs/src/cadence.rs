use std::{cmp::max, str::FromStr, time::Duration};

use chrono::DateTime;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Deps, Env, StdError, StdResult, Timestamp};
use cron::Schedule as CronSchedule;
use rujira_rs::fin::Side;

use crate::{
    actions::limit_orders::fin_limit_order::PriceStrategy, conditions::condition::Condition,
};

#[cw_serde]
pub enum Cadence {
    Blocks {
        interval: u64,
        previous: Option<u64>,
    },
    Time {
        duration: Duration,
        previous: Option<Timestamp>,
    },
    Cron {
        expr: String,
        previous: Option<Timestamp>,
    },
    LimitOrder {
        pair_address: Addr,
        side: Side,
        previous: Option<Decimal>,
        strategy: PriceStrategy,
    },
}

impl Cadence {
    pub fn is_due(&self, deps: Deps, env: &Env, scheduler: &Addr) -> StdResult<bool> {
        Ok(match self {
            Cadence::Blocks { interval, previous } => {
                previous.map_or(true, |previous| env.block.height >= previous + interval)
            }
            Cadence::Time { duration, previous } => previous.map_or(true, |previous| {
                env.block.time.seconds() >= previous.seconds() + duration.as_secs()
            }),
            Cadence::Cron { previous, .. } => {
                if previous.is_none() {
                    true
                } else {
                    self.into_condition(deps, env, scheduler)?
                        .is_satisfied(deps, env)?
                }
            }
            Cadence::LimitOrder {
                pair_address,
                side,
                strategy,
                previous,
            } => {
                if let Some(previous) = previous {
                    match strategy {
                        PriceStrategy::Fixed(price) => Condition::FinLimitOrderFilled {
                            owner: Some(scheduler.clone()),
                            pair_address: pair_address.clone(),
                            side: side.clone(),
                            price: price.clone(),
                        }
                        .is_satisfied(deps, env)?,
                        PriceStrategy::Offset { .. } => {
                            let previous_order_filled = Condition::FinLimitOrderFilled {
                                owner: Some(scheduler.clone()),
                                pair_address: pair_address.clone(),
                                side: side.clone(),
                                price: previous.clone(),
                            }
                            .is_satisfied(deps, env)?;

                            if previous_order_filled {
                                true
                            } else {
                                let new_price = strategy.get_new_price(deps, pair_address, side)?;
                                strategy.should_reset(previous.clone(), new_price)
                            }
                        }
                    }
                } else {
                    return Ok(false);
                }
            }
        })
    }

    pub fn into_condition(&self, deps: Deps, env: &Env, scheduler: &Addr) -> StdResult<Condition> {
        Ok(match self {
            Cadence::Blocks { interval, previous } => Condition::BlocksCompleted(
                previous.map_or(env.block.height, |previous| previous + interval),
            ),
            Cadence::Time { duration, previous } => {
                Condition::TimestampElapsed(previous.map_or(env.block.time, |previous| {
                    previous.plus_seconds(duration.as_secs())
                }))
            }
            Cadence::Cron { expr, previous } => {
                let schedule = CronSchedule::from_str(expr)
                    .map_err(|e| StdError::generic_err(format!("Invalid cron expression: {e}")))?;

                let next = schedule
                    .after(&DateTime::from_timestamp_nanos(
                        previous.map_or(env.block.time, |previous| previous).nanos() as i64,
                    ))
                    .next();

                if let Some(next) = next {
                    Condition::TimestampElapsed(Timestamp::from_seconds(next.timestamp() as u64))
                } else {
                    // Cron expression has no next occurrence, treat as never due
                    Condition::BlocksCompleted(u64::MAX)
                }
            }
            Cadence::LimitOrder {
                pair_address,
                side,
                strategy,
                previous,
            } => {
                let price = if let Some(previous) = previous {
                    previous.clone()
                } else {
                    strategy.get_new_price(deps, pair_address, side)?
                };

                Condition::FinLimitOrderFilled {
                    owner: Some(scheduler.clone()),
                    pair_address: pair_address.clone(),
                    side: side.clone(),
                    price,
                }
            }
        })
    }

    pub fn crank(self, deps: Deps, env: &Env) -> StdResult<Self> {
        Ok(match self {
            Cadence::Blocks { interval, previous } => Cadence::Blocks {
                interval,
                previous: Some(previous.map_or(env.block.height, |previous| {
                    let next = previous + interval;
                    if next < env.block.height - interval {
                        let blocks_completed = env.block.height - previous;
                        env.block.height - blocks_completed % interval
                    } else {
                        next
                    }
                })),
            },
            Cadence::Time { duration, previous } => Cadence::Time {
                duration,
                previous: Some(previous.map_or(env.block.time, |previous| {
                    let duration = duration.as_secs();
                    let next = previous.plus_seconds(duration);
                    if next < env.block.time.minus_seconds(duration) {
                        let time_elapsed = env.block.time.seconds() - previous.seconds();
                        env.block.time.minus_seconds(time_elapsed % duration)
                    } else {
                        next
                    }
                })),
            },
            Cadence::Cron { expr, previous } => {
                let schedule = CronSchedule::from_str(&expr).map_err(|e| {
                    cosmwasm_std::StdError::generic_err(format!("Invalid cron expression: {e}"))
                })?;

                let next = schedule
                    .after(&DateTime::from_timestamp_nanos(
                        previous
                            .map_or(env.block.time, |previous| max(previous, env.block.time))
                            .nanos() as i64,
                    ))
                    .next();

                if let Some(next) = next {
                    Cadence::Cron {
                        expr,
                        previous: Some(Timestamp::from_seconds(next.timestamp() as u64)),
                    }
                } else {
                    // Cron expression has no next occurrence, treat as never due
                    Cadence::Cron {
                        expr,
                        previous: Some(Timestamp::from_seconds(u64::MAX)),
                    }
                }
            }
            Cadence::LimitOrder {
                pair_address,
                side,
                strategy,
                previous,
            } => {
                if let Some(previous) = previous {
                    let new_price = strategy.get_new_price(deps, &pair_address, &side)?;
                    if strategy.should_reset(previous, new_price) {
                        Cadence::LimitOrder {
                            pair_address: pair_address.clone(),
                            side: side.clone(),
                            previous: Some(new_price),
                            strategy,
                        }
                    } else {
                        Cadence::LimitOrder {
                            pair_address: pair_address.clone(),
                            side: side.clone(),
                            previous: Some(previous),
                            strategy,
                        }
                    }
                } else {
                    Cadence::LimitOrder {
                        pair_address: pair_address.clone(),
                        side: side.clone(),
                        previous: Some(strategy.get_new_price(deps, &pair_address, &side)?),
                        strategy,
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        actions::limit_orders::fin_limit_order::{Direction, Offset},
        cadence::Cadence,
    };

    use super::*;

    use cosmwasm_std::{
        from_json,
        testing::{mock_dependencies, mock_env},
        to_json_binary, ContractResult, SystemResult, Uint128, WasmQuery,
    };
    use rujira_rs::fin::{BookItemResponse, BookResponse, OrderResponse, Price, QueryMsg};
    use std::time::Duration;

    #[test]
    fn updates_to_next_previous_block() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: None
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height)
            }
        );

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height + 5)
            }
        );

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 15)
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
        );

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 155)
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
        );

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 152)
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 2)
            }
        );

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 158)
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 8)
            }
        );
    }

    #[test]
    fn updates_to_next_previous_time() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert_eq!(
            Cadence::Time {
                duration: std::time::Duration::from_secs(10),
                previous: None
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Time {
                duration: std::time::Duration::from_secs(10),
                previous: Some(env.block.time)
            }
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.plus_seconds(5))
            }
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(15))
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(155))
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
        );
    }

    #[test]
    fn updates_to_next_previous_cron() {
        let deps = mock_dependencies();
        let env = mock_env();

        let cron = "*/10 * * * * *";

        assert_eq!(
            Cadence::Cron {
                expr: cron.to_string(),
                previous: None
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Cron {
                expr: cron.to_string(),
                previous: Some(Timestamp::from_seconds(
                    env.block.time.seconds() - env.block.time.seconds() % 10 + 10
                ))
            }
        );

        assert_eq!(
            Cadence::Cron {
                expr: cron.to_string(),
                previous: Some(Timestamp::from_seconds(0))
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Cron {
                expr: cron.to_string(),
                previous: Some(Timestamp::from_seconds(
                    env.block.time.seconds() - env.block.time.seconds() % 10 + 10
                ))
            }
        );

        assert_eq!(
            Cadence::Cron {
                expr: cron.to_string(),
                previous: Some(env.block.time)
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Cron {
                expr: cron.to_string(),
                previous: Some(Timestamp::from_seconds(
                    env.block.time.seconds() - env.block.time.seconds() % 10 + 10
                ))
            }
        );

        assert_eq!(
            Cadence::Cron {
                expr: cron.to_string(),
                previous: Some(env.block.time.plus_seconds(10))
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Cron {
                expr: cron.to_string(),
                previous: Some(Timestamp::from_seconds(
                    env.block.time.seconds() - env.block.time.seconds() % 10 + 20
                ))
            }
        );
    }

    #[test]
    fn updates_to_next_limit_order() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let pair_address = Addr::unchecked("pair");
        let side = Side::Base;
        let fixed_strategy = PriceStrategy::Fixed(Decimal::from_str("100.0").unwrap());

        assert_eq!(
            Cadence::LimitOrder {
                pair_address: pair_address.clone(),
                side: side.clone(),
                previous: None,
                strategy: fixed_strategy.clone()
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::LimitOrder {
                pair_address: pair_address.clone(),
                side: side.clone(),
                previous: Some(Decimal::from_str("100.0").unwrap()),
                strategy: fixed_strategy
            }
        );

        let offset_strategy = PriceStrategy::Offset {
            direction: Direction::Above,
            offset: Offset::Exact(Decimal::from_str("0.10").unwrap()),
            tolerance: Some(Offset::Percent(50)),
        };

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&BookResponse {
                    base: vec![BookItemResponse {
                        price: Decimal::from_str("1.45").unwrap(),
                        total: Uint128::new(1_000_000),
                    }],
                    quote: vec![BookItemResponse {
                        price: Decimal::from_str("1.35").unwrap(),
                        total: Uint128::new(1_000_000),
                    }],
                })
                .unwrap(),
            ))
        });

        assert_eq!(
            Cadence::LimitOrder {
                pair_address: pair_address.clone(),
                side: side.clone(),
                previous: None,
                strategy: offset_strategy.clone()
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::LimitOrder {
                pair_address: pair_address.clone(),
                side: side.clone(),
                previous: Some(Decimal::from_str("1.55").unwrap()),
                strategy: offset_strategy.clone()
            }
        );

        assert_eq!(
            Cadence::LimitOrder {
                pair_address: pair_address.clone(),
                side: side.clone(),
                previous: Some(Decimal::from_str("1.53").unwrap()),
                strategy: offset_strategy.clone()
            }
            .crank(deps.as_ref(), &env)
            .unwrap(),
            Cadence::LimitOrder {
                pair_address: pair_address.clone(),
                side: side.clone(),
                previous: Some(Decimal::from_str("1.53").unwrap()),
                strategy: offset_strategy
            }
        );
    }

    #[test]
    fn gets_next_block_condition() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: None
            }
            .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
            .unwrap(),
            Condition::BlocksCompleted(env.block.height)
        );

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height)
            }
            .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
            .unwrap(),
            Condition::BlocksCompleted(env.block.height + 10)
        );

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
            .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
            .unwrap(),
            Condition::BlocksCompleted(env.block.height - 5 + 10)
        );
    }

    #[test]
    fn gets_next_time_condition() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: None
            }
            .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
            .unwrap(),
            Condition::TimestampElapsed(env.block.time)
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time)
            }
            .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
            .unwrap(),
            Condition::TimestampElapsed(env.block.time.plus_seconds(10))
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
            .unwrap(),
            Condition::TimestampElapsed(env.block.time.plus_seconds(10 - 5))
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(155))
            }
            .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
            .unwrap(),
            Condition::TimestampElapsed(env.block.time.minus_seconds(155 - 10))
        );
    }

    #[test]
    fn gets_next_cron_condition() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert_eq!(
            Cadence::Cron {
                expr: "*/30 * * * * *".to_string(),
                previous: None,
            }
            .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
            .unwrap(),
            Condition::TimestampElapsed(Timestamp::from_seconds(
                env.block.time.seconds() - env.block.time.seconds() % 30 + 30,
            ))
        );

        let previous = env.block.time.plus_seconds(100);

        assert_eq!(
            Cadence::Cron {
                expr: "*/30 * * * * *".to_string(),
                previous: Some(previous),
            }
            .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
            .unwrap(),
            Condition::TimestampElapsed(Timestamp::from_seconds(
                previous.seconds() - previous.seconds() % 30 + 30,
            ))
        );

        assert!(Cadence::Cron {
            expr: "bad cron".to_string(),
            previous: None,
        }
        .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap_err()
        .to_string()
        .contains("Invalid cron expression"));
    }

    #[test]
    fn gets_next_limit_order_condition() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let pair_address = Addr::unchecked("pair");
        let side = Side::Base;
        let fixed_strategy = PriceStrategy::Fixed(Decimal::from_str("100.0").unwrap());

        assert_eq!(
            Cadence::LimitOrder {
                pair_address: pair_address.clone(),
                side: side.clone(),
                previous: None,
                strategy: fixed_strategy.clone()
            }
            .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
            .unwrap(),
            Condition::FinLimitOrderFilled {
                owner: Some(Addr::unchecked("scheduler")),
                pair_address: pair_address.clone(),
                side: side.clone(),
                price: Decimal::from_str("100.0").unwrap()
            }
        );

        let offset_strategy = PriceStrategy::Offset {
            direction: Direction::Above,
            offset: Offset::Exact(Decimal::from_str("0.10").unwrap()),
            tolerance: Some(Offset::Percent(50)),
        };

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&BookResponse {
                    base: vec![BookItemResponse {
                        price: Decimal::from_str("1.45").unwrap(),
                        total: Uint128::new(1_000_000),
                    }],
                    quote: vec![BookItemResponse {
                        price: Decimal::from_str("1.35").unwrap(),
                        total: Uint128::new(1_000_000),
                    }],
                })
                .unwrap(),
            ))
        });

        assert_eq!(
            Cadence::LimitOrder {
                pair_address: pair_address.clone(),
                side: side.clone(),
                previous: None,
                strategy: offset_strategy.clone()
            }
            .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
            .unwrap(),
            Condition::FinLimitOrderFilled {
                owner: Some(Addr::unchecked("scheduler")),
                pair_address: pair_address.clone(),
                side: side.clone(),
                price: Decimal::from_str("1.55").unwrap()
            }
        );
    }

    #[test]
    fn block_schedule_is_due() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert!(Cadence::Blocks {
            interval: 10,
            previous: None
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        assert!(!Cadence::Blocks {
            interval: 5,
            previous: Some(env.block.height - 4)
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        assert!(Cadence::Blocks {
            interval: 5,
            previous: Some(env.block.height - 5)
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        assert!(Cadence::Blocks {
            interval: 5,
            previous: Some(env.block.height - 6)
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());
    }

    #[test]
    fn time_schedule_is_due() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert!(Cadence::Time {
            duration: Duration::from_secs(10),
            previous: None
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        assert!(!Cadence::Time {
            duration: Duration::from_secs(6),
            previous: Some(env.block.time.minus_seconds(5))
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        assert!(Cadence::Time {
            duration: Duration::from_secs(5),
            previous: Some(env.block.time.minus_seconds(5))
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        assert!(Cadence::Time {
            duration: Duration::from_secs(4),
            previous: Some(env.block.time.minus_seconds(5))
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());
    }

    #[test]
    fn cron_schedule_is_due() {
        let deps = mock_dependencies();
        let env = mock_env();
        let cron = "*/10 * * * * *";

        assert!(Cadence::Cron {
            expr: cron.to_string(),
            previous: None
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        assert!(!Cadence::Cron {
            expr: cron.to_string(),
            previous: Some(env.block.time.minus_seconds(5))
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        assert!(Cadence::Cron {
            expr: cron.to_string(),
            previous: Some(env.block.time.minus_seconds(15))
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());
    }

    #[test]
    fn limit_order_schedule_is_due() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        assert!(!Cadence::LimitOrder {
            pair_address: Addr::unchecked("pair"),
            side: Side::Base,
            previous: None,
            strategy: PriceStrategy::Fixed(Decimal::from_str("100.0").unwrap())
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&OrderResponse {
                    owner: "scheduler".to_string(),
                    side: Side::Base,
                    price: Price::Fixed(Decimal::from_str("100.0").unwrap()),
                    rate: Decimal::from_str("0.1").unwrap(),
                    updated_at: Timestamp::from_seconds(12),
                    offer: Uint128::new(7123123),
                    remaining: Uint128::new(1),
                    filled: Uint128::new(23453),
                })
                .unwrap(),
            ))
        });

        assert!(!Cadence::LimitOrder {
            pair_address: Addr::unchecked("pair"),
            side: Side::Base,
            previous: Some(Decimal::from_str("100.0").unwrap()),
            strategy: PriceStrategy::Fixed(Decimal::from_str("100.0").unwrap())
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&OrderResponse {
                    owner: "scheduler".to_string(),
                    side: Side::Base,
                    price: Price::Fixed(Decimal::from_str("100.0").unwrap()),
                    rate: Decimal::from_str("0.1").unwrap(),
                    updated_at: Timestamp::from_seconds(12),
                    offer: Uint128::new(7123123),
                    remaining: Uint128::new(0),
                    filled: Uint128::new(23453),
                })
                .unwrap(),
            ))
        });

        assert!(Cadence::LimitOrder {
            pair_address: Addr::unchecked("pair"),
            side: Side::Base,
            previous: Some(Decimal::from_str("100.0").unwrap()),
            strategy: PriceStrategy::Fixed(Decimal::from_str("100.0").unwrap())
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        assert!(!Cadence::LimitOrder {
            pair_address: Addr::unchecked("pair"),
            side: Side::Base,
            previous: None,
            strategy: PriceStrategy::Offset {
                direction: Direction::Above,
                offset: Offset::Exact(Decimal::from_str("0.10").unwrap()),
                tolerance: Some(Offset::Percent(50)),
            }
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&OrderResponse {
                    owner: "scheduler".to_string(),
                    side: Side::Base,
                    price: Price::Fixed(Decimal::from_str("1.55").unwrap()),
                    rate: Decimal::from_str("0.1").unwrap(),
                    updated_at: Timestamp::from_seconds(12),
                    offer: Uint128::new(7123123),
                    remaining: Uint128::new(0),
                    filled: Uint128::new(23453),
                })
                .unwrap(),
            ))
        });

        assert!(Cadence::LimitOrder {
            pair_address: Addr::unchecked("pair"),
            side: Side::Base,
            previous: Some(Decimal::from_str("1.55").unwrap()),
            strategy: PriceStrategy::Offset {
                direction: Direction::Above,
                offset: Offset::Exact(Decimal::from_str("0.10").unwrap()),
                tolerance: Some(Offset::Percent(50)),
            }
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Order(_) => to_json_binary(&OrderResponse {
                        owner: "scheduler".to_string(),
                        side: Side::Base,
                        price: Price::Fixed(Decimal::from_str("1.55").unwrap()),
                        rate: Decimal::from_str("0.1").unwrap(),
                        updated_at: Timestamp::from_seconds(12),
                        offer: Uint128::new(7123123),
                        remaining: Uint128::new(12312),
                        filled: Uint128::new(23453),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.45").unwrap(),
                            total: Uint128::new(1_000_000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.35").unwrap(),
                            total: Uint128::new(1_000_000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!("unexpected query type"),
                },
                _ => panic!("unexpected query type"),
            }))
        });

        assert!(!Cadence::LimitOrder {
            pair_address: Addr::unchecked("pair"),
            side: Side::Base,
            previous: Some(Decimal::from_str("1.65").unwrap()),
            strategy: PriceStrategy::Offset {
                direction: Direction::Above,
                offset: Offset::Exact(Decimal::from_str("0.10").unwrap()),
                tolerance: Some(Offset::Percent(50)),
            }
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Order(_) => to_json_binary(&OrderResponse {
                        owner: "scheduler".to_string(),
                        side: Side::Base,
                        price: Price::Fixed(Decimal::from_str("1.55").unwrap()),
                        rate: Decimal::from_str("0.1").unwrap(),
                        updated_at: Timestamp::from_seconds(12),
                        offer: Uint128::new(7123123),
                        remaining: Uint128::new(0),
                        filled: Uint128::new(23453),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.45").unwrap(),
                            total: Uint128::new(1_000_000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.35").unwrap(),
                            total: Uint128::new(1_000_000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!("unexpected query type"),
                },
                _ => panic!("unexpected query type"),
            }))
        });

        assert!(Cadence::LimitOrder {
            pair_address: Addr::unchecked("pair"),
            side: Side::Base,
            previous: Some(Decimal::from_str("1.65").unwrap()),
            strategy: PriceStrategy::Offset {
                direction: Direction::Above,
                offset: Offset::Exact(Decimal::from_str("0.10").unwrap()),
                tolerance: Some(Offset::Percent(50)),
            }
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Order(_) => to_json_binary(&OrderResponse {
                        owner: "scheduler".to_string(),
                        side: Side::Base,
                        price: Price::Fixed(Decimal::from_str("1.55").unwrap()),
                        rate: Decimal::from_str("0.1").unwrap(),
                        updated_at: Timestamp::from_seconds(12),
                        offer: Uint128::new(7123123),
                        remaining: Uint128::new(12312),
                        filled: Uint128::new(23453),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.45").unwrap(),
                            total: Uint128::new(1_000_000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.35").unwrap(),
                            total: Uint128::new(1_000_000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!("unexpected query type"),
                },
                _ => panic!("unexpected query type"),
            }))
        });

        assert!(Cadence::LimitOrder {
            pair_address: Addr::unchecked("pair"),
            side: Side::Base,
            previous: Some(Decimal::from_str("1.65").unwrap()),
            strategy: PriceStrategy::Offset {
                direction: Direction::Above,
                offset: Offset::Exact(Decimal::from_str("0.10").unwrap()),
                tolerance: Some(Offset::Percent(1)),
            }
        }
        .is_due(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap());
    }
}
