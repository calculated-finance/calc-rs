use std::collections::HashSet;

use calc_rs::{
    actions::action::Action,
    statistics::Statistics,
    strategy::{StrategyConfig, StrategyExecuteMsg},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, StdResult, Storage};
use cw_storage_plus::Item;

#[cw_serde]
pub struct StoredStrategy {
    pub manager: Addr,
    pub owner: Addr,
    pub escrowed: Vec<String>,
    pub behaviour: Action,
}

impl From<StrategyConfig> for StoredStrategy {
    fn from(strategy: StrategyConfig) -> Self {
        StoredStrategy {
            manager: strategy.manager,
            owner: strategy.owner,
            escrowed: strategy.escrowed.into_iter().collect(),
            behaviour: strategy.action,
        }
    }
}

impl From<StoredStrategy> for StrategyConfig {
    fn from(stored: StoredStrategy) -> Self {
        StrategyConfig {
            manager: stored.manager,
            owner: stored.owner,
            escrowed: HashSet::from_iter(stored.escrowed.into_iter()),
            action: stored.behaviour,
        }
    }
}

pub struct StrategyStore {
    config: Item<StoredStrategy>,
}

impl StrategyStore {
    pub fn save(&self, storage: &mut dyn Storage, update: StrategyConfig) -> StdResult<()> {
        self.config.save(storage, &update.into())
    }

    pub fn load(&self, storage: &dyn Storage) -> StdResult<StrategyConfig> {
        let stored_strategy = self.config.load(storage)?;
        Ok(stored_strategy.into())
    }
}

pub const STATE: Item<StrategyExecuteMsg> = Item::new("state");

pub const STRATEGY: StrategyStore = StrategyStore {
    config: Item::new("config"),
};

pub const STATS: Item<Statistics> = Item::new("stats");
