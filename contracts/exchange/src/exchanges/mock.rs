use calc_rs::types::{ContractResult, ExpectedReturnAmount};
use cosmwasm_std::{Addr, Coin, Decimal, Deps, Env, StdResult};
use rujira_rs::NativeAsset;

use crate::types::Exchange;

pub struct MockExchange {
    pub can_swap_fn: Box<dyn Fn(Deps, &Coin, &Coin) -> StdResult<bool> + Send + Sync>,
    pub route_fn: Box<dyn Fn(Deps, &Coin, &NativeAsset) -> StdResult<Vec<Coin>> + Send + Sync>,
    pub get_expected_receive_amount_fn:
        Box<dyn Fn(Deps, &Coin, &NativeAsset) -> StdResult<ExpectedReturnAmount> + Send + Sync>,
    pub get_spot_price_fn:
        Box<dyn Fn(Deps, &NativeAsset, &NativeAsset) -> StdResult<Decimal> + Send + Sync>,
    pub swap_fn: Box<dyn Fn(Deps, Env, &Coin, &Coin, Addr) -> ContractResult + Send + Sync>,
}

impl Default for MockExchange {
    fn default() -> Self {
        Self {
            can_swap_fn: Box::new(|_, _, _| Ok(true)),
            route_fn: Box::new(|_, _, _| Ok(vec![])),
            get_expected_receive_amount_fn: Box::new(|_, _, _| {
                Ok(ExpectedReturnAmount {
                    return_amount: Default::default(),
                    slippage: Default::default(),
                })
            }),
            get_spot_price_fn: Box::new(|_, _, _| Ok(Decimal::one())),
            swap_fn: Box::new(|_, _, _, _, _| Ok(Default::default())),
        }
    }
}

impl Exchange for MockExchange {
    fn can_swap(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
    ) -> StdResult<bool> {
        (self.can_swap_fn)(deps, swap_amount, minimum_receive_amount)
    }
    fn route(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
    ) -> StdResult<Vec<Coin>> {
        (self.route_fn)(deps, swap_amount, target_denom)
    }
    fn expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
    ) -> StdResult<ExpectedReturnAmount> {
        (self.get_expected_receive_amount_fn)(deps, swap_amount, target_denom)
    }
    fn spot_price(
        &self,
        deps: Deps,
        swap_denom: &NativeAsset,
        target_denom: &NativeAsset,
    ) -> StdResult<Decimal> {
        (self.get_spot_price_fn)(deps, swap_denom, target_denom)
    }
    fn swap(
        &self,
        deps: Deps,
        env: Env,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        recipient: Addr,
    ) -> ContractResult {
        (self.swap_fn)(deps, env, swap_amount, minimum_receive_amount, recipient)
    }
}
