use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Decimal};

use crate::{core::Condition, distributor::Destination};

#[cw_serde]
pub struct InstantiateStopLossCommand {
    pub owner: Addr,
    pub manager_contract: Addr,
    pub scheduler_contract: Addr,
    pub distributor_code_id: u64,
    pub pair_address: Addr,
    pub swap_denom: String,
    pub target_denom: String,
    pub offset: Decimal,
    pub execution_rebate: Option<Coin>,
    pub affiliate_code: Option<String>,
    pub minimum_distribute_amount: Option<Coin>,
    pub mutable_destinations: Vec<Destination>,
    pub immutable_destinations: Vec<Destination>,
}

#[cw_serde]
pub struct StopLossConfig {
    pub owner: Addr,
    pub manager_contract: Addr,
    pub scheduler_contract: Addr,
    pub distributor_contract: Addr,
    pub pair_address: Addr,
    pub swap_denom: String,
    pub target_denom: String,
    pub offset: Decimal,
    pub move_conditions: Vec<Condition>,
    pub distribute_conditions: Vec<Condition>,
    pub execution_rebate: Option<Coin>,
}

#[cw_serde]
pub struct StopLossStatistics {
    pub remaining: Coin,
    pub filled: Coin,
    pub claimed: Coin,
    pub withdrawn: Vec<Coin>,
}
