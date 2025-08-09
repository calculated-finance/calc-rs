use std::{
    cmp::{max, min},
    vec,
};

use crate::{
    actions::swaps::swap::{Adjusted, Executable, New, Quotable, SwapAmountAdjustment, SwapQuote},
    core::Contract,
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, CosmosMsg, Decimal, Deps, Env, StdError, StdResult, Uint128,
};
use rujira_rs::fin::{
    BookResponse, ConfigResponse, ExecuteMsg, QueryMsg, SimulationResponse, SwapRequest,
};

#[cw_serde]
pub struct FinRoute {
    pub pair_address: Addr,
}

impl Quotable for FinRoute {
    fn validate(&self, deps: Deps, route: &SwapQuote<New>) -> StdResult<()> {
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
        quote: SwapQuote<New>,
    ) -> StdResult<SwapQuote<Adjusted>> {
        let (new_swap_amount, new_minimum_receive_amount) = match quote.adjustment.clone() {
            SwapAmountAdjustment::Fixed => {
                let swap_balance = deps.querier.query_balance(
                    env.contract.address.clone(),
                    quote.swap_amount.denom.clone(),
                )?;

                let new_swap_amount = Coin::new(
                    min(swap_balance.amount, quote.swap_amount.amount),
                    quote.swap_amount.denom.clone(),
                );

                let new_minimum_receive_amount =
                    quote
                        .minimum_receive_amount
                        .amount
                        .mul_floor(Decimal::from_ratio(
                            new_swap_amount.amount,
                            quote.swap_amount.amount,
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
                    quote.swap_amount.denom.clone(),
                )?;

                let new_swap_amount = Coin::new(
                    min(swap_balance.amount, quote.swap_amount.amount),
                    quote.swap_amount.denom.clone(),
                );

                let simulation = deps.querier.query_wasm_smart::<SimulationResponse>(
                    self.pair_address.clone(),
                    &QueryMsg::Simulate(new_swap_amount.clone()),
                )?;

                let expected_receive_amount =
                    Coin::new(simulation.returned, quote.swap_amount.denom.clone());

                let base_price =
                    Decimal::from_ratio(base_receive_amount.amount, quote.swap_amount.amount);

                let current_price =
                    Decimal::from_ratio(quote.swap_amount.amount, expected_receive_amount.amount);

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
                            .unwrap_or(Coin::new(0u128, quote.swap_amount.denom.clone()))
                            .amount,
                    ),
                    quote.swap_amount.denom.clone(),
                );

                let new_minimum_receive_amount =
                    quote
                        .minimum_receive_amount
                        .amount
                        .mul_ceil(Decimal::from_ratio(
                            new_swap_amount.amount,
                            quote.swap_amount.amount,
                        ));

                (new_swap_amount, new_minimum_receive_amount)
            }
        };

        Ok(SwapQuote {
            swap_amount: new_swap_amount,
            minimum_receive_amount: Coin::new(
                new_minimum_receive_amount,
                quote.minimum_receive_amount.denom,
            ),
            maximum_slippage_bps: quote.maximum_slippage_bps,
            adjustment: quote.adjustment,
            route: quote.route,
            state: Adjusted,
        })
    }

    fn validate_adjusted(
        &self,
        deps: Deps,
        _env: &Env,
        route: SwapQuote<Adjusted>,
    ) -> StdResult<SwapQuote<Executable>> {
        if route.swap_amount.amount.is_zero() {
            return Err(StdError::generic_err(
                "Swap amount after adjustment is zero".to_string(),
            ));
        }

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
            swap_amount: route.swap_amount,
            minimum_receive_amount: route.minimum_receive_amount,
            maximum_slippage_bps: route.maximum_slippage_bps,
            adjustment: route.adjustment,
            route: route.route,
            state: Executable {
                expected_amount_out,
            },
        })
    }

    fn execute(
        &self,
        _deps: Deps,
        _env: &Env,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
    ) -> StdResult<CosmosMsg> {
        let swap_msg = Contract(self.pair_address.clone()).call(
            to_json_binary(&ExecuteMsg::Swap(SwapRequest {
                min_return: Some(minimum_receive_amount.amount),
                to: None,
                callback: None,
            }))?,
            vec![swap_amount.clone()],
        );

        Ok(swap_msg)
    }
}
