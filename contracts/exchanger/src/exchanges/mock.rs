use calc_rs::{
    exchanger::{ExpectedReceiveAmount, Route},
    types::{Callback, ContractResult},
};
use cosmwasm_std::{Addr, Coin, Decimal, Deps, Env, MessageInfo, StdResult};
use rujira_rs::NativeAsset;

use crate::types::Exchange;

pub struct MockExchange {
    pub can_swap_fn:
        Box<dyn Fn(Deps, &Coin, &Coin, &Option<Route>) -> StdResult<bool> + Send + Sync>,
    pub path_fn: Box<
        dyn Fn(Deps, &Coin, &NativeAsset, &Option<Route>) -> StdResult<Vec<Coin>> + Send + Sync,
    >,
    pub get_expected_receive_amount_fn: Box<
        dyn Fn(Deps, &Coin, &NativeAsset, &Option<Route>) -> StdResult<ExpectedReceiveAmount>
            + Send
            + Sync,
    >,
    pub get_spot_price_fn: Box<
        dyn Fn(Deps, &NativeAsset, &NativeAsset, &Option<Route>) -> StdResult<Decimal>
            + Send
            + Sync,
    >,
    pub swap_fn: Box<
        dyn Fn(
                Deps,
                &Env,
                &MessageInfo,
                &Coin,
                &Coin,
                &Option<Route>,
                Addr,
                Option<Callback>,
            ) -> ContractResult
            + Send
            + Sync,
    >,
}

impl Default for MockExchange {
    fn default() -> Self {
        Self {
            can_swap_fn: Box::new(|_, _, _, _| Ok(true)),
            path_fn: Box::new(|_, swap_amount, target_denom, _| {
                Ok(vec![
                    swap_amount.clone(),
                    Coin {
                        denom: target_denom.denom_string(),
                        amount: swap_amount.amount,
                    },
                ])
            }),
            get_expected_receive_amount_fn: Box::new(|_, swap_amount, target_denom, _| {
                Ok(ExpectedReceiveAmount {
                    receive_amount: Coin {
                        denom: target_denom.denom_string(),
                        amount: swap_amount.amount,
                    },
                    slippage_bps: Default::default(),
                })
            }),
            get_spot_price_fn: Box::new(|_, _, _, _| Ok(Decimal::one())),
            swap_fn: Box::new(|_, _, _, _, _, _, _, _| Ok(Default::default())),
        }
    }
}

impl Exchange for MockExchange {
    fn can_swap(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        route: &Option<Route>,
    ) -> StdResult<bool> {
        (self.can_swap_fn)(deps, swap_amount, minimum_receive_amount, route)
    }

    fn path(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
        route: &Option<Route>,
    ) -> StdResult<Vec<Coin>> {
        (self.path_fn)(deps, swap_amount, target_denom, route)
    }

    fn expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
        route: &Option<Route>,
    ) -> StdResult<ExpectedReceiveAmount> {
        (self.get_expected_receive_amount_fn)(deps, swap_amount, target_denom, route)
    }

    fn spot_price(
        &self,
        deps: Deps,
        swap_denom: &NativeAsset,
        target_denom: &NativeAsset,
        route: &Option<Route>,
    ) -> StdResult<Decimal> {
        (self.get_spot_price_fn)(deps, swap_denom, target_denom, route)
    }

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
    ) -> ContractResult {
        (self.swap_fn)(
            deps,
            env,
            info,
            swap_amount,
            minimum_receive_amount,
            route,
            recipient,
            on_complete,
        )
    }
}
