use calc_rs::{core::StrategyConfig, ladder::LadderStatistics, manager::StrategyExecuteMsg};
use cosmwasm_std::{StdResult, Storage};
use cw_storage_plus::Item;

pub struct ConfigStore {
    item: Item<StrategyConfig>,
}

impl ConfigStore {
    pub fn save(&self, storage: &mut dyn Storage, update: &StrategyConfig) -> StdResult<()> {
        self.item.save(storage, &update)
    }

    pub fn load(&self, storage: &dyn Storage) -> StdResult<StrategyConfig> {
        self.item.load(storage)
    }
}

pub const STATE: Item<StrategyExecuteMsg> = Item::new("state");

pub const CONFIG: ConfigStore = ConfigStore {
    item: Item::new("config"),
};

pub const STATS: Item<LadderStatistics> = Item::new("stats");
