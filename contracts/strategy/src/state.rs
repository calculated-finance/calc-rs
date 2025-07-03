use calc_rs::{
    core::{Statistics, StrategyConfig},
    ladder::LadderStatistics,
    manager::StrategyExecuteMsg,
};
use cosmwasm_std::{StdResult, Storage};
use cw_storage_plus::Item;

pub struct StrategyStore {
    config: Item<StrategyConfig>,
}

impl StrategyStore {
    pub fn save(&self, storage: &mut dyn Storage, update: &StrategyConfig) -> StdResult<()> {
        self.config.save(storage, &update)
    }

    pub fn load(&self, storage: &dyn Storage) -> StdResult<StrategyConfig> {
        self.config.load(storage)
    }
}

pub const STATE: Item<StrategyExecuteMsg> = Item::new("state");

pub const STRATEGY: StrategyStore = StrategyStore {
    config: Item::new("config"),
};

pub const STATS: Item<Statistics> = Item::new("stats");
