use calc_rs::{
    exchanger::{ExpectedReceiveAmount, Route},
    core::{Callback, ContractResult},
};
use cosmwasm_std::{Addr, Coin, Deps, Env, MessageInfo, StdResult};

use crate::types::Exchange;

pub struct MockExchange {
    pub get_expected_receive_amount_fn: Box<
        dyn Fn(Deps, &Coin, &str, &Option<Route>) -> StdResult<ExpectedReceiveAmount> + Send + Sync,
    >,
    pub swap_fn: Box<
        dyn Fn(
                Deps,
                &Env,
                &MessageInfo,
                &Coin,
                &Coin,
                u128,
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
            get_expected_receive_amount_fn: Box::new(|_, swap_amount, target_denom, _| {
                Ok(ExpectedReceiveAmount {
                    receive_amount: Coin::new(swap_amount.amount, target_denom),
                    slippage_bps: Default::default(),
                })
            }),
            swap_fn: Box::new(|_, _, _, _, _, _, _, _, _| Ok(Default::default())),
        }
    }
}

impl Exchange for MockExchange {
    fn expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &str,
        route: &Option<Route>,
    ) -> StdResult<ExpectedReceiveAmount> {
        (self.get_expected_receive_amount_fn)(deps, swap_amount, target_denom, route)
    }

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
    ) -> ContractResult {
        (self.swap_fn)(
            deps,
            env,
            info,
            swap_amount,
            minimum_receive_amount,
            maximum_slippage_bps,
            route,
            recipient,
            on_complete,
        )
    }
}
