use crate::types::Callback;
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Coin, Decimal};

#[cw_serde]
pub enum Route {
    Fin { address: Addr },
    Thorchain {},
}

#[cw_serde]
pub enum ExchangeExecuteMsg {
    Swap {
        minimum_receive_amount: Coin,
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
pub enum ExchangeQueryMsg {
    #[returns(bool)]
    CanSwap {
        swap_amount: Coin,
        minimum_receive_amount: Coin,
        route: Option<Route>,
    },
    #[returns(Vec<Coin>)]
    Path {
        swap_amount: Coin,
        target_denom: String,
        route: Option<Route>,
    },
    #[returns(Decimal)]
    SpotPrice {
        swap_denom: String,
        target_denom: String,
        route: Option<Route>,
    },
    #[returns(ExpectedReceiveAmount)]
    ExpectedReceiveAmount {
        swap_amount: Coin,
        target_denom: String,
        route: Option<Route>,
    },
}
