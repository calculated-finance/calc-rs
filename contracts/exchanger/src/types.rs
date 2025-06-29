use calc_rs::{
    exchanger::{ExpectedReceiveAmount, Route},
    core::{Callback, ContractResult},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Deps, Env, MessageInfo, StdResult};

#[cw_serde]
pub struct ExchangeConfig {
    pub scheduler_address: Addr,
    pub affiliate_code: Option<String>,
    pub affiliate_bps: Option<u64>,
}

pub trait Exchange {
    fn expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &str,
        route: &Option<Route>,
    ) -> StdResult<ExpectedReceiveAmount>;
    fn swap(
        &self,
        deps: Deps,
        env: &Env,
        info: &MessageInfo,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        maximum_slippage_bps: u128,
        route: &Option<Route>,
        recipient: Addr,
        on_complete: Option<Callback>,
    ) -> ContractResult;
}
