use calc_rs::types::{Affiliate, ManagerConfig, Status, Strategy};
use cosmwasm_std::Addr;
use cw_storage_plus::{Index, IndexList, IndexedMap, Item, Map, UniqueIndex};

pub const CONFIG: Item<ManagerConfig> = Item::new("config");

pub const STRATEGY_COUNTER: Item<u64> = Item::new("strategy_counter");

pub struct Strategies<'a> {
    pub owner_updated_at: UniqueIndex<'a, (Addr, u64, Addr), Strategy, Addr>,
    pub owner_status: UniqueIndex<'a, (Addr, u8, Addr), Strategy, Addr>,
    pub updated_at: UniqueIndex<'a, (u64, Addr), Strategy, Addr>,
    pub status_updated_at: UniqueIndex<'a, (Status, u64, Addr), Strategy, Addr>,
}

impl<'a> IndexList<Strategy> for Strategies<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<Strategy>> + '_> {
        let s: Vec<&dyn Index<Strategy>> = vec![
            &self.owner_updated_at,
            &self.owner_status,
            &self.updated_at,
            &self.status_updated_at,
        ];
        Box::new(s.into_iter())
    }
}

pub fn strategy_store<'a>() -> IndexedMap<Addr, Strategy, Strategies<'a>> {
    IndexedMap::new(
        "strategies_v1",
        Strategies {
            owner_updated_at: UniqueIndex::new(
                |s| (s.owner.clone(), s.updated_at, s.contract_address.clone()),
                "strategies_v1__owner_updated_at",
            ),
            owner_status: UniqueIndex::new(
                |s| {
                    (
                        s.owner.clone(),
                        s.status.clone() as u8,
                        s.contract_address.clone(),
                    )
                },
                "strategies_v1__owner_status",
            ),
            updated_at: UniqueIndex::new(
                |s| (s.updated_at, s.contract_address.clone()),
                "strategies_v1__updated_at",
            ),
            status_updated_at: UniqueIndex::new(
                |s| (s.status.clone(), s.updated_at, s.contract_address.clone()),
                "strategies_v1__status_updated_at",
            ),
        },
    )
}

pub const AFFILIATES: Map<String, Affiliate> = Map::new("affiliates");
