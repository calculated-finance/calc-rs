use calc_rs::types::{Condition, ConditionFilter, Trigger};
use cosmwasm_std::{Addr, Binary, Coin, Deps, Env, Order, StdError, StdResult, Storage, Uint64};
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, Item, MultiIndex};

use crate::types::Executable;

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

pub fn triggers<'a>() -> IndexedMap<u64, Trigger, TriggerIndexes<'a>> {
    IndexedMap::new(
        "triggers",
        TriggerIndexes {
            owner: MultiIndex::new(|_, t| t.owner.clone(), "triggers", "triggers__owner"),
            timestamp: MultiIndex::new(
                |_, t| match &t.condition {
                    Condition::Timestamp { timestamp } => timestamp.seconds(),
                    _ => Uint64::MAX.into(),
                },
                "triggers",
                "triggers__timestamp",
            ),
            block_height: MultiIndex::new(
                |_, t| match &t.condition {
                    Condition::BlockHeight { height } => *height,
                    _ => Uint64::MAX.into(),
                },
                "triggers",
                "triggers__block_height",
            ),
        },
    )
}

pub fn save_trigger(
    storage: &mut dyn Storage,
    owner: Addr,
    condition: Condition,
    msg: Binary,
    to: Addr,
    execution_rebate: Vec<Coin>,
) -> StdResult<()> {
    let id = TRIGGER_COUNTER.update(storage, |id| Ok::<u64, StdError>(id + 1))?;
    triggers().save(
        storage,
        id,
        &Trigger {
            id,
            owner,
            condition,
            msg,
            to,
            execution_rebate,
        },
    )
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
    .flat_map(|r| r.map(|(_, v)| v))
    .filter(|trigger| {
        if let Some(can_execute) = can_execute {
            trigger.can_execute(env) == can_execute
        } else {
            true
        }
    })
    .collect::<Vec<Trigger>>();

    Ok(triggers)
}

pub fn delete_trigger(storage: &mut dyn Storage, id: u64) -> StdResult<()> {
    triggers().remove(storage, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::to_json_binary;
    use cosmwasm_std::{testing::mock_dependencies, Addr, Timestamp};
    use std::vec;

    #[test]
    fn saves_a_trigger() {
        let mut deps = mock_dependencies();
        let owner = Addr::unchecked("owner");
        let to = Addr::unchecked("to");
        let condition = Condition::Timestamp {
            timestamp: Timestamp::from_seconds(1000),
        };
        let msg = to_json_binary(&"test message").unwrap();
        let execution_rebate = vec![Coin::new(1u128, "rune")];

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        save_trigger(
            deps.as_mut().storage,
            owner.clone(),
            condition.clone(),
            msg.clone(),
            to.clone(),
            execution_rebate.clone(),
        )
        .unwrap();

        assert_eq!(
            triggers().load(deps.as_ref().storage, 1).unwrap(),
            Trigger {
                id: 1,
                owner,
                condition,
                msg,
                to,
                execution_rebate,
            }
        );
    }

    #[test]
    fn fetches_triggers_by_owner() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let owner1 = Addr::unchecked("owner1");
        let owner2 = Addr::unchecked("owner2");
        let to = Addr::unchecked("to");
        let condition = Condition::Timestamp {
            timestamp: Timestamp::from_seconds(1000),
        };
        let msg = to_json_binary(&"test message").unwrap();
        let execution_rebate = vec![Coin::new(1u128, "rune")];

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        save_trigger(
            deps.as_mut().storage,
            owner1.clone(),
            condition.clone(),
            msg.clone(),
            to.clone(),
            execution_rebate.clone(),
        )
        .unwrap();

        save_trigger(
            deps.as_mut().storage,
            owner2.clone(),
            condition.clone(),
            msg.clone(),
            to.clone(),
            execution_rebate.clone(),
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
                None
            )
            .unwrap(),
            vec![Trigger {
                id: 1,
                owner: owner1,
                condition: condition.clone(),
                msg: msg.clone(),
                to: to.clone(),
                execution_rebate: execution_rebate.clone(),
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
                condition: condition.clone(),
                msg: msg.clone(),
                to: to.clone(),
                execution_rebate: execution_rebate.clone(),
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
                condition: condition.clone(),
                msg: msg.clone(),
                to: to.clone(),
                execution_rebate: execution_rebate.clone(),
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
                condition: condition.clone(),
                msg: msg.clone(),
                to: to.clone(),
                execution_rebate: execution_rebate.clone(),
            }]
        );
    }

    #[test]
    fn fetches_triggers_by_timestamp() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = Addr::unchecked("owner");
        let to = Addr::unchecked("to");
        let msg = to_json_binary(&"test message").unwrap();
        let execution_rebate = vec![Coin::new(1u128, "rune")];

        let conditions = vec![
            Condition::Timestamp {
                timestamp: Timestamp::from_seconds(1000),
            },
            Condition::Timestamp {
                timestamp: Timestamp::from_seconds(2000),
            },
        ];

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for condition in conditions.clone() {
            save_trigger(
                deps.as_mut().storage,
                owner.clone(),
                condition.clone(),
                msg.clone(),
                to.clone(),
                execution_rebate.clone(),
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
                    owner: owner.clone(),
                    condition: conditions[0].clone(),
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                },
                Trigger {
                    id: 2,
                    owner: owner.clone(),
                    condition: conditions[1].clone(),
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                }
            ]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: None,
                    end: Some(Timestamp::from_seconds(2500)),
                },
                None,
                None
            )
            .unwrap(),
            vec![
                Trigger {
                    id: 1,
                    owner: owner.clone(),
                    condition: conditions[0].clone(),
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                },
                Trigger {
                    id: 2,
                    owner: owner.clone(),
                    condition: conditions[1].clone(),
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                }
            ]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: Some(Timestamp::from_seconds(0)),
                    end: None,
                },
                None,
                None
            )
            .unwrap(),
            vec![
                Trigger {
                    id: 1,
                    owner: owner.clone(),
                    condition: conditions[0].clone(),
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                },
                Trigger {
                    id: 2,
                    owner: owner.clone(),
                    condition: conditions[1].clone(),
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                }
            ]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: Some(Timestamp::from_seconds(500)),
                    end: Some(Timestamp::from_seconds(1500)),
                },
                None,
                None
            )
            .unwrap(),
            vec![Trigger {
                id: 1,
                owner: owner.clone(),
                condition: conditions[0].clone(),
                msg: msg.clone(),
                to: to.clone(),
                execution_rebate: execution_rebate.clone(),
            }]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: Some(Timestamp::from_seconds(1500)),
                    end: Some(Timestamp::from_seconds(2500)),
                },
                None,
                None
            )
            .unwrap(),
            vec![Trigger {
                id: 2,
                owner: owner.clone(),
                condition: conditions[1].clone(),
                msg: msg.clone(),
                to: to.clone(),
                execution_rebate: execution_rebate.clone(),
            }]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Timestamp {
                    start: Some(Timestamp::from_seconds(2500)),
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
                    start: Some(Timestamp::from_seconds(2500)),
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
                    start: Some(Timestamp::from_seconds(2500)),
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
        let owner = Addr::unchecked("owner");
        let to = Addr::unchecked("to");
        let msg = to_json_binary(&"test message").unwrap();
        let execution_rebate = vec![Coin::new(1u128, "rune")];

        let conditions = vec![
            Condition::BlockHeight { height: 1000 },
            Condition::BlockHeight { height: 2000 },
        ];

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for condition in conditions.clone() {
            save_trigger(
                deps.as_mut().storage,
                owner.clone(),
                condition.clone(),
                msg.clone(),
                to.clone(),
                execution_rebate.clone(),
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
                    owner: owner.clone(),
                    condition: conditions[0].clone(),
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                },
                Trigger {
                    id: 2,
                    owner: owner.clone(),
                    condition: conditions[1].clone(),
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                }
            ]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::BlockHeight {
                    start: Some(0),
                    end: None,
                },
                None,
                None
            )
            .unwrap(),
            vec![
                Trigger {
                    id: 1,
                    owner: owner.clone(),
                    condition: conditions[0].clone(),
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                },
                Trigger {
                    id: 2,
                    owner: owner.clone(),
                    condition: conditions[1].clone(),
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                }
            ]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::BlockHeight {
                    start: None,
                    end: Some(2500),
                },
                None,
                None
            )
            .unwrap(),
            vec![
                Trigger {
                    id: 1,
                    owner: owner.clone(),
                    condition: conditions[0].clone(),
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                },
                Trigger {
                    id: 2,
                    owner: owner.clone(),
                    condition: conditions[1].clone(),
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                }
            ]
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::BlockHeight {
                    start: Some(1000),
                    end: Some(1500),
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
                    start: Some(1000),
                    end: Some(1500),
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
    fn fetches_triggers_up_to_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = Addr::unchecked("owner");
        let to = Addr::unchecked("to");
        let msg = to_json_binary(&"test message").unwrap();
        let execution_rebate = vec![Coin::new(1u128, "rune")];

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 0..100 {
            save_trigger(
                deps.as_mut().storage,
                owner.clone(),
                Condition::Timestamp {
                    timestamp: Timestamp::from_seconds(i as u64),
                },
                msg.clone(),
                to.clone(),
                execution_rebate.clone(),
            )
            .unwrap();
        }

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Owner {
                    address: owner.clone()
                },
                Some(10),
                None,
            )
            .unwrap(),
            (0..10)
                .map(|i| Trigger {
                    id: (i + 1) as u64,
                    owner: owner.clone(),
                    condition: Condition::Timestamp {
                        timestamp: Timestamp::from_seconds(i as u64),
                    },
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                })
                .collect::<Vec<Trigger>>()
        );

        assert_eq!(
            fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Owner {
                    address: owner.clone()
                },
                None,
                None
            )
            .unwrap(),
            (0..50)
                .map(|i| Trigger {
                    id: (i + 1) as u64,
                    owner: owner.clone(),
                    condition: Condition::Timestamp {
                        timestamp: Timestamp::from_seconds(i as u64),
                    },
                    msg: msg.clone(),
                    to: to.clone(),
                    execution_rebate: execution_rebate.clone(),
                })
                .collect::<Vec<Trigger>>()
        );
    }
}
