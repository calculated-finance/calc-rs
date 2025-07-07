use crate::core::Callback;
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Coin};

#[cw_serde]
pub enum Route {
    FinMarket {
        address: Addr,
    },
    Thorchain {
        streaming_interval: Option<u64>,
        max_streaming_quantity: Option<u64>,
        affiliate_code: Option<String>,
        affiliate_bps: Option<u64>,
    },
}

#[cw_serde]
pub struct ExchangerInstantiateMsg {
    pub scheduler_address: Addr,
}

#[cw_serde]
pub enum ExchangerExecuteMsg {
    Swap {
        minimum_receive_amount: Coin,
        maximum_slippage_bps: u128,
        route: Option<Route>,
        recipient: Option<Addr>,
        on_complete: Option<Callback>,
    },
}

#[cw_serde]
pub struct ExpectedReceiveAmount {
    pub receive_amount: Coin,
    pub slippage_bps: u128,
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum ExchangerQueryMsg {
    #[returns(ExpectedReceiveAmount)]
    ExpectedReceiveAmount {
        swap_amount: Coin,
        target_denom: String,
        route: Option<Route>,
    },
}
