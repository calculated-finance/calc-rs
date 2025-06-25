use calc_rs::types::AccumulateStrategyConfig;
use cw_storage_plus::Item;

pub const CONFIG: Item<AccumulateStrategyConfig> = Item::new("config");
