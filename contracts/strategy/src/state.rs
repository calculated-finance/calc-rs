use calc_rs::types::StrategyConfig;
use cosmwasm_std::Addr;
use cw_storage_plus::Item;

pub const MANAGER: Item<Addr> = Item::new("manager");

pub const FEE_COLLECTOR: Item<Addr> = Item::new("fee_collector");

pub const CONFIG: Item<StrategyConfig> = Item::new("config");
