use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary};

#[cw_serde]
pub struct ExternalNode {
    pub contract_address: Addr,
    pub config: Binary,
}

#[cw_serde]
pub enum NodeExecuteMsg {
    Register {
        strategy_revision: u64,
        node_index: u16,
        config: Binary,
    },
    Execute {
        strategy_revision: u64,
        node_index: u16,
    },
    Commit {
        strategy_revision: u64,
        node_index: u16,
    },
    Cancel {
        strategy_revision: u64,
        node_index: u16,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum NodeQueryMsg {
    #[returns(NodeDetailsResponse)]
    Details {
        strategy: Addr,
        strategy_revision: u64,
        node_index: u16,
    },
    #[returns(bool)]
    IsSatisfied {
        strategy: Addr,
        strategy_revision: u64,
        node_index: u16,
    },
    #[returns(Vec<cosmwasm_std::Coin>)]
    Balances {
        strategy: Addr,
        strategy_revision: u64,
        node_index: u16,
    },
}

#[cw_serde]
pub struct NodeDetailsResponse {
    pub config: Binary,
}
