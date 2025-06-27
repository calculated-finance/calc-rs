use calc_rs::types::{Affiliate, ManagerConfig, Strategy, StrategyType};
use cosmwasm_std::Addr;
use cw_storage_plus::{Index, IndexList, IndexedMap, Item, Map, MultiIndex};

pub const CONFIG: Item<ManagerConfig> = Item::new("config");

pub const CODE_IDS: Map<StrategyType, u64> = Map::new("code_ids");

pub const STRATEGY_COUNTER: Item<u64> = Item::new("strategy_counter");

pub struct StrategyIndexes<'a> {
    pub updated_at: MultiIndex<'a, u64, Strategy, Addr>,
    pub owner_updated_at: MultiIndex<'a, (Addr, u64), Strategy, Addr>,
    pub status_updated_at: MultiIndex<'a, (u8, u64), Strategy, Addr>,
    pub owner_status_updated_at: MultiIndex<'a, (Addr, u8, u64), Strategy, Addr>,
}

impl<'a> IndexList<Strategy> for StrategyIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<Strategy>> + '_> {
        let s: Vec<&dyn Index<Strategy>> = vec![
            &self.updated_at,
            &self.owner_updated_at,
            &self.status_updated_at,
            &self.owner_status_updated_at,
        ];
        Box::new(s.into_iter())
    }
}

pub fn strategy_store<'a>() -> IndexedMap<Addr, Strategy, StrategyIndexes<'a>> {
    IndexedMap::new(
        "strategies",
        StrategyIndexes {
            updated_at: MultiIndex::new(
                |_, s| (s.updated_at),
                "strategies",
                "strategies_updated_at",
            ),
            owner_updated_at: MultiIndex::new(
                |_, s| (s.owner.clone(), s.updated_at),
                "strategies",
                "strategies_owner_updated_at",
            ),
            status_updated_at: MultiIndex::new(
                |_, s| (s.status.clone() as u8, s.updated_at),
                "strategies",
                "strategies_status_updated_at",
            ),
            owner_status_updated_at: MultiIndex::new(
                |_, s| (s.owner.clone(), s.status.clone() as u8, s.updated_at),
                "strategies",
                "strategies_owner_status_updated_at",
            ),
        },
    )
}

pub const AFFILIATES: Map<String, Affiliate> = Map::new("affiliates");
