use calc_rs::types::{Condition, ConditionFilter, Trigger};
use cosmwasm_std::{Addr, Deps, Env, Order, StdError, StdResult, Storage};
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, Item, MultiIndex};

pub const TRIGGER_COUNTER: Item<u64> = Item::new("trigger_counter");

pub struct TriggerIndexes<'a> {
    pub owner: MultiIndex<'a, Addr, Trigger, u64>,
    pub timestamp: MultiIndex<'a, u64, Trigger, u64>,
    pub block_height: MultiIndex<'a, u64, Trigger, u64>,
}

impl<'a> IndexList<Trigger> for TriggerIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<Trigger>> + '_> {
        let v: Vec<&dyn Index<Trigger>> = vec![&self.owner, &self.timestamp, &self.block_height];
        Box::new(v.into_iter())
    }
}

fn triggers<'a>() -> IndexedMap<u64, Trigger, TriggerIndexes<'a>> {
    IndexedMap::new(
        "triggers",
        TriggerIndexes {
            owner: MultiIndex::new(|_, t| t.owner.clone(), "triggers", "triggers__owner"),
            timestamp: MultiIndex::new(
                |_, t| {
                    t.conditions
                        .iter()
                        .map(|c| match c {
                            Condition::TimestampElapsed(timestamp) => timestamp.seconds(),
                            _ => u64::MAX,
                        })
                        .min()
                        .unwrap_or(u64::MAX)
                },
                "triggers",
                "triggers__timestamp",
            ),
            block_height: MultiIndex::new(
                |_, t| {
                    t.conditions
                        .iter()
                        .map(|c| match c {
                            Condition::BlocksCompleted(height) => *height,
                            _ => u64::MAX,
                        })
                        .min()
                        .unwrap_or(u64::MAX)
                },
                "triggers",
                "triggers__block_height",
            ),
        },
    )
}

pub fn save_trigger(storage: &mut dyn Storage, trigger: Trigger) -> StdResult<()> {
    let id = TRIGGER_COUNTER
        .update(storage, |id| Ok::<u64, StdError>(id + 1))
        .unwrap_or_else(|_| {
            TRIGGER_COUNTER.save(storage, &1).unwrap();
            1
        });

    triggers().save(storage, id, &Trigger { id, ..trigger })
}

pub fn fetch_trigger(storage: &dyn Storage, id: u64) -> StdResult<Trigger> {
    triggers()
        .load(storage, id)
        .map_err(|_| StdError::not_found(format!("Trigger with id {} not found", id)))
}

pub fn fetch_triggers(
    deps: Deps,
    env: &Env,
    filter: ConditionFilter,
    limit: Option<usize>,
    can_execute: Option<bool>,
) -> StdResult<Vec<Trigger>> {
    let triggers = match filter {
        ConditionFilter::Owner { address } => {
            triggers()
                .idx
                .owner
                .prefix(address)
                .range(deps.storage, None, None, Order::Ascending)
        }
        ConditionFilter::Timestamp { start, end } => {
            if can_execute.is_some() {
                return Err(StdError::generic_err(
                    "Cannot include a value for can_execute when filtering by timestamp",
                ));
            }
            triggers().idx.timestamp.range(
                deps.storage,
                start.map(|s| Bound::inclusive((s.seconds(), u64::MAX))),
                end.map(|e| Bound::inclusive((e.seconds(), u64::MAX))),
                Order::Ascending,
            )
        }
        ConditionFilter::BlockHeight { start, end } => {
            if can_execute.is_some() {
                return Err(StdError::generic_err(
                    "Cannot include a value for can_execute when filtering by block height",
                ));
            }
            triggers().idx.block_height.range(
                deps.storage,
                start.map(|s| Bound::inclusive((s, u64::MAX))),
                end.map(|e| Bound::inclusive((e, u64::MAX))),
                Order::Ascending,
            )
        }
    }
    .take(match limit {
        Some(limit) => match limit {
            0..=50 => limit,
            _ => 50,
        },
        _ => 50,
    })
    .flat_map(|r| r.map(|(_, v)| v));

    Ok(if let Some(can_execute) = can_execute {
        triggers
            .filter(|trigger| trigger.can_execute(deps, env).unwrap_or(false) == can_execute)
            .collect::<Vec<Trigger>>()
    } else {
        triggers.collect::<Vec<Trigger>>()
    })
}

pub fn delete_trigger(storage: &mut dyn Storage, id: u64) -> StdResult<()> {
    triggers().remove(storage, id)
}

#[cfg(test)]
mod trigger_state_tests {
    use super::*;

    use calc_rs::types::TriggerConditionsThreshold;
    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::{testing::mock_dependencies, Addr, Timestamp};
    use cosmwasm_std::{to_json_binary, Coin};
    use std::vec;

    fn default_trigger() -> Trigger {
        Trigger {
            id: 1,
            owner: Addr::unchecked("owner"),
            conditions: vec![Condition::TimestampElapsed(Timestamp::from_seconds(
                mock_env().block.time.seconds() + 10,
            ))],
            threshold: TriggerConditionsThreshold::All,
            msg: to_json_binary(&"default message").unwrap(),
            to: Addr::unchecked("to"),
            execution_rebate: vec![Coin::new(1u128, "rune")],
        }
    }

    #[test]
    fn saves_a_trigger() {
        let mut deps = mock_dependencies();
        let trigger = default_trigger();

        save_trigger(deps.as_mut().storage, trigger.clone()).unwrap();

        assert_eq!(triggers().load(deps.as_ref().storage, 1).unwrap(), trigger);
    }

    #[test]
    fn saves_two_identical_triggers() {
        let mut deps = mock_dependencies();
        let trigger = default_trigger();

        save_trigger(deps.as_mut().storage, trigger.clone()).unwrap();
        save_trigger(deps.as_mut().storage, trigger.clone()).unwrap();

        assert_eq!(
            triggers().load(deps.as_ref().storage, 1).unwrap(),
            Trigger {
                id: 1,
                ..trigger.clone()
            }
        );
        assert_eq!(
            triggers().load(deps.as_ref().storage, 2).unwrap(),
            Trigger { id: 2, ..trigger }
        );
    }

    #[test]
    fn fetches_triggers_by_owner() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let owner1 = Addr::unchecked("owner1");
        let owner2 = Addr::unchecked("owner2");

        let trigger = default_trigger();

        save_trigger(
            deps.as_mut().storage,
            Trigger {
                owner: owner1.clone(),
                ..trigger.clone()
            },
        )
        .unwrap();

        save_trigger(
            deps.as_mut().storage,
            Trigger {
                owner: owner2.clone(),
                ..trigger.clone()
            },
        )
        .unwrap();

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Owner {
                    address: owner1.clone(),
                },
                None,
                None,
            )
            .unwrap(),
            vec![Trigger {
                id: 1,
                owner: owner1.clone(),
                ..trigger.clone()
            }]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Owner {
                    address: owner2.clone(),
                },
                None,
                None
            )
            .unwrap(),
            vec![Trigger {
                id: 2,
                owner: owner2.clone(),
                ..trigger.clone()
            }]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Owner {
                    address: Addr::unchecked("nonexistent"),
                },
                None,
                None
            )
            .unwrap(),
            vec![]
        );

        env.block.time = Timestamp::from_seconds(900);

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Owner {
                    address: owner2.clone(),
                },
                None,
                Some(true)
            )
            .unwrap(),
            vec![]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Owner {
                    address: owner2.clone(),
                },
                None,
                Some(false)
            )
            .unwrap(),
            vec![Trigger {
                id: 2,
                owner: owner2.clone(),
                ..trigger.clone()
            }]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Owner {
                    address: owner2.clone(),
                },
                None,
                None
            )
            .unwrap(),
            vec![Trigger {
                id: 2,
                owner: owner2.clone(),
                ..trigger.clone()
            }]
        );
    }

    #[test]
    fn fetches_triggers_by_timestamp() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let conditions = vec![
            Condition::TimestampElapsed(Timestamp::from_seconds(env.block.time.seconds() + 1000)),
            Condition::TimestampElapsed(Timestamp::from_seconds(env.block.time.seconds() + 2000)),
        ];

        let trigger = default_trigger();

        for condition in conditions.clone() {
            save_trigger(
                deps.as_mut().storage,
                Trigger {
                    conditions: vec![condition.clone()],
                    ..trigger.clone()
                },
            )
            .unwrap();
        }

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: None,
                    end: None,
                },
                None,
                None
            )
            .unwrap(),
            vec![
                Trigger {
                    id: 1,
                    conditions: vec![conditions[0].clone()],
                    ..trigger.clone()
                },
                Trigger {
                    id: 2,
                    conditions: vec![conditions[1].clone()],
                    ..trigger.clone()
                }
            ]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: None,
                    end: Some(env.block.time.plus_seconds(2500)),
                },
                None,
                None
            )
            .unwrap(),
            vec![
                Trigger {
                    id: 1,
                    conditions: vec![conditions[0].clone()],
                    ..trigger.clone()
                },
                Trigger {
                    id: 2,
                    conditions: vec![conditions[1].clone()],
                    ..trigger.clone()
                }
            ]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: Some(env.block.time),
                    end: None,
                },
                None,
                None
            )
            .unwrap(),
            vec![
                Trigger {
                    id: 1,
                    conditions: vec![conditions[0].clone()],
                    ..trigger.clone()
                },
                Trigger {
                    id: 2,
                    conditions: vec![conditions[1].clone()],
                    ..trigger.clone()
                }
            ]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: Some(env.block.time.plus_seconds(500)),
                    end: Some(env.block.time.plus_seconds(1500)),
                },
                None,
                None
            )
            .unwrap(),
            vec![Trigger {
                id: 1,
                conditions: vec![conditions[0].clone()],
                ..trigger.clone()
            }]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: Some(env.block.time.plus_seconds(1500)),
                    end: Some(env.block.time.plus_seconds(2500)),
                },
                None,
                None
            )
            .unwrap(),
            vec![Trigger {
                id: 2,
                conditions: vec![conditions[1].clone()],
                ..trigger.clone()
            }]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: Some(env.block.time.plus_seconds(2500)),
                    end: None,
                },
                None,
                None
            )
            .unwrap(),
            vec![]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: Some(env.block.time.plus_seconds(2500)),
                    end: None,
                },
                None,
                Some(true)
            )
            .unwrap_err(),
            StdError::generic_err(
                "Cannot include a value for can_execute when filtering by timestamp"
            )
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: Some(env.block.time.plus_seconds(2500)),
                    end: None,
                },
                None,
                Some(false)
            )
            .unwrap_err(),
            StdError::generic_err(
                "Cannot include a value for can_execute when filtering by timestamp"
            )
        );
    }

    #[test]
    fn fetches_triggers_by_block_height() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let conditions = vec![
            Condition::BlocksCompleted(env.block.height + 1000),
            Condition::BlocksCompleted(env.block.height + 2000),
        ];

        let trigger = default_trigger();

        for condition in conditions.clone() {
            save_trigger(
                deps.as_mut().storage,
                Trigger {
                    conditions: vec![condition.clone()],
                    ..trigger.clone()
                },
            )
            .unwrap();
        }

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::BlockHeight {
                    start: None,
                    end: None,
                },
                None,
                None
            )
            .unwrap(),
            vec![
                Trigger {
                    id: 1,
                    conditions: vec![conditions[0].clone()],
                    ..trigger.clone()
                },
                Trigger {
                    id: 2,
                    conditions: vec![conditions[1].clone()],
                    ..trigger.clone()
                }
            ]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::BlockHeight {
                    start: Some(env.block.height),
                    end: None,
                },
                None,
                None
            )
            .unwrap(),
            vec![
                Trigger {
                    id: 1,
                    conditions: vec![conditions[0].clone()],
                    ..trigger.clone()
                },
                Trigger {
                    id: 2,
                    conditions: vec![conditions[1].clone()],
                    ..trigger.clone()
                }
            ]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::BlockHeight {
                    start: None,
                    end: Some(env.block.height + 2500),
                },
                None,
                None
            )
            .unwrap(),
            vec![
                Trigger {
                    id: 1,
                    conditions: vec![conditions[0].clone()],
                    ..trigger.clone()
                },
                Trigger {
                    id: 2,
                    conditions: vec![conditions[1].clone()],
                    ..trigger.clone()
                }
            ]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::BlockHeight {
                    start: Some(env.block.height + 1000),
                    end: Some(env.block.height + 1500),
                },
                None,
                Some(true)
            )
            .unwrap_err(),
            StdError::generic_err(
                "Cannot include a value for can_execute when filtering by block height"
            )
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::BlockHeight {
                    start: Some(env.block.height + 1000),
                    end: Some(env.block.height + 1500),
                },
                None,
                Some(false)
            )
            .unwrap_err(),
            StdError::generic_err(
                "Cannot include a value for can_execute when filtering by block height"
            )
        );
    }

    #[test]
    fn fetches_triggers_with_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let trigger = default_trigger();

        for i in 0..100 {
            save_trigger(
                deps.as_mut().storage,
                Trigger {
                    id: (i + 1) as u64,
                    conditions: vec![Condition::TimestampElapsed(Timestamp::from_seconds(
                        i * 100,
                    ))],
                    ..trigger.clone()
                },
            )
            .unwrap();
        }

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Owner {
                    address: trigger.owner.clone()
                },
                None,
                None
            )
            .unwrap(),
            (0..50)
                .map(|i| Trigger {
                    id: (i + 1) as u64,
                    conditions: vec![Condition::TimestampElapsed(Timestamp::from_seconds(
                        i * 100,
                    ))],
                    ..trigger.clone()
                })
                .collect::<Vec<Trigger>>()
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Owner {
                    address: trigger.owner.clone()
                },
                Some(10),
                None,
            )
            .unwrap(),
            (0..10)
                .map(|i| Trigger {
                    id: (i + 1) as u64,
                    conditions: vec![Condition::TimestampElapsed(Timestamp::from_seconds(
                        i * 100 as u64,
                    ))],
                    ..trigger.clone()
                })
                .collect::<Vec<Trigger>>()
        );
    }
}
