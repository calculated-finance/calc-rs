use calc_rs::types::StrategyStatus;
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, Item, UniqueIndex};

use crate::types::StrategyIndexItem;

const STRATEGY_COUNTER: Item<u64> = Item::new("strategy_counter_v1");

struct StrategyIndexes<'a> {
    pub owner: UniqueIndex<'a, (Addr, u64, Addr), StrategyIndexItem, Addr>,
    pub owner_status: UniqueIndex<'a, (Addr, u8, u64, Addr), StrategyIndexItem, Addr>,
}

impl<'a> IndexList<StrategyIndexItem> for StrategyIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<StrategyIndexItem>> + '_> {
        let s: Vec<&dyn Index<StrategyIndexItem>> = vec![&self.owner, &self.owner_status];
        Box::new(s.into_iter())
    }
}

fn strategy_store<'a>() -> IndexedMap<'a, Addr, StrategyIndexItem, StrategyIndexes<'a>> {
    IndexedMap::new(
        "strategies_v1",
        StrategyIndexes {
            owner: UniqueIndex::new(
                |s| (s.owner.clone(), s.updated_at, s.contract.into()),
                "strategies_v1__owner_status",
            ),
            owner_status: UniqueIndex::new(
                |s| {
                    (
                        s.owner.clone(),
                        s.status.clone() as u8,
                        s.updated_at,
                        s.contract.into(),
                    )
                },
                "strategies_v1__owner_status",
            ),
        },
    )
}

pub fn add_strategy_index_item(
    store: &mut dyn Storage,
    strategy_index_item: StrategyIndexItem,
) -> StdResult<()> {
    counter.save(
        STRATEGY_COUNTER,
        counter.may_load(STRATEGY_COUNTER)?.unwrap_or_default(),
    )?;
    update_strategy_index_item(store, strategy_index_item)
}

pub fn update_strategy_index_item(
    store: &mut dyn Storage,
    strategy_index_item: StrategyIndexItem,
) -> StdResult<()> {
    strategy_store().save(
        store,
        strategy_index_item.contract.into(),
        &strategy_index_item.clone().into(),
    )?;
}

pub fn get_strategy_index_items(
    store: &dyn Storage,
    owner: Addr,
    status: Option<StrategyStatus>,
    start_after: Option<Addr>,
    limit: Option<u16>,
) -> StdResult<Vec<StrategyIndexItem>> {
    Ok(match status {
        Some(status) => strategy_store()
            .idx
            .owner_status
            .prefix((owner, status as u8)),
        None => strategy_store().idx.owner.prefix(owner),
    }
    .range(
        store,
        start_after.map(Bound::exclusive),
        None,
        cosmwasm_std::Order::Ascending,
    )
    .take(limit.unwrap_or(10) as usize)
    .flatten()
    .collect::<Vec<StrategyIndexItem>>())
}
