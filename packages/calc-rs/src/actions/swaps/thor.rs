use std::{
    cmp::{max, min},
    vec,
};

use crate::{
    actions::swaps::swap::{
        Adjusted, Executable, New, Quotable, SwapAmountAdjustment, SwapQuote, SwapRoute,
    },
    thorchain::{MsgDeposit, SwapQuote as ThorchainSwapQuote, SwapQuoteRequest},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, CosmosMsg, Decimal, Deps, Env, StdError, StdResult, Uint128};

#[cw_serde]
pub struct StreamingSwap {
    swap_amount: Coin,
    expected_receive_amount: Coin,
    starting_block: u64,
    streaming_swap_blocks: u64,
    memo: String,
}

#[cw_serde]
pub struct ThorchainRoute {
    pub streaming_interval: Option<u64>,
    pub max_streaming_quantity: Option<u64>,
    pub affiliate_code: Option<String>,
    pub affiliate_bps: Option<u64>,
    pub latest_swap: Option<StreamingSwap>,
}

fn is_secured_asset(denom: &str) -> bool {
    denom.to_lowercase() == "rune" || denom.contains("-")
}

impl Quotable for ThorchainRoute {
    fn validate(&self, _deps: Deps, route: &SwapQuote<New>) -> StdResult<()> {
        if !is_secured_asset(route.swap_amount.denom.as_str()) {
            return Err(StdError::generic_err(
                "Swap denom must be RUNE or a secured asset",
            ));
        }

        if !is_secured_asset(route.minimum_receive_amount.denom.as_str()) {
            return Err(StdError::generic_err(
                "Target denom must be RUNE or a secured asset",
            ));
        }

        if let Some(streaming_interval) = self.streaming_interval {
            if streaming_interval == 0 {
                return Err(StdError::generic_err("Streaming interval cannot be zero"));
            }

            if streaming_interval > 50 {
                return Err(StdError::generic_err(
                    "Streaming interval cannot exceed 50 blocks",
                ));
            }
        }

        if let Some(max_streaming_quantity) = self.max_streaming_quantity {
            if max_streaming_quantity == 0 {
                return Err(StdError::generic_err(
                    "Maximum streaming quantity cannot be zero",
                ));
            }

            // 6 seconds per block, max 24 hours to a swap = 14,400 max swaps
            if max_streaming_quantity > 14_400 {
                return Err(StdError::generic_err(
                    "Maximum streaming quantity cannot exceed 14,400",
                ));
            }
        }

        Ok(())
    }

    fn adjust(
        &self,
        deps: Deps,
        env: &Env,
        route: SwapQuote<New>,
    ) -> StdResult<SwapQuote<Adjusted>> {
        let (new_swap_amount, new_minimum_receive_amount, max_streaming_quantity) =
            match route.adjustment.clone() {
                SwapAmountAdjustment::Fixed => {
                    let swap_balance = deps.querier.query_balance(
                        env.contract.address.clone(),
                        route.swap_amount.denom.clone(),
                    )?;

                    let new_swap_amount = Coin::new(
                        min(swap_balance.amount, route.swap_amount.amount),
                        route.swap_amount.denom.clone(),
                    );

                    let new_minimum_receive_amount = Coin::new(
                        route
                            .minimum_receive_amount
                            .amount
                            .mul_floor(Decimal::from_ratio(
                                new_swap_amount.amount,
                                route.swap_amount.amount,
                            )),
                        route.minimum_receive_amount.denom.clone(),
                    );

                    let quote = get_swap_quote(deps, &route)?;

                    (
                        new_swap_amount,
                        new_minimum_receive_amount,
                        min(
                            quote.max_streaming_quantity,
                            self.max_streaming_quantity
                                .unwrap_or(quote.max_streaming_quantity),
                        ),
                    )
                }
                SwapAmountAdjustment::LinearScalar {
                    base_receive_amount,
                    minimum_swap_amount,
                    scalar,
                } => {
                    let quote = get_swap_quote(deps, &route)?;

                    let base_price =
                        Decimal::from_ratio(base_receive_amount.amount, route.swap_amount.amount);

                    let current_price =
                        Decimal::from_ratio(route.swap_amount.amount, quote.expected_amount_out);

                    let price_delta = base_price.abs_diff(current_price) / base_price;
                    let scaled_price_delta = price_delta * scalar;

                    let scaled_swap_amount = if current_price < base_price {
                        route
                            .swap_amount
                            .amount
                            .mul_floor(Decimal::one().saturating_add(scaled_price_delta))
                    } else {
                        route
                            .swap_amount
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

                    let new_minimum_receive_amount = Coin::new(
                        route
                            .minimum_receive_amount
                            .amount
                            .mul_ceil(Decimal::from_ratio(
                                new_swap_amount.amount,
                                route.swap_amount.amount,
                            )),
                        route.minimum_receive_amount.denom.clone(),
                    );

                    (
                        new_swap_amount,
                        new_minimum_receive_amount,
                        min(
                            quote.max_streaming_quantity,
                            self.max_streaming_quantity
                                .unwrap_or(quote.max_streaming_quantity),
                        ),
                    )
                }
            };

        Ok(SwapQuote {
            swap_amount: new_swap_amount,
            minimum_receive_amount: new_minimum_receive_amount,
            maximum_slippage_bps: route.maximum_slippage_bps,
            adjustment: route.adjustment,
            route: SwapRoute::Thorchain(ThorchainRoute {
                max_streaming_quantity: Some(max_streaming_quantity),
                ..self.clone()
            }),
            state: Adjusted,
        })
    }

    fn validate_adjusted(
        &self,
        deps: Deps,
        env: &Env,
        route: SwapQuote<Adjusted>,
    ) -> StdResult<SwapQuote<Executable>> {
        if route.swap_amount.amount.is_zero() {
            return Err(StdError::generic_err("Swap amount cannot be zero"));
        }

        let adjusted_quote = get_swap_quote(deps, &route)?;

        if let Some(fees) = adjusted_quote.fees {
            if fees.slippage_bps > route.maximum_slippage_bps {
                return Err(StdError::generic_err(format!(
                    "Slippage BPS ({}) exceeds maximum allowed ({})",
                    fees.slippage_bps, route.maximum_slippage_bps
                )));
            }
        }

        if adjusted_quote.expected_amount_out < route.minimum_receive_amount.amount {
            return Err(StdError::generic_err(format!(
                "Expected amount out ({}) is less than minimum receive amount ({})",
                adjusted_quote.expected_amount_out, route.minimum_receive_amount.amount
            )));
        }

        if adjusted_quote.recommended_min_amount_in > route.swap_amount.amount {
            return Err(StdError::generic_err(format!(
                "Recommended min amount in ({}) is greater than swap amount ({})",
                adjusted_quote.recommended_min_amount_in, route.swap_amount.amount
            )));
        }

        Ok(SwapQuote {
            swap_amount: route.swap_amount.clone(),
            minimum_receive_amount: route.minimum_receive_amount.clone(),
            maximum_slippage_bps: route.maximum_slippage_bps,
            adjustment: route.adjustment,
            route: SwapRoute::Thorchain(ThorchainRoute {
                latest_swap: Some(StreamingSwap {
                    swap_amount: route.swap_amount,
                    expected_receive_amount: Coin::new(
                        adjusted_quote.expected_amount_out,
                        route.minimum_receive_amount.denom.clone(),
                    ),
                    starting_block: env.block.height + 1,
                    streaming_swap_blocks: adjusted_quote.streaming_swap_blocks,
                    memo: adjusted_quote.memo,
                }),
                ..self.clone()
            }),
            state: Executable {
                expected_amount_out: Coin::new(
                    adjusted_quote.expected_amount_out,
                    route.minimum_receive_amount.denom,
                ),
            },
        })
    }

    fn execute(
        &self,
        deps: Deps,
        env: &Env,
        swap_amount: Coin,
        _minimum_receive_amount: Coin,
    ) -> StdResult<CosmosMsg> {
        let current_swap = if let Some(current_swap) = self.latest_swap.clone() {
            current_swap
        } else {
            return Err(StdError::generic_err("No current swap found to execute"));
        };

        let swap_msg = MsgDeposit {
            memo: current_swap.memo,
            coins: vec![swap_amount.clone()],
            signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
        }
        .into_cosmos_msg()?;

        Ok(swap_msg)
    }
}

impl<S> TryFrom<&SwapQuote<S>> for SwapQuoteRequest {
    type Error = StdError;

    fn try_from(quote: &SwapQuote<S>) -> Result<Self, Self::Error> {
        match &quote.route {
            SwapRoute::Thorchain(ThorchainRoute {
                streaming_interval,
                max_streaming_quantity,
                affiliate_code,
                affiliate_bps,
                ..
            }) => {
                Ok(SwapQuoteRequest {
                    from_asset: quote.swap_amount.denom.clone(),
                    to_asset: quote.minimum_receive_amount.denom.clone(),
                    amount: quote.swap_amount.amount,
                    streaming_interval: Uint128::new(
                        // Default to swapping every 3 blocks
                        streaming_interval.unwrap_or(3) as u128,
                    ),
                    streaming_quantity: Uint128::new(
                        // Setting this to 0 allows the chain to
                        // calculate the maximum streaming quantity
                        max_streaming_quantity.unwrap_or(0) as u128,
                    ),
                    destination: String::new(), // This will be set later
                    refund_address: String::new(), // This will be set later
                    affiliate: affiliate_code.clone().map_or_else(Vec::new, |c| vec![c]),
                    affiliate_bps: affiliate_bps.map_or_else(Vec::new, |b| vec![b]),
                })
            }
            _ => Err(StdError::generic_err(
                "Cannot convert non-Thorchain route to SwapQuoteRequest",
            )),
        }
    }
}

pub fn get_swap_quote<S>(deps: Deps, quote: &SwapQuote<S>) -> StdResult<ThorchainSwapQuote> {
    ThorchainSwapQuote::get(deps.querier, &SwapQuoteRequest::try_from(quote)?).map_err(|e| {
        StdError::generic_err(format!(
            "Failed to get L1 swap quote with {:?}: {e}",
            quote.route
        ))
    })
}
