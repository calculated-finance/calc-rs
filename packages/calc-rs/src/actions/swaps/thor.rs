use std::vec;

use crate::{
    actions::swaps::swap::{Adjusted, Executable, New, SwapQuote, SwapRoute},
    thorchain::{MsgDeposit, SwapQuote as ThorchainSwapQuote, SwapQuoteRequest},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, CosmosMsg, Deps, Env, StdError, StdResult, Uint128};

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

impl ThorchainRoute {
    pub fn validate(&self, _deps: Deps, route: &SwapQuote<New>) -> StdResult<()> {
        self.get_expected_amount_out(_deps, route).map_err(|e| {
            StdError::generic_err(format!("Failed to get swap quote for Thorchain route: {e}"))
        })?;

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

    pub fn get_expected_amount_out(
        &self,
        deps: Deps,
        quote: &SwapQuote<New>,
    ) -> StdResult<Uint128> {
        let quote = get_thorchain_swap_quote(deps, quote)?;
        Ok(quote.expected_amount_out)
    }

    pub fn validate_adjusted(
        self,
        deps: Deps,
        env: &Env,
        quote: SwapQuote<Adjusted>,
    ) -> StdResult<SwapQuote<Executable>> {
        if quote.swap_amount.amount.is_zero() {
            return Err(StdError::generic_err("Swap amount cannot be zero"));
        }

        let adjusted_quote = get_thorchain_swap_quote(deps, &quote)?;

        if let Some(fees) = adjusted_quote.fees {
            if fees.slippage_bps > quote.maximum_slippage_bps {
                return Err(StdError::generic_err(format!(
                    "Slippage BPS ({}) exceeds maximum allowed ({})",
                    fees.slippage_bps, quote.maximum_slippage_bps
                )));
            }
        }

        if adjusted_quote.expected_amount_out < quote.minimum_receive_amount.amount {
            return Err(StdError::generic_err(format!(
                "Expected amount out ({}) is less than minimum receive amount ({})",
                adjusted_quote.expected_amount_out, quote.minimum_receive_amount.amount
            )));
        }

        if adjusted_quote.recommended_min_amount_in > quote.swap_amount.amount {
            return Err(StdError::generic_err(format!(
                "Recommended min amount in ({}) is greater than swap amount ({})",
                adjusted_quote.recommended_min_amount_in, quote.swap_amount.amount
            )));
        }

        Ok(SwapQuote {
            swap_amount: quote.swap_amount.clone(),
            minimum_receive_amount: quote.minimum_receive_amount.clone(),
            maximum_slippage_bps: quote.maximum_slippage_bps,
            adjustment: quote.adjustment,
            route: SwapRoute::Thorchain(ThorchainRoute {
                latest_swap: Some(StreamingSwap {
                    swap_amount: quote.swap_amount,
                    expected_receive_amount: Coin::new(
                        adjusted_quote.expected_amount_out,
                        quote.minimum_receive_amount.denom.clone(),
                    ),
                    starting_block: env.block.height + 1,
                    streaming_swap_blocks: adjusted_quote.streaming_swap_blocks,
                    memo: adjusted_quote.memo,
                }),
                ..self
            }),
            state: Executable {
                expected_amount_out: Coin::new(
                    adjusted_quote.expected_amount_out,
                    quote.minimum_receive_amount.denom,
                ),
            },
        })
    }

    pub fn execute(
        &self,
        deps: Deps,
        env: &Env,
        swap_amount: &Coin,
        _minimum_receive_amount: &Coin,
    ) -> StdResult<CosmosMsg> {
        let current_swap = if let Some(current_swap) = &self.latest_swap {
            current_swap
        } else {
            return Err(StdError::generic_err("No current swap found to execute"));
        };

        let swap_msg = MsgDeposit {
            memo: current_swap.memo.clone(),
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

pub fn get_thorchain_swap_quote<S>(
    deps: Deps,
    quote: &SwapQuote<S>,
) -> StdResult<ThorchainSwapQuote> {
    ThorchainSwapQuote::get(deps.querier, &SwapQuoteRequest::try_from(quote)?).map_err(|e| {
        StdError::generic_err(format!(
            "Failed to get L1 swap quote with {:?}: {e}",
            quote.route
        ))
    })
}
