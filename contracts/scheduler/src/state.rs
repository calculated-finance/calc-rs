use calc_rs::{
    conditions::Condition,
    scheduler::{ConditionFilter, Trigger},
};
use cosmwasm_std::{Addr, Decimal, Order, StdResult, Storage, Uint64};
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, Item, MultiIndex};

pub const MANAGER: Item<Addr> = Item::new("manager");

pub struct TriggerIndexes<'a> {
    pub timestamp: MultiIndex<'a, u64, Trigger, u64>,
    pub block_height: MultiIndex<'a, u64, Trigger, u64>,
    pub limit_order_pair: MultiIndex<'a, Addr, Trigger, u64>,
    pub limit_order_pair_price: MultiIndex<'a, (Addr, String), Trigger, u64>,
}

impl<'a> IndexList<Trigger> for TriggerIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<Trigger>> + '_> {
        let v: Vec<&dyn Index<Trigger>> = vec![
            &self.timestamp,
            &self.block_height,
            &self.limit_order_pair,
            &self.limit_order_pair_price,
        ];
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
                start.map(|s| Bound::inclusive((s, u64::MAX))),
                end.map(|e| Bound::inclusive((e, u64::MAX))),
                Order::Ascending,
            ),
            ConditionFilter::Timestamp { start, end } => self.triggers.idx.timestamp.range(
                storage,
                start.map(|s| Bound::inclusive((s.seconds(), u64::MAX))),
                end.map(|e| Bound::inclusive((e.seconds(), u64::MAX))),
                Order::Ascending,
            ),
            ConditionFilter::LimitOrder {
                pair_address,
                price_range,
                start_after,
            } => match price_range {
                None => self
                    .triggers
                    .idx
                    .limit_order_pair
                    .prefix(pair_address)
                    .range(
                        storage,
                        start_after.map(Bound::exclusive),
                        None,
                        Order::Ascending,
                    ),
                Some((above, below)) => self
                    .triggers
                    .idx
                    .limit_order_pair_price
                    .sub_prefix(pair_address)
                    .range(
                        storage,
                        Some(Bound::exclusive((
                            above.to_string(),
                            start_after.unwrap_or(0),
                        ))),
                        Some(Bound::exclusive((below.to_string(), u64::MAX))),
                        Order::Ascending,
                    ),
            },
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
        "triggers",
        TriggerIndexes {
            timestamp: MultiIndex::new(
                |_, t| match t.condition {
                    Condition::TimestampElapsed(timestamp) => timestamp.seconds(),
                    _ => u64::MAX,
                },
                "triggers",
                "triggers__timestamp",
            ),
            block_height: MultiIndex::new(
                |_, t| match t.condition {
                    Condition::BlocksCompleted(height) => height,
                    _ => u64::MAX,
                },
                "triggers",
                "triggers__block_height",
            ),
            limit_order_pair: MultiIndex::new(
                |_, t| match t.condition.clone() {
                    Condition::LimitOrderFilled { pair_address, .. } => pair_address,
                    _ => Addr::unchecked(""),
                },
                "triggers",
                "triggers__limit_order_pair",
            ),
            limit_order_pair_price: MultiIndex::new(
                |_, t| match t.condition.clone() {
                    Condition::LimitOrderFilled {
                        pair_address,
                        price,
                        ..
                    } => (pair_address, price.to_string()),
                    _ => (Addr::unchecked(""), Decimal::zero().to_string()),
                },
                "triggers",
                "triggers__limit_order_pair_price",
            ),
        },
    ),
};
