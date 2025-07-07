use std::{
    cmp::{max, min},
    collections::HashSet,
    u8, vec,
};

use crate::{
    actions::{action::Action, operation::Operation, swap::SwapAmountAdjustment},
    conditions::{Condition, Threshold},
    core::{Callback, Contract},
    scheduler::{CreateTrigger, SchedulerExecuteMsg},
    statistics::Statistics,
    thorchain::{MsgDeposit, SwapQuote, SwapQuoteRequest},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, Coins, Decimal, Deps, Env, Event, StdError, StdResult, SubMsg,
    Uint128,
};

#[cw_serde]
pub struct StreamingSwap {
    starting_block: u64,
    streaming_interval: u64,
    streaming_quantity: u64,
    swap_amount: Coin,
    expected_receive_amount: Coin,
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
    pub on_complete: Option<Callback>,
    pub scheduler: Addr,
}

fn is_secured_asset(denom: &str) -> bool {
    denom.to_lowercase() == "rune" || denom.contains("-")
}

impl Operation for ThorSwap {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
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

        if self.swap_amount.amount.is_zero() {
            return Err(StdError::generic_err("Swap amount cannot be zero"));
        }

        if self.maximum_slippage_bps > 10_000 {
            return Err(StdError::generic_err(
                "Maximum slippage basis points cannot exceed 10,000",
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

            if max_streaming_quantity > 14_000 {
                return Err(StdError::generic_err(
                    "Maximum streaming quantity cannot exceed 14,000",
                ));
            }
        }

        Ok((Action::ThorSwap(self), vec![], vec![]))
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
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

                    let swap_quote_request = SwapQuoteRequest {
                        from_asset: self.swap_amount.denom.clone(),
                        to_asset: self.minimum_receive_amount.denom.clone(),
                        amount: self.swap_amount.amount,
                        streaming_interval: Uint128::new(
                            // Default to swapping every 3 blocks
                            self.streaming_interval.unwrap_or(3) as u128,
                        ),
                        streaming_quantity: Uint128::new(
                            // Setting this to 0 allows the chain to
                            // calculate the maximum streaming quantity
                            self.max_streaming_quantity.unwrap_or(0) as u128,
                        ),
                        destination: env.contract.address.to_string(),
                        refund_address: env.contract.address.to_string(),
                        affiliate: self
                            .affiliate_code
                            .clone()
                            .map_or_else(std::vec::Vec::new, |c| vec![c]),
                        affiliate_bps: self
                            .affiliate_bps
                            .map_or_else(std::vec::Vec::new, |b| vec![b]),
                    };

                    let quote = SwapQuote::get(deps.querier, &swap_quote_request).map_err(|e| {
                        StdError::generic_err(format!(
                            "Failed to get L1 swap quote with {:#?}: {e}",
                            swap_quote_request
                        ))
                    })?;

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
                            .mul_floor(Decimal::one() + scaled_price_delta)
                    } else {
                        self.swap_amount
                            .amount
                            .mul_floor(Decimal::one() - scaled_price_delta)
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
            return Ok((Action::ThorSwap(self), vec![], vec![]));
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

        if adjusted_quote.expected_amount_out < new_minimum_receive_amount.amount
            || adjusted_quote.recommended_min_amount_in > new_swap_amount.amount
        {
            return Ok((Action::ThorSwap(self), vec![], vec![]));
        }

        let mut messages: Vec<SubMsg> = vec![];
        let events: Vec<Event> = vec![];

        let swap_msg = SubMsg::reply_always(
            MsgDeposit {
                memo: adjusted_quote.memo,
                coins: vec![new_swap_amount.clone()],
                signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
            }
            .into_cosmos_msg()?,
            0,
        )
        .with_payload(to_json_binary(&Statistics {
            swapped: vec![new_swap_amount],
            ..Statistics::default()
        })?);

        messages.push(swap_msg);

        if let Some(Callback {
            msg,
            contract,
            execution_rebate,
        }) = self.on_complete.clone()
        {
            let create_trigger_msg = SubMsg::reply_never(Contract(self.scheduler.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::CreateTrigger(CreateTrigger {
                    condition: Condition::BlocksCompleted(
                        env.block.height + adjusted_quote.streaming_swap_blocks,
                    ),
                    threshold: Threshold::Any,
                    to: contract,
                    msg: msg,
                }))?,
                execution_rebate,
            ));

            messages.push(create_trigger_msg);
        }

        Ok((Action::ThorSwap(self), messages, events))
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::from([self.minimum_receive_amount.denom.clone()]))
    }

    fn balances(&self, _deps: Deps, _env: &Env, _denoms: &HashSet<String>) -> StdResult<Coins> {
        Ok(Coins::default())
    }

    fn withdraw(
        self,
        _deps: Deps,
        _env: &Env,
        _desired: &HashSet<String>,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::ThorSwap(self), vec![], vec![]))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::ThorSwap(self), vec![], vec![]))
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
            "Failed to get L1 swap quote with {:#?}: {e}",
            swap_quote_request
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
