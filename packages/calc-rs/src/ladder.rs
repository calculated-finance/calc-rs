use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Decimal, Uint128};

use crate::distributor::Destination;

#[cw_serde]
pub enum Direction {
    Above,
    Below,
}

#[cw_serde]
pub enum Offset {
    Percentage(Decimal),
    Absolute(Decimal),
}

#[cw_serde]
pub struct LadderOrder {
    pub offset: Offset,
    pub direction: Direction,
    pub pair_address: Addr,
    pub swap_denom: String,
    pub target_denom: String,
    pub amount: Option<Uint128>,
}

#[cw_serde]
pub struct InstantiateLadderCommand {
    pub owner: Addr,
    pub manager_contract: Addr,
    pub scheduler_contract: Addr,
    pub distributor_code_id: u64,
    pub execution_rebate: Option<Coin>,
    pub affiliate_code: Option<String>,
    pub minimum_distribute_amount: Option<Coin>,
    pub mutable_destinations: Vec<Destination>,
    pub immutable_destinations: Vec<Destination>,
    pub orders: Vec<LadderOrder>,
}

#[cw_serde]
pub struct LadderConfig {
    pub owner: Addr,
    pub manager_contract: Addr,
    pub scheduler_contract: Addr,
    pub distributor_contract: Addr,
    pub execution_rebate: Option<Coin>,
    pub orders: Vec<LadderOrder>,
}

#[cw_serde]
pub struct LadderStatistics {
    pub remaining: Coin,
    pub filled: Coin,
    pub claimed: Coin,
    pub withdrawn: Vec<Coin>,
}
