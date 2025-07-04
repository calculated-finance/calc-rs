// use crate::core::{Condition, Schedule};
// use crate::distributor::Destination;
// use crate::exchanger::Route;
// use cosmwasm_schema::cw_serde;
// use cosmwasm_std::{Addr, Coin};

// #[cw_serde]
// pub struct TwapConfig {
//     pub owner: Addr,
//     pub manager_contract: Addr,
//     pub exchanger_contract: Addr,
//     pub scheduler_contract: Addr,
//     pub distributor_contract: Addr,
//     pub swap_amount: Coin,
//     pub minimum_receive_amount: Coin,
//     pub maximum_slippage_bps: u128,
//     pub route: Option<Route>,
//     pub swap_cadence: Schedule,
//     pub swap_conditions: Vec<Condition>,
//     pub schedule_conditions: Vec<Condition>,
//     pub execution_rebate: Option<Coin>,
// }

// #[cw_serde]
// pub struct InstantiateTwapCommand {
//     pub owner: Addr,
//     pub swap_amount: Coin,
//     pub minimum_receive_amount: Coin,
//     pub maximum_slippage_bps: u128,
//     pub route: Option<Route>,
//     pub swap_cadence: Schedule,
//     pub distributor_code_id: u64,
//     pub exchanger_contract: Addr,
//     pub scheduler_contract: Addr,
//     pub execution_rebate: Option<Coin>,
//     pub affiliate_code: Option<String>,
//     pub minimum_distribute_amount: Option<Coin>,
//     pub mutable_destinations: Vec<Destination>,
//     pub immutable_destinations: Vec<Destination>,
// }
