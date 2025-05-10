use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Addr;

use crate::types::{Strategy, StrategyConfig, StrategyStatus};

#[cw_serde]
pub struct FactoryInstantiateMsg {
    pub vault_code_id: u64,
}

#[cw_serde]
pub enum FactoryExecuteMsg {
    CreateStrategy {
        label: String,
        config: StrategyConfig,
    },
    CreateHandle {
        owner: Addr,
        status: StrategyStatus,
    },
    UpdateHandle {
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
pub struct StrategyInstantiateMsg {
    pub config: StrategyConfig,
}

#[cw_serde]
pub enum StrategyExecuteMsg {
    Execute {},
    Withdraw { assets: Vec<String> },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum StrategyQueryMsg {
    #[returns(StrategyConfig)]
    Config {},
    #[returns(bool)]
    CanExecute {},
}
