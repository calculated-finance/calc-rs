use calc_rs::types::{Condition, Trigger};
use cosmwasm_std::{Addr, StdError, Uint64};
use cw_storage_plus::{Index, IndexList, IndexedMap, Item, MultiIndex};
use rujira_rs::CallbackData;

const TRIGGER_COUNTER: Item<u64> = Item::new("condition_counter");

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
    storage: &mut dyn cosmwasm_std::Storage,
    owner: Addr,
    condition: Condition,
    callback: CallbackData,
    to: Addr,
) -> cosmwasm_std::StdResult<()> {
    let id = TRIGGER_COUNTER.update(storage, |id| Ok::<u64, StdError>(id + 1))?;
    triggers().save(
        storage,
        id,
        &Trigger {
            id,
            owner,
            condition,
            callback,
            to,
        },
    )
}
