use std::{
    cmp::{max, min},
    collections::HashSet,
    vec,
};

use crate::{
    actions::{action::Action, operation::StatelessOperation, optimal_swap::SwapAmountAdjustment},
    statistics::Statistics,
    strategy::{StrategyMsg, StrategyMsgPayload},
    thorchain::{MsgDeposit, SwapQuote, SwapQuoteRequest},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, Decimal, Deps, Env, Event, StdError, StdResult, Uint128};

pub enum ThorchainSwapEvent {
    SwapSkipped {
        reason: String,
    },
    Swap {
        swap_amount: Coin,
        expected_receive_amount: Coin,
        streaming_swap_blocks: u64,
    },
}

impl From<ThorchainSwapEvent> for Event {
    fn from(val: ThorchainSwapEvent) -> Self {
        match val {
            ThorchainSwapEvent::SwapSkipped { reason } => {
                Event::new("thorchain_swap_skipped").add_attribute("reason", reason)
            }
            ThorchainSwapEvent::Swap {
                swap_amount,
                expected_receive_amount,
                streaming_swap_blocks,
            } => Event::new("thorchain_swap")
                .add_attribute("swap_amount", swap_amount.to_string())
                .add_attribute(
                    "expected_receive_amount",
                    expected_receive_amount.to_string(),
                )
                .add_attribute("streaming_swap_blocks", streaming_swap_blocks.to_string()),
        }
    }
}

#[cw_serde]
pub struct StreamingSwap {
    swap_amount: Coin,
    expected_receive_amount: Coin,
    starting_block: u64,
    streaming_swap_blocks: u64,
}

#[cw_serde]
pub struct ThorSwap {
    pub swap_amount: Coin,
    pub minimum_receive_amount: Coin,
    pub maximum_slippage_bps: u128,
    pub adjustment: SwapAmountAdjustment,
    pub streaming_interval: Option<u64>,
    pub max_streaming_quantity: Option<u64>,
    pub affiliate_code: Option<String>,
    pub affiliate_bps: Option<u64>,
    pub previous_swap: Option<StreamingSwap>,
}

fn is_secured_asset(denom: &str) -> bool {
    denom.to_lowercase() == "rune" || denom.contains("-")
}

impl ThorSwap {
    pub fn execute_unsafe(
        self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        let (new_swap_amount, new_minimum_receive_amount, max_streaming_quantity) =
            match self.adjustment.clone() {
                SwapAmountAdjustment::Fixed => {
                    let swap_balance = deps.querier.query_balance(
                        env.contract.address.clone(),
                        self.swap_amount.denom.clone(),
                    )?;

                    let new_swap_amount = Coin::new(
                        min(swap_balance.amount, self.swap_amount.amount),
                        self.swap_amount.denom.clone(),
                    );

                    let new_minimum_receive_amount = Coin::new(
                        self.minimum_receive_amount
                            .amount
                            .mul_floor(Decimal::from_ratio(
                                new_swap_amount.amount,
                                self.swap_amount.amount,
                            )),
                        self.minimum_receive_amount.denom.clone(),
                    );

                    let quote = get_quote(
                        deps,
                        env,
                        &ThorSwap {
                            swap_amount: new_swap_amount.clone(),
                            minimum_receive_amount: new_minimum_receive_amount.clone(),
                            ..self.clone()
                        },
                    )?;

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
                    let quote = get_quote(deps, env, &self)?;

                    let base_price =
                        Decimal::from_ratio(base_receive_amount.amount, self.swap_amount.amount);

                    let current_price =
                        Decimal::from_ratio(self.swap_amount.amount, quote.expected_amount_out);

                    let price_delta = base_price.abs_diff(current_price) / base_price;
                    let scaled_price_delta = price_delta * scalar;

                    let scaled_swap_amount = if current_price < base_price {
                        self.swap_amount
                            .amount
                            .mul_floor(Decimal::one().saturating_add(scaled_price_delta))
                    } else {
                        self.swap_amount
                            .amount
                            .mul_floor(Decimal::one().saturating_sub(scaled_price_delta))
                    };

                    let new_swap_amount = Coin::new(
                        max(
                            scaled_swap_amount,
                            minimum_swap_amount
                                .clone()
                                .unwrap_or(Coin::new(0u128, self.swap_amount.denom.clone()))
                                .amount,
                        ),
                        self.swap_amount.denom.clone(),
                    );

                    let new_minimum_receive_amount = Coin::new(
                        self.minimum_receive_amount
                            .amount
                            .mul_ceil(Decimal::from_ratio(
                                new_swap_amount.amount,
                                self.swap_amount.amount,
                            )),
                        self.minimum_receive_amount.denom.clone(),
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

        if new_swap_amount.amount.is_zero() {
            return Ok((
                vec![],
                vec![ThorchainSwapEvent::SwapSkipped {
                    reason: "Adjusted swap amount is zero".to_string(),
                }
                .into()],
                Action::ThorSwap(self),
            ));
        }

        let adjusted_quote = get_quote(
            deps,
            env,
            &ThorSwap {
                swap_amount: new_swap_amount.clone(),
                minimum_receive_amount: new_minimum_receive_amount.clone(),
                max_streaming_quantity: Some(max_streaming_quantity),
                ..self.clone()
            },
        )?;

        if let Some(fees) = adjusted_quote.fees {
            if fees.slippage_bps as u128 > self.maximum_slippage_bps {
                return Ok((
                    vec![],
                    vec![ThorchainSwapEvent::SwapSkipped {
                        reason: format!(
                            "Slippage BPS ({}) exceeds maximum allowed ({})",
                            fees.slippage_bps, self.maximum_slippage_bps
                        ),
                    }
                    .into()],
                    Action::ThorSwap(self),
                ));
            }
        }

        if adjusted_quote.expected_amount_out < new_minimum_receive_amount.amount {
            return Ok((
                vec![],
                vec![ThorchainSwapEvent::SwapSkipped {
                    reason: format!(
                        "Expected amount out ({}) is less than adjusted minimum receive amount ({})",
                        adjusted_quote.expected_amount_out,
                        new_minimum_receive_amount.amount
                    ),
                }
                .into()],
                Action::ThorSwap(self),
            ));
        }

        if adjusted_quote.recommended_min_amount_in > new_swap_amount.amount {
            return Ok((
                vec![],
                vec![ThorchainSwapEvent::SwapSkipped {
                    reason: format!(
                        "Recommended min amount in ({}) is greater than adjusted swap amount ({})",
                        adjusted_quote.recommended_min_amount_in, new_swap_amount.amount
                    ),
                }
                .into()],
                Action::ThorSwap(self),
            ));
        }

        let swap_msg = StrategyMsg::with_payload(
            MsgDeposit {
                memo: adjusted_quote.memo,
                coins: vec![new_swap_amount.clone()],
                signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
            }
            .into_cosmos_msg()?,
            StrategyMsgPayload {
                statistics: Statistics {
                    swapped: vec![new_swap_amount.clone()],
                    ..Statistics::default()
                },
                events: vec![ThorchainSwapEvent::Swap {
                    swap_amount: new_swap_amount.clone(),
                    expected_receive_amount: Coin::new(
                        adjusted_quote.expected_amount_out,
                        new_minimum_receive_amount.denom.clone(),
                    ),
                    streaming_swap_blocks: adjusted_quote.streaming_swap_blocks,
                }
                .into()],
            },
        );

        Ok((
            vec![swap_msg],
            vec![],
            Action::ThorSwap(ThorSwap {
                previous_swap: Some(StreamingSwap {
                    swap_amount: new_swap_amount.clone(),
                    expected_receive_amount: Coin::new(
                        adjusted_quote.expected_amount_out,
                        new_minimum_receive_amount.denom.clone(),
                    ),
                    starting_block: env.block.height,
                    streaming_swap_blocks: adjusted_quote.streaming_swap_blocks,
                }),
                ..self
            }),
        ))
    }
}

impl StatelessOperation for ThorSwap {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if self.swap_amount.amount.is_zero() {
            return Err(StdError::generic_err("Swap amount cannot be zero"));
        }

        if self.maximum_slippage_bps > 10_000 {
            return Err(StdError::generic_err(
                "Maximum slippage basis points cannot exceed 10,000",
            ));
        }

        if !is_secured_asset(self.swap_amount.denom.as_str()) {
            return Err(StdError::generic_err(
                "Swap denom must be RUNE or a secured asset",
            ));
        }

        if !is_secured_asset(self.minimum_receive_amount.denom.as_str()) {
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

        // Check that we can get a quote for the swap
        get_quote(_deps, _env, &self)?;

        Ok((vec![], vec![], Action::ThorSwap(self)))
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        match self.clone().execute_unsafe(deps, env) {
            Ok(res) => res,
            Err(err) => (
                vec![],
                vec![ThorchainSwapEvent::SwapSkipped {
                    reason: err.to_string(),
                }
                .into()],
                Action::ThorSwap(self),
            ),
        }
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::from([self.minimum_receive_amount.denom.clone()]))
    }
}

pub fn get_quote(deps: Deps, env: &Env, swap: &ThorSwap) -> StdResult<SwapQuote> {
    let swap_quote_request = SwapQuoteRequest {
        from_asset: swap.swap_amount.denom.clone(),
        to_asset: swap.minimum_receive_amount.denom.clone(),
        amount: swap.swap_amount.amount,
        streaming_interval: Uint128::new(
            // Default to swapping every 3 blocks
            swap.streaming_interval.unwrap_or(3) as u128,
        ),
        streaming_quantity: Uint128::new(
            // Setting this to 0 allows the chain to
            // calculate the maximum streaming quantity
            swap.max_streaming_quantity.unwrap_or(0) as u128,
        ),
        destination: env.contract.address.to_string(),
        refund_address: env.contract.address.to_string(),
        affiliate: swap
            .affiliate_code
            .clone()
            .map_or_else(std::vec::Vec::new, |c| vec![c]),
        affiliate_bps: swap
            .affiliate_bps
            .map_or_else(std::vec::Vec::new, |b| vec![b]),
    };

    let quote = SwapQuote::get(deps.querier, &swap_quote_request).map_err(|e| {
        StdError::generic_err(format!(
            "Failed to get L1 swap quote with {swap_quote_request:#?}: {e}"
        ))
    })?;

    Ok(quote)
}

pub fn get_expected_amount_out(deps: Deps, env: &Env, swap: &ThorSwap) -> StdResult<Coin> {
    let swap_quote = get_quote(deps, env, swap)?;

    Ok(Coin::new(
        swap_quote.expected_amount_out,
        swap.minimum_receive_amount.denom.clone(),
    ))
}
