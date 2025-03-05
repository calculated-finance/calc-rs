use calc_rs::types::StrategyConfig;
use cosmwasm_schema::{cw_serde, QueryResponses};

use crate::types::VaultConfig;

#[cw_serde]
pub struct InstantiateMsg {
    pub config: StrategyConfig,
}

#[cw_serde]
pub enum ExecuteMsg {}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(VaultConfig)]
    Config {},
}
