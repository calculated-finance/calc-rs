use calc_rs::{
    conditions::condition::Condition,
    scheduler::{ConditionFilter, Trigger},
};
use cosmwasm_std::{Addr, Order, StdResult, Storage, Uint64};
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, Item, MultiIndex};

pub const MANAGER: Item<Addr> = Item::new("manager");

pub struct TriggerIndexes<'a> {
    pub timestamp: MultiIndex<'a, u64, Trigger, u64>,
    pub block_height: MultiIndex<'a, u64, Trigger, u64>,
}

impl<'a> IndexList<Trigger> for TriggerIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<Trigger>> + '_> {
        let v: Vec<&dyn Index<Trigger>> = vec![&self.timestamp, &self.block_height];
        Box::new(v.into_iter())
    }
}

pub struct TriggerStore<'a> {
    triggers: IndexedMap<u64, Trigger, TriggerIndexes<'a>>,
}

impl TriggerStore<'_> {
    pub fn save(&self, storage: &mut dyn Storage, trigger: &Trigger) -> StdResult<()> {
        self.triggers.save(storage, trigger.id.into(), trigger)
    }

    pub fn load(&self, storage: &dyn Storage, id: Uint64) -> StdResult<Trigger> {
        self.triggers.load(storage, id.into())
    }

    pub fn filtered(
        &self,
        storage: &dyn Storage,
        filter: ConditionFilter,
        limit: Option<usize>,
    ) -> StdResult<Vec<Trigger>> {
        let triggers = match filter {
            ConditionFilter::BlockHeight { start, end } => self.triggers.idx.block_height.range(
                storage,
                start.map(|s| Bound::inclusive((s, u64::MIN))),
                end.map(|e| Bound::inclusive((e, u64::MAX))),
                Order::Ascending,
            ),
            ConditionFilter::Timestamp { start, end } => self.triggers.idx.timestamp.range(
                storage,
                start.map(|s| Bound::inclusive((s.seconds(), u64::MIN))),
                end.map(|e| Bound::inclusive((e.seconds(), u64::MAX))),
                Order::Ascending,
            ),
        }
        .take(limit.unwrap_or(30))
        .flat_map(|r| r.map(|(_, v)| v))
        .collect::<Vec<_>>();

        Ok(triggers)
    }

    pub fn delete(&self, storage: &mut dyn Storage, id: u64) -> StdResult<()> {
        self.triggers.remove(storage, id)
    }
}

pub const TRIGGERS: TriggerStore<'static> = TriggerStore {
    triggers: IndexedMap::new(
        "triggers_v1",
        TriggerIndexes {
            timestamp: MultiIndex::new(
                |_, t| match t.condition {
                    Condition::TimestampElapsed(timestamp) => timestamp.seconds(),
                    _ => u64::MAX,
                },
                "triggers_v1",
                "triggers_v1__timestamp",
            ),
            block_height: MultiIndex::new(
                |_, t| match t.condition {
                    Condition::BlocksCompleted(height) => height,
                    _ => u64::MAX,
                },
                "triggers_v1",
                "triggers_v1__block_height",
            ),
        },
    ),
};

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{testing::MockStorage, Binary, Timestamp};

    #[test]
    fn test_block_height_filter() {
        let storage = &mut MockStorage::default();
        let owner = Addr::unchecked("creator");

        let trigger_1 = Trigger {
            id: Uint64::from(1u64),
            owner: owner.clone(),
            condition: Condition::BlocksCompleted(100),
            msg: Binary::default(),
            contract_address: Addr::unchecked("contract1"),
            executors: vec![],
            execution_rebate: vec![],
            jitter: None,
        };

        let trigger_2 = Trigger {
            id: Uint64::from(2u64),
            owner,
            condition: Condition::BlocksCompleted(200),
            msg: Binary::default(),
            contract_address: Addr::unchecked("contract1"),
            executors: vec![],
            execution_rebate: vec![],
            jitter: None,
        };

        TRIGGERS.save(storage, &trigger_1).unwrap();
        TRIGGERS.save(storage, &trigger_2).unwrap();

        assert_eq!(
            TRIGGERS
                .filtered(
                    storage,
                    ConditionFilter::BlockHeight {
                        start: None,
                        end: None,
                    },
                    None,
                )
                .unwrap(),
            vec![trigger_1.clone(), trigger_2.clone()]
        );

        assert_eq!(
            TRIGGERS
                .filtered(
                    storage,
                    ConditionFilter::BlockHeight {
                        start: Some(50),
                        end: None,
                    },
                    None,
                )
                .unwrap(),
            vec![trigger_1.clone(), trigger_2.clone()]
        );

        assert_eq!(
            TRIGGERS
                .filtered(
                    storage,
                    ConditionFilter::BlockHeight {
                        start: Some(150),
                        end: None,
                    },
                    None,
                )
                .unwrap(),
            vec![trigger_2.clone()]
        );

        assert_eq!(
            TRIGGERS
                .filtered(
                    storage,
                    ConditionFilter::BlockHeight {
                        start: None,
                        end: Some(150),
                    },
                    None,
                )
                .unwrap(),
            vec![trigger_1]
        );

        assert_eq!(
            TRIGGERS
                .filtered(
                    storage,
                    ConditionFilter::BlockHeight {
                        start: Some(250),
                        end: Some(350),
                    },
                    None,
                )
                .unwrap(),
            vec![]
        );
    }

    #[test]
    fn test_timestamp_filter() {
        let storage = &mut MockStorage::default();
        let owner = Addr::unchecked("creator");

        let trigger_1 = Trigger {
            id: Uint64::from(1u64),
            owner: owner.clone(),
            condition: Condition::TimestampElapsed(Timestamp::from_seconds(100)),
            msg: Binary::default(),
            contract_address: Addr::unchecked("contract1"),
            executors: vec![],
            execution_rebate: vec![],
            jitter: None,
        };

        let trigger_2 = Trigger {
            id: Uint64::from(2u64),
            owner,
            condition: Condition::TimestampElapsed(Timestamp::from_seconds(200)),
            msg: Binary::default(),
            contract_address: Addr::unchecked("contract1"),
            executors: vec![],
            execution_rebate: vec![],
            jitter: None,
        };

        TRIGGERS.save(storage, &trigger_1).unwrap();
        TRIGGERS.save(storage, &trigger_2).unwrap();

        assert_eq!(
            TRIGGERS
                .filtered(
                    storage,
                    ConditionFilter::Timestamp {
                        start: None,
                        end: None,
                    },
                    None,
                )
                .unwrap(),
            vec![trigger_1.clone(), trigger_2.clone()]
        );

        assert_eq!(
            TRIGGERS
                .filtered(
                    storage,
                    ConditionFilter::Timestamp {
                        start: Some(Timestamp::from_seconds(50)),
                        end: None,
                    },
                    None,
                )
                .unwrap(),
            vec![trigger_1.clone(), trigger_2.clone()]
        );

        assert_eq!(
            TRIGGERS
                .filtered(
                    storage,
                    ConditionFilter::Timestamp {
                        start: Some(Timestamp::from_seconds(150)),
                        end: None,
                    },
                    None,
                )
                .unwrap(),
            vec![trigger_2.clone()]
        );

        assert_eq!(
            TRIGGERS
                .filtered(
                    storage,
                    ConditionFilter::Timestamp {
                        start: None,
                        end: Some(Timestamp::from_seconds(150)),
                    },
                    None,
                )
                .unwrap(),
            vec![trigger_1]
        );

        assert_eq!(
            TRIGGERS
                .filtered(
                    storage,
                    ConditionFilter::Timestamp {
                        start: Some(Timestamp::from_seconds(250)),
                        end: Some(Timestamp::from_seconds(350)),
                    },
                    None,
                )
                .unwrap(),
            vec![]
        );
    }
}
