use calc_rs::manager::{ManagerConfig, StrategyHandle};
use cosmwasm_std::{Addr, StdResult};
use cw_storage_plus::{Index, IndexList, IndexedMap, Item, UniqueIndex};

pub struct ConfigStore {
    item: Item<ManagerConfig>,
}

impl ConfigStore {
    pub fn save(
        &self,
        storage: &mut dyn cosmwasm_std::Storage,
        msg: &ManagerConfig,
    ) -> StdResult<()> {
        self.item.save(storage, msg)
    }

    pub fn load(&self, storage: &dyn cosmwasm_std::Storage) -> StdResult<ManagerConfig> {
        self.item.load(storage)
    }
}

pub const CONFIG: ConfigStore = ConfigStore {
    item: Item::new("config"),
};

pub const STRATEGY_COUNTER: Item<u64> = Item::new("strategy_counter");

pub struct StrategyIndexes<'a> {
    pub updated_at: UniqueIndex<'a, String, StrategyHandle, Addr>,
    pub owner_updated_at: UniqueIndex<'a, (Addr, String), StrategyHandle, Addr>,
    pub status_updated_at: UniqueIndex<'a, (u8, String), StrategyHandle, Addr>,
    pub owner_status_updated_at: UniqueIndex<'a, (Addr, u8, String), StrategyHandle, Addr>,
}

impl<'a> IndexList<StrategyHandle> for StrategyIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<StrategyHandle>> + '_> {
        let s: Vec<&dyn Index<StrategyHandle>> = vec![
            &self.updated_at,
            &self.owner_updated_at,
            &self.status_updated_at,
            &self.owner_status_updated_at,
        ];
        Box::new(s.into_iter())
    }
}

pub fn updated_at_cursor(updated_at: u64, contract_address: Option<&Addr>) -> String {
    match contract_address {
        Some(addr) => format!("{updated_at:030}_{addr}"),
        None => format!("{updated_at:030}_"),
    }
}

pub fn strategy_store<'a>() -> IndexedMap<Addr, StrategyHandle, StrategyIndexes<'a>> {
    IndexedMap::new(
        "strategies",
        StrategyIndexes {
            updated_at: UniqueIndex::new(
                |s| updated_at_cursor(s.updated_at, Some(&s.contract_address)),
                "strategies_updated_at",
            ),
            owner_updated_at: UniqueIndex::new(
                |s| {
                    (
                        s.owner.clone(),
                        updated_at_cursor(s.updated_at, Some(&s.contract_address)),
                    )
                },
                "strategies_owner_updated_at",
            ),
            status_updated_at: UniqueIndex::new(
                |s| {
                    (
                        s.status.clone() as u8,
                        updated_at_cursor(s.updated_at, Some(&s.contract_address)),
                    )
                },
                "strategies_status_updated_at",
            ),
            owner_status_updated_at: UniqueIndex::new(
                |s| {
                    (
                        s.owner.clone(),
                        s.status.clone() as u8,
                        updated_at_cursor(s.updated_at, Some(&s.contract_address)),
                    )
                },
                "strategies_owner_status_updated_at",
            ),
        },
    )
}
