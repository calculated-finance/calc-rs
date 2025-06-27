use calc_rs::{
    exchanger::{ExpectedReceiveAmount, Route},
    types::{Callback, ContractResult},
};
use cosmwasm_std::{Addr, Coin, Decimal, Deps, Env, MessageInfo, StdResult};
use rujira_rs::NativeAsset;

pub trait Exchange {
    fn can_swap(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        route: &Option<Route>,
    ) -> StdResult<bool>;
    fn path(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
        route: &Option<Route>,
    ) -> StdResult<Vec<Coin>>;
    fn expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
        route: &Option<Route>,
    ) -> StdResult<ExpectedReceiveAmount>;
    fn spot_price(
        &self,
        deps: Deps,
        swap_denom: &NativeAsset,
        target_denom: &NativeAsset,
        route: &Option<Route>,
    ) -> StdResult<Decimal>;
    fn swap(
        &self,
        deps: Deps,
        env: &Env,
        info: &MessageInfo,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        route: &Option<Route>,
        recipient: Addr,
        on_complete: Option<Callback>,
    ) -> ContractResult;
}
