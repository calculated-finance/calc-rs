use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{CosmosMsg, WasmMsg};

use crate::types::{Strategy, StrategyStatus};

#[cw_serde]
pub enum ExecuteMsg {
    Create {},
    Execute {},
    Withdraw {},
    Deposit {},
    Pause {},
    Archive {},
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(Strategy)]
    Strategy { address: Address },
    #[returns(Strategy)]
    Strategies {
        owner: Option<Address>,
        status: Option<StrategyStatus>,
        start_after: Option<Address>,
        limit: Option<u32>,
    },
}
