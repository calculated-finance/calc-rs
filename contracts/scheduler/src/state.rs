use calc_rs::types::{Condition, ConditionFilter, Trigger};
use cosmwasm_std::{Addr, Binary, Coin, Deps, Order, StdError, StdResult, Storage, Uint64};
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, Item, MultiIndex};

pub const TRIGGER_COUNTER: Item<u64> = Item::new("condition_counter");

pub struct TriggerIndexes<'a> {
    pub owner: MultiIndex<'a, Addr, Trigger, u64>,
    pub timestamp: MultiIndex<'a, u64, Trigger, u64>,
    pub block_height: MultiIndex<'a, u64, Trigger, u64>,
    pub limit_order_id: MultiIndex<'a, u64, Trigger, u64>,
}

impl<'a> IndexList<Trigger> for TriggerIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<Trigger>> + '_> {
        let v: Vec<&dyn Index<Trigger>> = vec![
            &self.owner,
            &self.timestamp,
            &self.block_height,
            &self.limit_order_id,
        ];
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
            limit_order_id: MultiIndex::new(
                |_, t| match &t.condition {
                    Condition::LimitOrder { .. } => t.id,
                    _ => u64::MAX,
                },
                "triggers",
                "triggers__limit_order_id",
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
        ConditionFilter::LimitOrder {} => {
            triggers()
                .idx
                .limit_order_id
                .range(deps.storage, None, None, Order::Ascending)
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
    .collect::<Vec<Trigger>>()
}

pub fn delete_trigger(storage: &mut dyn Storage, id: u64) -> StdResult<()> {
    triggers().remove(storage, id)
}
