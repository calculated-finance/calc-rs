use calc_rs::types::{Condition, ConditionFilter, Trigger};
use cosmwasm_std::{Addr, Binary, Coin, Deps, Order, StdError, StdResult, Storage, Uint64};
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, Item, MultiIndex};

pub const TRIGGER_COUNTER: Item<u64> = Item::new("condition_counter");

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

pub fn fetch_triggers(deps: Deps, filter: ConditionFilter, limit: Option<usize>) -> Vec<Trigger> {
    match filter {
        ConditionFilter::Owner { address } => {
            triggers()
                .idx
                .owner
                .prefix(address)
                .range(deps.storage, None, None, Order::Ascending)
        }
        ConditionFilter::Timestamp { start, end } => triggers().idx.timestamp.range(
            deps.storage,
            start.map(|s| Bound::inclusive((s.seconds(), u64::MAX))),
            end.map(|e| Bound::inclusive((e.seconds(), u64::MAX))),
            Order::Ascending,
        ),
        ConditionFilter::BlockHeight { start, end } => triggers().idx.block_height.range(
            deps.storage,
            start.map(|s| Bound::inclusive((s, u64::MAX))),
            end.map(|e| Bound::inclusive((e, u64::MAX))),
            Order::Ascending,
        ),
    }
    .take(match limit {
        Some(limit) => match limit {
            0..=50 => limit,
            _ => 50,
        },
        _ => 50,
    })
    .flat_map(|r| r.map(|(_, v)| v))
    .collect::<Vec<Trigger>>()
}

pub fn delete_trigger(storage: &mut dyn Storage, id: u64) -> StdResult<()> {
    triggers().remove(storage, id)
}

#[cfg(test)]
mod tests {
    use super::*;

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
                ConditionFilter::Owner {
                    address: owner1.clone(),
                },
                None,
            ),
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
                ConditionFilter::Owner {
                    address: owner2.clone(),
                },
                None,
            ),
            vec![Trigger {
                id: 2,
                owner: owner2,
                condition: condition.clone(),
                msg: msg.clone(),
                to: to.clone(),
                execution_rebate: execution_rebate.clone(),
            }]
        );
    }
}
