use calc_rs::types::{ContractResult, ExpectedReturnAmount};
use cosmwasm_std::{Coin, Decimal, Deps, Env, MessageInfo, StdResult};

pub trait Exchange {
    fn can_swap(&self, deps: Deps, swap_denom: &str, target_denom: &str) -> StdResult<bool>;
    fn route(&self, deps: Deps, swap_amount: Coin, target_denom: &str) -> StdResult<Vec<Coin>>;
    fn get_expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: Coin,
        target_denom: &str,
    ) -> StdResult<ExpectedReturnAmount>;
    fn get_spot_price(
        &self,
        deps: Deps,
        swap_denom: &str,
        target_denom: &str,
    ) -> StdResult<Decimal>;
    fn swap(
        &self,
        deps: Deps,
        env: Env,
        info: MessageInfo,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
    ) -> ContractResult;
}
