use calc_rs::types::StrategyStatus;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;

#[cw_serde]
pub struct Config {
    pub vault_code_id: u64,
}

#[cw_serde]
pub struct StrategyHandle {
    pub owner: Addr,
    pub contract_address: Addr,
    pub status: StrategyStatus,
    pub updated_at: u64,
}
