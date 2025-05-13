use calc_rs::types::StrategyConfig;
use cosmwasm_std::{StdResult, Storage};
use cw_storage_plus::Item;

const CONFIG: Item<StrategyConfig> = Item::new("config");

pub fn get_config(store: &dyn Storage) -> StdResult<StrategyConfig> {
    CONFIG.load(store)
}

pub fn update_config(store: &mut dyn Storage, config: StrategyConfig) -> StdResult<StrategyConfig> {
    CONFIG.save(store, &config)?;
    Ok(config)
}
