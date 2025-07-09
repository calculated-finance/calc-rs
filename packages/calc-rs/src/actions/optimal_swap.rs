use std::{collections::HashSet, vec};

use crate::{
    actions::{
        action::Action,
        fin_swap::{get_expected_amount_out as get_expected_amount_out_fin, FinSwap},
        operation::StatelessOperation,
        thor_swap::{
            get_expected_amount_out as get_expected_amount_out_thorchain, StreamingSwap, ThorSwap,
        },
    },
    strategy::StrategyMsg,
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Decimal, Deps, Env, Event, StdError, StdResult};

#[cw_serde]
pub enum SwapAmountAdjustment {
    Fixed,
    LinearScalar {
        base_receive_amount: Coin,
        minimum_swap_amount: Option<Coin>,
        scalar: Decimal,
    },
}

pub trait Exchange {
    fn get_expected_amount_out(&self, deps: Deps, env: Env) -> StdResult<Coin>;
}

#[cw_serde]
pub enum SwapRoute {
    Fin(Addr),
    Thorchain {
        streaming_interval: Option<u64>,
        max_streaming_quantity: Option<u64>,
        affiliate_code: Option<String>,
        affiliate_bps: Option<u64>,
        previous_swap: Option<StreamingSwap>,
    },
}

impl From<Action> for SwapRoute {
    fn from(action: Action) -> Self {
        match action {
            Action::FinSwap(fin_swap) => SwapRoute::Fin(fin_swap.pair_address),
            Action::ThorSwap(thor_swap) => SwapRoute::Thorchain {
                streaming_interval: thor_swap.streaming_interval,
                max_streaming_quantity: thor_swap.max_streaming_quantity,
                affiliate_code: thor_swap.affiliate_code,
                affiliate_bps: thor_swap.affiliate_bps,
                previous_swap: thor_swap.previous_swap,
            },
            _ => panic!("Invalid action type for SwapRoute"),
        }
    }
}

impl SwapRoute {
    pub fn to_action(&self, swap: OptimalSwap) -> Action {
        match self {
            SwapRoute::Fin(address) => Action::FinSwap(FinSwap {
                pair_address: address.clone(),
                swap_amount: swap.swap_amount,
                minimum_receive_amount: swap.minimum_receive_amount,
                maximum_slippage_bps: swap.maximum_slippage_bps,
                adjustment: swap.adjustment,
            }),
            SwapRoute::Thorchain {
                streaming_interval,
                max_streaming_quantity,
                affiliate_code,
                affiliate_bps,
                previous_swap,
            } => Action::ThorSwap(ThorSwap {
                swap_amount: swap.swap_amount,
                minimum_receive_amount: swap.minimum_receive_amount,
                maximum_slippage_bps: swap.maximum_slippage_bps,
                adjustment: swap.adjustment,
                streaming_interval: *streaming_interval,
                max_streaming_quantity: *max_streaming_quantity,
                affiliate_code: affiliate_code.clone(),
                affiliate_bps: *affiliate_bps,
                previous_swap: previous_swap.clone(),
            }),
        }
    }

    pub fn get_expected_amount_out(
        &self,
        deps: Deps,
        env: &Env,
        swap: OptimalSwap,
    ) -> StdResult<Coin> {
        match self {
            SwapRoute::Fin(address) => get_expected_amount_out_fin(
                deps,
                &FinSwap {
                    pair_address: address.clone(),
                    swap_amount: swap.swap_amount,
                    minimum_receive_amount: swap.minimum_receive_amount,
                    maximum_slippage_bps: swap.maximum_slippage_bps,
                    adjustment: swap.adjustment,
                },
            ),
            SwapRoute::Thorchain {
                streaming_interval,
                max_streaming_quantity,
                affiliate_code,
                affiliate_bps,
                previous_swap,
            } => get_expected_amount_out_thorchain(
                deps,
                env,
                &ThorSwap {
                    swap_amount: swap.swap_amount,
                    minimum_receive_amount: swap.minimum_receive_amount,
                    maximum_slippage_bps: swap.maximum_slippage_bps,
                    adjustment: swap.adjustment,
                    streaming_interval: *streaming_interval,
                    max_streaming_quantity: *max_streaming_quantity,
                    affiliate_code: affiliate_code.clone(),
                    affiliate_bps: *affiliate_bps,
                    previous_swap: previous_swap.clone(),
                },
            ),
        }
    }
}

#[cw_serde]
pub struct OptimalSwap {
    pub swap_amount: Coin,
    pub minimum_receive_amount: Coin,
    pub maximum_slippage_bps: u128,
    pub adjustment: SwapAmountAdjustment,
    pub routes: Vec<SwapRoute>,
}

impl StatelessOperation for OptimalSwap {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if self.swap_amount.amount.is_zero() {
            return Err(StdError::generic_err("Swap amount cannot be zero"));
        }

        if self.maximum_slippage_bps > 10_000 {
            return Err(StdError::generic_err(
                "Maximum slippage basis points cannot exceed 10,000",
            ));
        }

        if self.routes.is_empty() {
            return Err(StdError::generic_err("No swap routes provided"));
        }

        let mut messages = vec![];
        let mut events = vec![];
        let mut initialised_routes = vec![];

        for route in self.routes.iter() {
            let (init_messages, init_events, action) =
                route.to_action(self.clone()).init(deps, env)?;

            messages.extend(init_messages);
            events.extend(init_events);
            initialised_routes.push(SwapRoute::from(action));
        }

        Ok((
            messages,
            events,
            Action::OptimalSwap(OptimalSwap {
                routes: initialised_routes,
                ..self
            }),
        ))
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        let best_route_index = self
            .routes
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                r.get_expected_amount_out(deps, env, self.clone())
                    .ok()
                    .map(|amount| (i, amount))
            })
            .max_by(|a, b| a.1.amount.cmp(&b.1.amount))
            .map(|(i, _)| i);

        if let Some(best_route_index) = best_route_index {
            let best_route = &self.routes[best_route_index];
            let (swap_messages, swap_events, action) =
                best_route.to_action(self.clone()).execute(deps, env);

            let mut updated_routes = self.routes.clone();
            updated_routes[best_route_index] = action.into();

            return (
                swap_messages,
                swap_events,
                Action::OptimalSwap(OptimalSwap {
                    routes: updated_routes,
                    ..self
                }),
            );
        };

        (vec![], vec![], Action::OptimalSwap(self))
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        let mut escrowed = HashSet::new();

        for route in &self.routes {
            let route_escrowed = route.to_action(self.clone()).escrowed(_deps, _env)?;
            escrowed.extend(route_escrowed);
        }

        Ok(escrowed)
    }
}
