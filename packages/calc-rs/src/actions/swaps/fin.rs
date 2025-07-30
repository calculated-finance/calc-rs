use std::{
    cmp::{max, min},
    vec,
};

use crate::{
    actions::swaps::swap::{
        Adjusted, Executable, New, Quotable, SwapAmountAdjustment, SwapQuote, Validated,
    },
    core::Contract,
    statistics::Statistics,
    strategy::{StrategyMsg, StrategyMsgPayload},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, Decimal, Deps, Env, Event, StdError, StdResult, Uint128,
};
use rujira_rs::fin::{
    BookResponse, ConfigResponse, ExecuteMsg, QueryMsg, SimulationResponse, SwapRequest,
};

enum FinSwapEvent {
    AttemptSwap {
        swap_amount: Coin,
        expected_receive_amount: Coin,
    },
}

impl From<FinSwapEvent> for Event {
    fn from(val: FinSwapEvent) -> Self {
        match val {
            FinSwapEvent::AttemptSwap {
                swap_amount,
                expected_receive_amount,
            } => Event::new("attempt_fin_swap")
                .add_attribute("swap_amount", swap_amount.to_string())
                .add_attribute(
                    "expected_receive_amount",
                    expected_receive_amount.to_string(),
                ),
        }
    }
}

#[cw_serde]
pub struct FinRoute {
    pub pair_address: Addr,
}

impl Quotable for FinRoute {
    fn verify(&self, deps: Deps, route: &SwapQuote<New>) -> StdResult<()> {
        let pair = deps
            .querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})?;

        let denoms = [pair.denoms.base(), pair.denoms.quote()];

        if !denoms.contains(&route.swap_amount.denom.as_str()) {
            return Err(StdError::generic_err(format!(
                "Pair at {} does not support swapping from {}",
                self.pair_address, route.swap_amount.denom
            )));
        }

        if !denoms.contains(&route.minimum_receive_amount.denom.as_str()) {
            return Err(StdError::generic_err(format!(
                "Pair at {} does not support swapping into {}",
                self.pair_address, route.minimum_receive_amount.denom
            )));
        }

        Ok(())
    }

    fn adjust(
        &self,
        deps: Deps,
        env: &Env,
        route: &SwapQuote<New>,
    ) -> StdResult<SwapQuote<Adjusted>> {
        let (new_swap_amount, new_minimum_receive_amount) = match route.adjustment.clone() {
            SwapAmountAdjustment::Fixed => {
                let swap_balance = deps.querier.query_balance(
                    env.contract.address.clone(),
                    route.swap_amount.denom.clone(),
                )?;

                let new_swap_amount = Coin::new(
                    min(swap_balance.amount, route.swap_amount.amount),
                    route.swap_amount.denom.clone(),
                );

                let new_minimum_receive_amount =
                    route
                        .minimum_receive_amount
                        .amount
                        .mul_floor(Decimal::from_ratio(
                            new_swap_amount.amount,
                            route.swap_amount.amount,
                        ));

                (new_swap_amount, new_minimum_receive_amount)
            }
            SwapAmountAdjustment::LinearScalar {
                base_receive_amount,
                minimum_swap_amount,
                scalar,
            } => {
                let swap_balance = deps.querier.query_balance(
                    env.contract.address.clone(),
                    route.swap_amount.denom.clone(),
                )?;

                let new_swap_amount = Coin::new(
                    min(swap_balance.amount, route.swap_amount.amount),
                    route.swap_amount.denom.clone(),
                );

                let simulation = deps.querier.query_wasm_smart::<SimulationResponse>(
                    self.pair_address.clone(),
                    &QueryMsg::Simulate(new_swap_amount.clone()),
                )?;

                let expected_receive_amount =
                    Coin::new(simulation.returned, route.swap_amount.denom.clone());

                let base_price =
                    Decimal::from_ratio(base_receive_amount.amount, route.swap_amount.amount);

                let current_price =
                    Decimal::from_ratio(route.swap_amount.amount, expected_receive_amount.amount);

                let price_delta = base_price.abs_diff(current_price) / base_price;
                let scaled_price_delta = price_delta * scalar;

                let scaled_swap_amount = if current_price < base_price {
                    new_swap_amount
                        .amount
                        .mul_floor(Decimal::one().saturating_add(scaled_price_delta))
                } else {
                    new_swap_amount
                        .amount
                        .mul_floor(Decimal::one().saturating_sub(scaled_price_delta))
                };

                let new_swap_amount = Coin::new(
                    max(
                        scaled_swap_amount,
                        minimum_swap_amount
                            .clone()
                            .unwrap_or(Coin::new(0u128, route.swap_amount.denom.clone()))
                            .amount,
                    ),
                    route.swap_amount.denom.clone(),
                );

                let new_minimum_receive_amount =
                    route
                        .minimum_receive_amount
                        .amount
                        .mul_ceil(Decimal::from_ratio(
                            new_swap_amount.amount,
                            route.swap_amount.amount,
                        ));

                (new_swap_amount, new_minimum_receive_amount)
            }
        };

        Ok(SwapQuote {
            swap_amount: new_swap_amount,
            minimum_receive_amount: Coin::new(
                new_minimum_receive_amount,
                route.minimum_receive_amount.denom.clone(),
            ),
            maximum_slippage_bps: route.maximum_slippage_bps,
            adjustment: route.adjustment.clone(),
            route: route.route.clone(),
            state: Adjusted,
        })
    }

    fn validate(
        &self,
        deps: Deps,
        _env: &Env,
        route: &SwapQuote<Adjusted>,
    ) -> StdResult<SwapQuote<Validated>> {
        if route.swap_amount.amount.is_zero() {
            return Err(StdError::generic_err(
                "Swap amount after adjustment is zero".to_string(),
            ));
        }

        let book_response = deps.querier.query_wasm_smart::<BookResponse>(
            self.pair_address.clone(),
            &QueryMsg::Book {
                limit: Some(1),
                offset: None,
            },
        )?;

        let mid_price = (book_response.base[0].price + book_response.quote[0].price)
            / Decimal::from_ratio(2u128, 1u128);

        let pair = deps
            .querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})?;

        let spot_price = if route.swap_amount.denom == pair.denoms.base() {
            Decimal::one() / mid_price
        } else {
            mid_price
        };

        let simulation_response = deps.querier.query_wasm_smart::<SimulationResponse>(
            self.pair_address.clone(),
            &QueryMsg::Simulate(route.swap_amount.clone()),
        )?;

        let expected_amount_out = Coin::new(
            simulation_response.returned,
            route.minimum_receive_amount.denom.clone(),
        );

        if expected_amount_out.amount < route.minimum_receive_amount.amount {
            return Err(StdError::generic_err(format!(
                "Expected amount out {} for swapping {} is less than minimum receive amount {}",
                expected_amount_out.amount,
                route.swap_amount.amount,
                route.minimum_receive_amount.amount
            )));
        }

        let optimal_return_amount = max(
            expected_amount_out.amount,
            route
                .swap_amount
                .amount
                .mul_floor(Decimal::one() / spot_price),
        );

        let slippage_bps = Uint128::new(10_000).mul_ceil(
            Decimal::one()
                .checked_sub(Decimal::from_ratio(
                    expected_amount_out.amount,
                    optimal_return_amount,
                ))
                .unwrap_or(Decimal::one()),
        );

        if slippage_bps.gt(&Uint128::new(route.maximum_slippage_bps as u128)) {
            return Err(StdError::generic_err(format!(
                "Slippage of {} bps exceeds maximum allowed of {} bps",
                slippage_bps, route.maximum_slippage_bps
            )));
        }

        Ok(SwapQuote {
            swap_amount: route.swap_amount.clone(),
            minimum_receive_amount: Coin::new(
                route.minimum_receive_amount.amount,
                route.minimum_receive_amount.denom.clone(),
            ),
            maximum_slippage_bps: route.maximum_slippage_bps,
            adjustment: route.adjustment.clone(),
            route: route.route.clone(),
            state: Validated {
                expected_amount_out,
            },
        })
    }

    fn execute(
        &self,
        _deps: Deps,
        _env: &Env,
        route: &SwapQuote<Validated>,
    ) -> StdResult<SwapQuote<Executable>> {
        let swap_msg = StrategyMsg::with_payload(
            Contract(self.pair_address.clone()).call(
                to_json_binary(&ExecuteMsg::Swap(SwapRequest {
                    min_return: Some(route.minimum_receive_amount.amount),
                    to: None,
                    callback: None,
                }))?,
                vec![route.swap_amount.clone()],
            ),
            StrategyMsgPayload {
                statistics: Statistics {
                    debited: vec![route.swap_amount.clone()],
                    ..Statistics::default()
                },
                events: vec![FinSwapEvent::AttemptSwap {
                    swap_amount: route.swap_amount.clone(),
                    expected_receive_amount: route.state.expected_amount_out.clone(),
                }
                .into()],
            },
        );

        Ok(SwapQuote {
            swap_amount: route.swap_amount.clone(),
            minimum_receive_amount: route.minimum_receive_amount.clone(),
            maximum_slippage_bps: route.maximum_slippage_bps,
            adjustment: route.adjustment.clone(),
            route: route.route.clone(),
            state: Executable {
                messages: vec![swap_msg],
            },
        })
    }
}
