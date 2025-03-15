use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Addr;

use crate::types::{Strategy, StrategyConfig, StrategyStatus};

#[cw_serde]
pub struct FactoryInstantiateMsg {
    pub vault_code_id: u64,
}

#[cw_serde]
pub enum FactoryExecuteMsg {
    Create {
        label: String,
        config: StrategyConfig,
    },
    CreateIndex {
        owner: Addr,
        status: StrategyStatus,
    },
    UpdateIndex {
        status: Option<StrategyStatus>,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum FactoryQueryMsg {
    #[returns(Strategy)]
    Strategy { address: Addr },
    #[returns(Vec<Strategy>)]
    Strategies {
        owner: Option<Addr>,
        status: Option<StrategyStatus>,
        start_after: Option<Addr>,
        limit: Option<u32>,
    },
}

#[cw_serde]
pub struct VaultInstantiateMsg {
    pub config: StrategyConfig,
}

#[cw_serde]
pub enum VaultExecuteMsg {
    Execute {},
    Withdraw {},
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum VaultQueryMsg {
    #[returns(StrategyConfig)]
    Config {},
}
