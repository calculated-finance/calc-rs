use std::{cmp::max, str::FromStr, time::Duration};

use chrono::DateTime;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Env, StdResult, Timestamp};
use cron::Schedule as CronSchedule;

use crate::conditions::Condition;

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
}

impl Cadence {
    pub fn is_due(&self, env: &Env) -> StdResult<bool> {
        Ok(match self {
            Cadence::Blocks { interval, previous } => {
                previous.is_none_or(|previous| env.block.height > previous + interval)
            }
            Cadence::Time { duration, previous } => previous.is_none_or(|previous| {
                env.block.time.seconds() > previous.seconds() + duration.as_secs()
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
                    env.block.time.seconds() > next.timestamp() as u64
                } else {
                    false
                }
            }
        })
    }

    pub fn into_condition(&self, env: &Env) -> StdResult<Condition> {
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
        })
    }

    pub fn next(self, env: &Env) -> StdResult<Self> {
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
                        previous: None,
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::cadence::Cadence;

    use super::*;

    use std::time::Duration;

    use cosmwasm_std::testing::mock_env;

    #[test]
    fn updates_to_next_scheduled_block() {
        let env = mock_env();

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: None
            }
            .next(&env)
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
            .next(&env)
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
            .next(&env)
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
            .next(&env)
            .unwrap(),
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height + 5)
            }
        );
    }

    #[test]
    fn updates_to_next_scheduled_time() {
        let env = mock_env();

        assert_eq!(
            Cadence::Time {
                duration: std::time::Duration::from_secs(10),
                previous: None
            }
            .next(&env)
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
            .next(&env)
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
            .next(&env)
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
            .next(&env)
            .unwrap(),
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.plus_seconds(5))
            }
        );
    }

    #[test]
    fn updates_to_next_scheduled_cron() {
        let env = mock_env();

        let cron = "*/10 * * * * *";

        assert_eq!(
            Cadence::Cron {
                expr: cron.to_string(),
                previous: None
            }
            .next(&env)
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
            .next(&env)
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
            .next(&env)
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
            .next(&env)
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
        let env = mock_env();

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: None
            }
            .into_condition(&env)
            .unwrap(),
            Condition::BlocksCompleted(env.block.height)
        );

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height)
            }
            .into_condition(&env)
            .unwrap(),
            Condition::BlocksCompleted(env.block.height + 10)
        );

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
            .into_condition(&env)
            .unwrap(),
            Condition::BlocksCompleted(env.block.height - 5 + 10)
        );
    }

    #[test]
    fn gets_next_time_condition() {
        let env = mock_env();

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: None
            }
            .into_condition(&env)
            .unwrap(),
            Condition::TimestampElapsed(env.block.time)
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time)
            }
            .into_condition(&env)
            .unwrap(),
            Condition::TimestampElapsed(env.block.time.plus_seconds(10))
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .into_condition(&env)
            .unwrap(),
            Condition::TimestampElapsed(env.block.time.plus_seconds(10 - 5))
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(155))
            }
            .into_condition(&env)
            .unwrap(),
            Condition::TimestampElapsed(env.block.time.minus_seconds(155 - 10))
        );
    }

    #[test]
    fn gets_next_cron_condition() {
        let env = mock_env();

        let cron = "* * * * * *";

        assert_eq!(
            Cadence::Cron {
                expr: cron.to_string(),
                previous: None
            }
            .into_condition(&env)
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
        .into_condition(&env)
        .unwrap_err()
        .to_string()
        .contains("Invalid cron expression"));
    }

    #[test]
    fn block_schedule_is_due() {
        let env = mock_env();

        assert!(Cadence::Blocks {
            interval: 10,
            previous: None
        }
        .is_due(&env)
        .unwrap());

        assert!(!Cadence::Blocks {
            interval: 10,
            previous: Some(env.block.height - 5)
        }
        .is_due(&env)
        .unwrap());

        assert!(Cadence::Blocks {
            interval: 5,
            previous: Some(env.block.height - 6)
        }
        .is_due(&env)
        .unwrap());

        assert!(!Cadence::Blocks {
            interval: 5,
            previous: Some(env.block.height - 5)
        }
        .is_due(&env)
        .unwrap());
    }

    #[test]
    fn time_schedule_is_due() {
        let env = mock_env();

        assert!(Cadence::Time {
            duration: Duration::from_secs(10),
            previous: None
        }
        .is_due(&env)
        .unwrap());

        assert!(!Cadence::Time {
            duration: Duration::from_secs(10),
            previous: Some(env.block.time.minus_seconds(5))
        }
        .is_due(&env)
        .unwrap());

        assert!(Cadence::Time {
            duration: Duration::from_secs(5),
            previous: Some(env.block.time.minus_seconds(6))
        }
        .is_due(&env)
        .unwrap());

        assert!(!Cadence::Time {
            duration: Duration::from_secs(5),
            previous: Some(env.block.time.minus_seconds(5))
        }
        .is_due(&env)
        .unwrap());
    }

    #[test]
    fn cron_schedule_is_due() {
        let env = mock_env();
        let cron = "*/10 * * * * *";

        assert!(Cadence::Cron {
            expr: cron.to_string(),
            previous: None
        }
        .is_due(&env)
        .unwrap());

        assert!(!Cadence::Cron {
            expr: cron.to_string(),
            previous: Some(env.block.time.minus_seconds(5))
        }
        .is_due(&env)
        .unwrap());

        assert!(Cadence::Cron {
            expr: cron.to_string(),
            previous: Some(env.block.time.minus_seconds(15))
        }
        .is_due(&env)
        .unwrap());
    }
}
