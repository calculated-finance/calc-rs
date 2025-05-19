use calc_rs::types::Status;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, HexBinary};

#[cw_serde]
pub struct Config {
    pub checksum: HexBinary,
    pub code_id: u64,
}

#[cw_serde]
pub struct StrategyHandle {
    pub owner: Addr,
    pub contract_address: Addr,
    pub status: Status,
    pub updated_at: u64,
}
