use calc_rs::types::{ContractResult, ExpectedReturnAmount};
use cosmwasm_std::{Addr, Coin, Decimal, Deps, Env, StdResult};
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
    ) -> StdResult<ExpectedReturnAmount>;
    fn spot_price(
        &self,
        deps: Deps,
        swap_denom: &NativeAsset,
        target_denom: &NativeAsset,
    ) -> StdResult<Decimal>;
    fn swap(
        &self,
        deps: Deps,
        env: Env,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        recipient: Addr,
    ) -> ContractResult;
}
