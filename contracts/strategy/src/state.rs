use calc_rs::types::StrategyConfig;
use cosmwasm_std::Addr;
use cw_storage_plus::Item;

pub const FACTORY: Item<Addr> = Item::new("factory");

pub const CONFIG: Item<StrategyConfig> = Item::new("config");
