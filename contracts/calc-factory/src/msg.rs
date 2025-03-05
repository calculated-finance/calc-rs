use calc_rs::types::StrategyConfig;
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, CosmosMsg, WasmMsg};

use crate::types::{Strategy, StrategyStatus};

#[cw_serde]
pub struct InstantiateMsg {}

#[cw_serde]
pub enum ExecuteMsg {
    Create { config: StrategyConfig },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Vec<Strategy>)]
    Strategies {
        owner: Addr,
        status: Option<StrategyStatus>,
        start_after: Option<Addr>,
        limit: Option<u16>,
    },
    #[returns(Vec<Addr>)]
    ExecutableStrategies {
        start_after: Option<Addr>,
        limit: Option<u16>,
    },
}
