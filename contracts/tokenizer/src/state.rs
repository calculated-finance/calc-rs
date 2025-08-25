use calc_rs::conditions::asset_value_ratio::PriceSource;
use cosmwasm_std::Addr;
use cw_storage_plus::Item;

pub const DENOM: Item<String> = Item::new("denom");
pub const BASE_DENOM: Item<String> = Item::new("base_denom");
pub const STRATEGY: Item<Addr> = Item::new("strategy");
pub const ORACLES: Item<Vec<(String, PriceSource)>> = Item::new("oracles");
pub const DESCRIPTION: Item<String> = Item::new("description");
