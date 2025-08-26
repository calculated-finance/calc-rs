use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Coin};
use rujira_rs::TokenMetadata;

use crate::{conditions::asset_value_ratio::PriceSource, manager::Affiliate, strategy::Node};

#[cw_serde]
pub struct TokenizerInstantiateMsg {
    pub label: String,
    pub quote_denom: String,
    pub oracles: Vec<(String, PriceSource)>,
    pub nodes: Vec<Node>,
    pub affiliates: Vec<Affiliate>,
    pub manager_address: Addr,
    pub token_metadata: TokenMetadata,
}

#[cw_serde]
pub struct TokenizerConfig {
    pub denom: String,
    pub quote_denom: String,
    pub oracles: Vec<(String, PriceSource)>,
    pub strategy_address: Addr,
    pub description: String,
}

#[cw_serde]
pub enum TokenizerExecuteMsg {
    Deposit {
        recipient: Option<Addr>,
    },
    Withdraw {},
    Mint {
        recipient: Addr,
        previous_value: Coin,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum TokenizerQueryMsg {
    #[returns(TokenizerConfig)]
    Config {},
    #[returns(Coin)]
    Value {},
}
