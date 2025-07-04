use calc_rs::{
    statistics::Statistics,
    strategy::{Strategy2, StrategyExecuteMsg},
};
use cosmwasm_std::{StdResult, Storage};
use cw_storage_plus::Item;

pub struct StrategyStore {
    config: Item<Strategy2>,
}

impl StrategyStore {
    pub fn save(&self, storage: &mut dyn Storage, update: &Strategy2) -> StdResult<()> {
        self.config.save(storage, update)
    }

    pub fn load(&self, storage: &dyn Storage) -> StdResult<Strategy2> {
        self.config.load(storage)
    }
}

pub const STATE: Item<StrategyExecuteMsg> = Item::new("state");

pub const STRATEGY: StrategyStore = StrategyStore {
    config: Item::new("config"),
};

pub const STATS: Item<Statistics> = Item::new("stats");
