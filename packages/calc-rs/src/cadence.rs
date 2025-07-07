use std::{str::FromStr, time::Duration};

use chrono::{DateTime, Utc};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Env, Timestamp};
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
    Cron(String),
}

impl Cadence {
    pub fn is_due(&self, env: &Env) -> bool {
        match self {
            Cadence::Blocks { interval, previous } => {
                previous.map_or(true, |previous| env.block.height > previous + interval)
            }
            Cadence::Time { duration, previous } => previous.map_or(true, |previous| {
                env.block.time.seconds() > previous.seconds() + duration.as_secs()
            }),
            Cadence::Cron(cron_str) => {
                if let Ok(schedule) = CronSchedule::from_str(cron_str) {
                    if let Some(next) = schedule.upcoming(Utc).next() {
                        return env.block.time.seconds() >= next.timestamp() as u64;
                    }
                }
                false
            }
        }
    }

    pub fn into_condition(&self, env: &Env) -> Condition {
        match self {
            Cadence::Blocks { interval, previous } => Condition::BlocksCompleted(
                previous.map_or(env.block.height, |previous| previous + interval),
            ),
            Cadence::Time { duration, previous } => {
                Condition::TimestampElapsed(previous.map_or(env.block.time, |previous| {
                    previous.plus_seconds(duration.as_secs())
                }))
            }
            Cadence::Cron(cron_str) => {
                if let Ok(schedule) = CronSchedule::from_str(cron_str) {
                    if let Some(next) = schedule
                        .after(&DateTime::from_timestamp_nanos(
                            env.block.time.nanos() as i64
                        ))
                        .next()
                    {
                        return Condition::TimestampElapsed(Timestamp::from_seconds(
                            next.timestamp() as u64,
                        ));
                    }
                }
                // Return a condition that will never be met if cron is invalid
                Condition::BlocksCompleted(u64::MAX)
            }
        }
    }

    pub fn next(self, env: &Env) -> Self {
        match self {
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
            Cadence::Cron(_) => self,
        }
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
            .next(&env),
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
            .next(&env),
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
            .next(&env),
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
            .next(&env),
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
            .next(&env),
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
            .next(&env),
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
            .next(&env),
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
            .next(&env),
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.plus_seconds(5))
            }
        );
    }

    #[test]
    fn updates_to_next_scheduled_cron() {
        let env = mock_env();

        let cron = "0 0 * * * *";

        assert_eq!(
            Cadence::Cron(cron.to_string()).next(&env),
            Cadence::Cron(cron.to_string())
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
            .into_condition(&env),
            Condition::BlocksCompleted(env.block.height)
        );

        assert_eq!(
            Cadence::Blocks {
                interval: 10,
                previous: Some(env.block.height)
            }
            .into_condition(&env),
            Condition::BlocksCompleted(env.block.height + 10)
        );

        assert_eq!(
            Cadence::Blocks {
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
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: None
            }
            .into_condition(&env),
            Condition::TimestampElapsed(env.block.time)
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time)
            }
            .into_condition(&env),
            Condition::TimestampElapsed(env.block.time.plus_seconds(10))
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .into_condition(&env),
            Condition::TimestampElapsed(env.block.time.plus_seconds(10 - 5))
        );

        assert_eq!(
            Cadence::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(155))
            }
            .into_condition(&env),
            Condition::TimestampElapsed(env.block.time.minus_seconds(155 - 10))
        );
    }

    #[test]
    fn gets_next_cron_condition() {
        let env = mock_env();

        let cron = "0 0 * * * *";

        assert_eq!(
            Cadence::Cron(cron.to_string()).into_condition(&env),
            Cadence::Cron(cron.to_string()).into_condition(&env),
        );

        let cron = "bad cron";

        assert_eq!(
            Cadence::Cron(cron.to_string()).into_condition(&env),
            Condition::BlocksCompleted(u64::MAX)
        );
    }

    #[test]
    fn block_schedule_is_due() {
        let env = mock_env();

        assert!(Cadence::Blocks {
            interval: 10,
            previous: None
        }
        .is_due(&env));

        assert!(!Cadence::Blocks {
            interval: 10,
            previous: Some(env.block.height - 5)
        }
        .is_due(&env));

        assert!(Cadence::Blocks {
            interval: 5,
            previous: Some(env.block.height - 6)
        }
        .is_due(&env));

        assert!(!Cadence::Blocks {
            interval: 5,
            previous: Some(env.block.height - 5)
        }
        .is_due(&env));
    }

    #[test]
    fn time_schedule_is_due() {
        let env = mock_env();

        assert!(Cadence::Time {
            duration: Duration::from_secs(10),
            previous: None
        }
        .is_due(&env));

        assert!(!Cadence::Time {
            duration: Duration::from_secs(10),
            previous: Some(env.block.time.minus_seconds(5))
        }
        .is_due(&env));

        assert!(Cadence::Time {
            duration: Duration::from_secs(5),
            previous: Some(env.block.time.minus_seconds(6))
        }
        .is_due(&env));

        assert!(!Cadence::Time {
            duration: Duration::from_secs(5),
            previous: Some(env.block.time.minus_seconds(5))
        }
        .is_due(&env));
    }
}
