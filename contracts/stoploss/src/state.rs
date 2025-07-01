use calc_rs::{
    manager::StrategyExecuteMsg,
    stoploss::{StopLossConfig, StopLossStatistics},
};
use cosmwasm_std::{StdResult, Storage};
use cw_storage_plus::Item;

pub struct ConfigStore {
    item: Item<StopLossConfig>,
}

impl ConfigStore {
    pub fn save(&self, storage: &mut dyn Storage, update: &StopLossConfig) -> StdResult<()> {
        self.item.save(storage, &update)
    }

    pub fn load(&self, storage: &dyn Storage) -> StdResult<StopLossConfig> {
        self.item.load(storage)
    }
}

pub const STATE: Item<StrategyExecuteMsg> = Item::new("state");

pub const CONFIG: ConfigStore = ConfigStore {
    item: Item::new("config"),
};

pub const STATS: Item<StopLossStatistics> = Item::new("stats");
