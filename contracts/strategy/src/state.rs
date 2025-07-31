use std::collections::HashSet;

use calc_rs::{
    statistics::Statistics,
    strategy::{Active, Committed, Strategy, StrategyConfig, StrategyExecuteMsg},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, StdError, StdResult, Storage};
use cw_storage_plus::Item;

pub const DENOMS: Item<HashSet<String>> = Item::new("denoms");
pub const ESCROWED: Item<HashSet<String>> = Item::new("escrowed");
pub const STATE: Item<StrategyExecuteMsg> = Item::new("state");
pub const STATS: Item<Statistics> = Item::new("stats");

pub const ACTIVE_STRATEGY: Item<Strategy<Active>> = Item::new("active_strategy");

pub const CONFIG: StrategyStore = StrategyStore {
    store: Item::new("config"),
};

#[cw_serde]
pub struct StoredStrategy {
    pub manager: Addr,
    pub strategy: Strategy<Committed>,
}

pub struct StrategyStore {
    store: Item<StoredStrategy>,
}

impl StrategyStore {
    pub fn init(&self, storage: &mut dyn Storage, config: StrategyConfig) -> StdResult<()> {
        DENOMS.save(storage, &config.denoms)?;
        ESCROWED.save(storage, &config.escrowed)?;
        STATS.save(storage, &Statistics::default())?;

        self.store.save(
            storage,
            &StoredStrategy {
                manager: config.manager,
                strategy: config.strategy,
            },
        )
    }

    pub fn save(&self, storage: &mut dyn Storage, update: Strategy<Committed>) -> StdResult<()> {
        self.store.update(storage, |config| {
            Ok::<StoredStrategy, StdError>(StoredStrategy {
                manager: config.manager,
                strategy: update,
            })
        })?;

        Ok(())
    }

    pub fn load(&self, storage: &dyn Storage) -> StdResult<StrategyConfig> {
        let stored_strategy = self.store.load(storage)?;
        Ok(StrategyConfig {
            manager: stored_strategy.manager,
            strategy: stored_strategy.strategy,
            denoms: DENOMS.load(storage)?,
            escrowed: ESCROWED.load(storage)?,
        })
    }
}
