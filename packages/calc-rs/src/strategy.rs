use std::{collections::HashSet, u8, vec};

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Coin};

use crate::{
    actions::action::Action,
    manager::{Affiliate, StrategyStatus},
    statistics::Statistics,
};

#[cw_serde]
pub struct StrategyConfig {
    pub manager: Addr,
    pub owner: Addr,
    pub escrowed: HashSet<String>,
    pub action: Action,
}

#[cw_serde]
pub struct StrategyInstantiateMsg {
    pub owner: Addr,
    pub affiliates: Vec<Affiliate>,
    pub action: Action,
}

#[cw_serde]
pub enum StrategyExecuteMsg {
    Execute {},
    Withdraw(Vec<Coin>),
    Update(Action),
    UpdateStatus(StrategyStatus),
    Clear {},
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum StrategyQueryMsg {
    #[returns(StrategyConfig)]
    Config {},
    #[returns(Statistics)]
    Statistics {},
    #[returns(Vec<Coin>)]
    Balances { include: Vec<String> },
}
