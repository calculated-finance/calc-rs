use std::collections::HashSet;

use calc_rs::{
    conditions::{Condition, Conditions},
    scheduler::{ConditionFilter, CreateTrigger, Trigger},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Order, StdError, StdResult, Storage};
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, Item, MultiIndex, UniqueIndex};

pub const TRIGGER_COUNTER: Item<u64> = Item::new("trigger_counter");
pub const CONDITION_COUNTER: Item<u64> = Item::new("condition_counter");

#[cw_serde]
pub struct ConditionStore {
    id: u64,
    trigger_id: u64,
    condition: Condition,
}

pub struct ConditionIndexes<'a> {
    pub trigger_id: MultiIndex<'a, u64, ConditionStore, u64>,
    pub timestamp: MultiIndex<'a, u64, ConditionStore, u64>,
    pub block_height: MultiIndex<'a, u64, ConditionStore, u64>,
    pub limit_order: UniqueIndex<'a, u64, ConditionStore, u64>,
}

impl<'a> IndexList<ConditionStore> for ConditionIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<ConditionStore>> + '_> {
        let v: Vec<&dyn Index<ConditionStore>> = vec![
            &self.limit_order,
            &self.trigger_id,
            &self.timestamp,
            &self.block_height,
        ];
        Box::new(v.into_iter())
    }
}

pub const CONDITIONS: IndexedMap<u64, ConditionStore, ConditionIndexes<'static>> = IndexedMap::new(
    "conditions",
    ConditionIndexes {
        trigger_id: MultiIndex::new(|_, c| c.trigger_id, "conditions", "conditions__trigger_id"),
        timestamp: MultiIndex::new(
            |_, c| match c.condition {
                Condition::TimestampElapsed(timestamp) => timestamp.seconds(),
                _ => u64::MAX,
            },
            "conditions",
            "conditions__timestamp",
        ),
        block_height: MultiIndex::new(
            |_, c| match c.condition {
                Condition::BlocksCompleted(height) => height,
                _ => u64::MAX,
            },
            "conditions",
            "conditions__block_height",
        ),
        limit_order: UniqueIndex::new(|c| c.id, "conditions__limit_order"),
    },
);

pub struct TriggerIndexes<'a> {
    pub owner: MultiIndex<'a, Addr, Trigger, u64>,
}

impl<'a> IndexList<Trigger> for TriggerIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<Trigger>> + '_> {
        let v: Vec<&dyn Index<Trigger>> = vec![&self.owner];
        Box::new(v.into_iter())
    }
}

pub struct TriggerStore<'a> {
    triggers: IndexedMap<u64, Trigger, TriggerIndexes<'a>>,
}

fn save_condition(
    storage: &mut dyn Storage,
    trigger_id: u64,
    condition: &Condition,
) -> StdResult<()> {
    match condition {
        Condition::Compose(Conditions { conditions, .. }) => {
            for cond in conditions {
                save_condition(storage, trigger_id, cond)?;
            }

            Ok(())
        }
        _ => {
            let condition_id =
                CONDITION_COUNTER.update(storage, |id| Ok::<u64, StdError>(id + 1))?;

            CONDITIONS.save(
                storage,
                condition_id,
                &ConditionStore {
                    id: condition_id,
                    trigger_id,
                    condition: Condition::from(condition.clone()),
                },
            )
        }
    }
}

impl TriggerStore<'_> {
    pub fn save(
        &self,
        storage: &mut dyn Storage,
        owner: Addr,
        command: CreateTrigger,
        execution_rebate: Vec<Coin>,
    ) -> StdResult<()> {
        let trigger_id = TRIGGER_COUNTER.update(storage, |id| Ok::<u64, StdError>(id + 1))?;

        save_condition(storage, trigger_id, &command.condition)?;

        self.triggers.save(
            storage,
            trigger_id,
            &Trigger {
                id: trigger_id,
                owner: owner,
                condition: command.condition,
                threshold: command.threshold,
                msg: command.msg,
                to: command.to,
                execution_rebate: execution_rebate,
            },
        )
    }

    pub fn load(&self, storage: &dyn Storage, id: u64) -> StdResult<Trigger> {
        self.triggers.load(storage, id)
    }

    pub fn owner(
        &self,
        storage: &dyn Storage,
        owner: Addr,
        limit: Option<usize>,
        start_after: Option<u64>,
    ) -> Vec<Trigger> {
        self.triggers
            .idx
            .owner
            .prefix(owner)
            .range(
                storage,
                start_after.map(Bound::exclusive),
                None,
                Order::Ascending,
            )
            .take(limit.unwrap_or(30))
            .flat_map(|r| r.map(|(_, v)| v))
            .collect::<Vec<_>>()
    }

    pub fn filter(
        &self,
        storage: &dyn Storage,
        filter: ConditionFilter,
        limit: Option<usize>,
    ) -> StdResult<Vec<Trigger>> {
        let conditions = match filter {
            ConditionFilter::BlockHeight { start, end } => CONDITIONS
                .idx
                .block_height
                .range(
                    storage,
                    start.map(|s| Bound::inclusive((s, u64::MAX))),
                    end.map(|e| Bound::inclusive((e, u64::MAX))),
                    Order::Ascending,
                )
                .take(limit.unwrap_or(30))
                .flat_map(|r| r.map(|(_, v)| v))
                .collect::<Vec<_>>(),
            ConditionFilter::Timestamp { start, end } => CONDITIONS
                .idx
                .timestamp
                .range(
                    storage,
                    start.map(|s| Bound::inclusive((s.seconds(), u64::MAX))),
                    end.map(|e| Bound::inclusive((e.seconds(), u64::MAX))),
                    Order::Ascending,
                )
                .take(limit.unwrap_or(30))
                .flat_map(|r| r.map(|(_, v)| v))
                .collect::<Vec<_>>(),
            ConditionFilter::LimitOrder { start_after } => CONDITIONS
                .idx
                .limit_order
                .range(
                    storage,
                    start_after.map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .take(limit.unwrap_or(30))
                .flat_map(|r| r.map(|(_, v)| v))
                .collect::<Vec<_>>(),
        };

        let mut trigger_ids: HashSet<u64> = HashSet::new();
        let mut triggers: Vec<Trigger> = Vec::new();

        for condition in conditions {
            if trigger_ids.contains(&condition.trigger_id) {
                continue;
            }

            let trigger = TRIGGERS.load(storage, condition.trigger_id)?;
            trigger_ids.insert(condition.trigger_id);
            triggers.push(trigger);
        }

        Ok(triggers)
    }

    pub fn delete(&self, storage: &mut dyn Storage, id: u64) -> StdResult<()> {
        let conditions_to_remove = CONDITIONS
            .idx
            .trigger_id
            .prefix(id)
            .keys(storage, None, None, Order::Ascending)
            .collect::<StdResult<Vec<_>>>()?;

        for condition_id in conditions_to_remove {
            CONDITIONS.remove(storage, condition_id)?;
        }

        self.triggers.remove(storage, id)
    }
}

pub const TRIGGERS: TriggerStore<'static> = TriggerStore {
    triggers: IndexedMap::new(
        "triggers",
        TriggerIndexes {
            owner: MultiIndex::new(|_, t| t.owner.clone(), "triggers", "triggers__owner"),
        },
    ),
};
