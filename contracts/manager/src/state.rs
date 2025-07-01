use calc_rs::manager::{Affiliate, ManagerConfig, Strategy};
use cosmwasm_std::{Addr, StdError, StdResult};
use cw_storage_plus::{Index, IndexList, IndexedMap, Item, Map, UniqueIndex};

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
    pub updated_at: UniqueIndex<'a, String, Strategy, Addr>,
    pub owner_updated_at: UniqueIndex<'a, (Addr, String), Strategy, Addr>,
    pub status_updated_at: UniqueIndex<'a, (u8, String), Strategy, Addr>,
    pub owner_status_updated_at: UniqueIndex<'a, (Addr, u8, String), Strategy, Addr>,
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

pub fn updated_at_cursor(updated_at: u64, contract_address: Option<Addr>) -> String {
    format!(
        "{:030}_{}",
        updated_at,
        contract_address.unwrap_or(Addr::unchecked(""))
    )
}

pub fn strategy_store<'a>() -> IndexedMap<Addr, Strategy, StrategyIndexes<'a>> {
    IndexedMap::new(
        "strategies",
        StrategyIndexes {
            updated_at: UniqueIndex::new(
                |s| updated_at_cursor(s.updated_at, Some(s.contract_address.clone())),
                "strategies_updated_at",
            ),
            owner_updated_at: UniqueIndex::new(
                |s| {
                    (
                        s.owner.clone(),
                        updated_at_cursor(s.updated_at, Some(s.contract_address.clone())),
                    )
                },
                "strategies_owner_updated_at",
            ),
            status_updated_at: UniqueIndex::new(
                |s| {
                    (
                        s.status.clone() as u8,
                        updated_at_cursor(s.updated_at, Some(s.contract_address.clone())),
                    )
                },
                "strategies_status_updated_at",
            ),
            owner_status_updated_at: UniqueIndex::new(
                |s| {
                    (
                        s.owner.clone(),
                        s.status.clone() as u8,
                        updated_at_cursor(s.updated_at, Some(s.contract_address.clone())),
                    )
                },
                "strategies_owner_status_updated_at",
            ),
        },
    )
}

pub const AFFILIATES: Map<String, Affiliate> = Map::new("affiliates");

#[cfg(test)]
mod config_store_tests {
    use calc_rs::manager::StrategyType;
    use cosmwasm_std::{testing::mock_dependencies, Coin};

    use super::*;

    #[test]
    fn must_provide_at_least_one_code_id() {
        let mut deps = mock_dependencies();

        let config_store = ConfigStore {
            item: Item::new("config"),
        };

        let config = ManagerConfig {
            code_ids: vec![],
            affiliate_creation_fee: Default::default(),
            default_affiliate_bps: 0,
            admin: Addr::unchecked("admin"),
            fee_collector: Addr::unchecked("collector"),
        };

        let result = config_store
            .save(deps.as_mut().storage, &config)
            .unwrap_err();

        assert_eq!(
            result,
            StdError::generic_err("At least one code ID must be provided")
        );
    }

    #[test]
    fn must_provide_affiliate_creation_fee() {
        let mut deps = mock_dependencies();

        let config_store = ConfigStore {
            item: Item::new("config"),
        };

        let config = ManagerConfig {
            code_ids: vec![(StrategyType::Twap, 1)],
            affiliate_creation_fee: Coin::new(0u128, "rune"),
            default_affiliate_bps: 0,
            admin: Addr::unchecked("admin"),
            fee_collector: Addr::unchecked("collector"),
        };

        let result = config_store
            .save(deps.as_mut().storage, &config)
            .unwrap_err();

        assert_eq!(
            result,
            StdError::generic_err("Affiliate creation fee must be greater than zero")
        );
    }

    #[test]
    fn must_provide_default_affiliate_bps_under_8() {
        let mut deps = mock_dependencies();

        let config_store = ConfigStore {
            item: Item::new("config"),
        };

        let config = ManagerConfig {
            code_ids: vec![(StrategyType::Twap, 1)],
            affiliate_creation_fee: Coin::new(100u128, "rune"),
            default_affiliate_bps: 8,
            admin: Addr::unchecked("admin"),
            fee_collector: Addr::unchecked("collector"),
        };

        let result = config_store
            .save(deps.as_mut().storage, &config)
            .unwrap_err();

        assert_eq!(
            result,
            StdError::generic_err("Default affiliate basis points cannot exceed 7 (0.07%)")
        );
    }
}
