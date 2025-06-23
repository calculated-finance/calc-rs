use calc_rs::types::{Callback, ContractResult, ExpectedReceiveAmount};
use cosmwasm_std::{Addr, Coin, Decimal, Deps, Env, MessageInfo, StdResult};
use rujira_rs::NativeAsset;

pub trait Exchange {
    fn can_swap(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
    ) -> StdResult<bool>;
    fn route(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
    ) -> StdResult<Vec<Coin>>;
    fn expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
    ) -> StdResult<ExpectedReceiveAmount>;
    fn spot_price(
        &self,
        deps: Deps,
        swap_denom: &NativeAsset,
        target_denom: &NativeAsset,
    ) -> StdResult<Decimal>;
    fn swap(
        &self,
        deps: Deps,
        env: &Env,
        info: &MessageInfo,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        recipient: Addr,
        on_complete: Option<Callback>,
    ) -> ContractResult;
}
