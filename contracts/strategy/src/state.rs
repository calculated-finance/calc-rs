use calc_rs::types::StrategyConfig;
use cosmwasm_std::Addr;
use cw_storage_plus::Item;

pub const MANAGER: Item<Addr> = Item::new("manager");

pub const CONFIG: Item<StrategyConfig> = Item::new("config");
