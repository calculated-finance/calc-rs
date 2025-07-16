use std::{cmp::max, str::FromStr, time::Duration};

use chrono::DateTime;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Deps, Env, StdResult, Timestamp};
use cron::Schedule as CronSchedule;
use rujira_rs::fin::Side;

use crate::{actions::limit_order::OrderPriceStrategy, conditions::Condition};

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
        strategy: OrderPriceStrategy,
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
            Cadence::Cron { expr, previous } => {
                if previous.is_none() {
                    return Ok(true);
                }

                let schedule = CronSchedule::from_str(expr).map_err(|e| {
                    cosmwasm_std::StdError::generic_err(format!("Invalid cron string: {e}"))
                })?;

                let next = schedule
                    .after(&DateTime::from_timestamp_nanos(
                        previous.unwrap_or(env.block.time).nanos() as i64,
                    ))
                    .next();

                if let Some(next) = next {
                    env.block.time.seconds() >= next.timestamp() as u64
                } else {
                    false
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
                        OrderPriceStrategy::Fixed(price) => Condition::LimitOrderFilled {
                            owner: scheduler.clone(),
                            pair_address: pair_address.clone(),
                            side: side.clone(),
                            price: price.clone(),
                        }
                        .is_satisfied(deps, env)?,
                        OrderPriceStrategy::Offset { .. } => {
                            let previous_order_filled = Condition::LimitOrderFilled {
                                owner: scheduler.clone(),
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
                    return Ok(true);
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
                let schedule = CronSchedule::from_str(expr).map_err(|e| {
                    cosmwasm_std::StdError::generic_err(format!("Invalid cron string: {e}"))
                })?;

                let next = schedule
                    .after(&DateTime::from_timestamp_nanos(
                        previous
                            .map_or(env.block.time, |previous| max(previous, env.block.time))
                            .nanos() as i64,
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

                Condition::LimitOrderFilled {
                    owner: scheduler.clone(),
                    pair_address: pair_address.clone(),
                    side: side.clone(),
                    price,
                }
            }
        })
    }

    pub fn next(self, deps: Deps, env: &Env) -> StdResult<Self> {
        Ok(match self {
            Cadence::Blocks { interval, previous } => Cadence::Blocks {
                interval,
                previous: Some(previous.map_or(env.block.height, |previous| {
                    let next = previous + interval;
                    if next < env.block.height {
                        let blocks_completed = env.block.height - previous;
                        env.block.height + blocks_completed % interval
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
                    if next < env.block.time {
                        let time_elapsed = env.block.time.seconds() - previous.seconds();
                        env.block.time.plus_seconds(time_elapsed % duration)
                    } else {
                        next
                    }
                })),
            },
            Cadence::Cron { expr, previous } => {
                let schedule = CronSchedule::from_str(&expr).map_err(|e| {
                    cosmwasm_std::StdError::generic_err(format!("Invalid cron string: {e}"))
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
                    Cadence::Blocks {
                        interval: u64::MAX,
                        previous: Some(u64::MAX),
                    }
                }
            }
            Cadence::LimitOrder {
                pair_address,
                side,
                strategy,
                ..
            } => Cadence::LimitOrder {
                pair_address: pair_address.clone(),
                side: side.clone(),
                previous: Some(strategy.get_new_price(deps, &pair_address, &side)?),
                strategy,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::cadence::Cadence;

    use super::*;

    use cosmwasm_std::testing::{mock_dependencies, mock_env};
    use std::time::Duration;

    #[test]
    fn updates_to_next_scheduled_block() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: None
            }
            .next(deps.as_ref(), &env)
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
            .next(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5 + 10)
            }
        );

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 15)
            }
            .next(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height + 5)
            }
        );

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 155)
            }
            .next(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height + 5)
            }
        );
    }

    #[test]
    fn updates_to_next_scheduled_time() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert_eq!(
            Cadence::Time {
                duration: std::time::Duration::from_secs(10),
                previous: None
            }
            .next(deps.as_ref(), &env)
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
            .next(deps.as_ref(), &env)
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
            .next(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.plus_seconds(5))
            }
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(155))
            }
            .next(deps.as_ref(), &env)
            .unwrap(),
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.plus_seconds(5))
            }
        );
    }

    #[test]
    fn updates_to_next_scheduled_cron() {
        let deps = mock_dependencies();
        let env = mock_env();

        let cron = "*/10 * * * * *";

        assert_eq!(
            Cadence::Cron {
                expr: cron.to_string(),
                previous: None
            }
            .next(deps.as_ref(), &env)
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
            .next(deps.as_ref(), &env)
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
            .next(deps.as_ref(), &env)
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
            .next(deps.as_ref(), &env)
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

        let cron = "* * * * * *";

        assert_eq!(
            Cadence::Cron {
                expr: cron.to_string(),
                previous: None
            }
            .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
            .unwrap(),
            Condition::TimestampElapsed(Timestamp::from_seconds(
                env.block.time.seconds() - env.block.time.seconds() % 10 + 10
            )),
        );

        let cron = "bad cron";

        assert!(Cadence::Cron {
            expr: cron.to_string(),
            previous: None
        }
        .into_condition(deps.as_ref(), &env, &Addr::unchecked("scheduler"))
        .unwrap_err()
        .to_string()
        .contains("Invalid cron expression"));
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
}
