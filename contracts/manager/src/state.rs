use calc_rs::manager::{Affiliate, ManagerConfig, Strategy};
use cosmwasm_std::{Addr, StdError, StdResult};
use cw_storage_plus::{Index, IndexList, IndexedMap, Item, Map, MultiIndex};

pub struct ConfigStore {
    item: Item<ManagerConfig>,
}

impl ConfigStore {
    pub fn save(
        &self,
        storage: &mut dyn cosmwasm_std::Storage,
        msg: &ManagerConfig,
    ) -> StdResult<()> {
        if msg.code_ids.is_empty() {
            return Err(StdError::generic_err(
                "At least one code ID must be provided",
            ));
        }

        if msg.affiliate_creation_fee.amount.is_zero() {
            return Err(StdError::generic_err(
                "Affiliate creation fee must be greater than zero",
            ));
        }

        if msg.default_affiliate_bps > 7 {
            return Err(StdError::generic_err(
                "Default affiliate basis points cannot exceed 7 (0.07%)",
            ));
        }

        self.item.save(storage, &msg)
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
