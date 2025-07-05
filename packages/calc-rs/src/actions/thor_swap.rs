use std::{
    cmp::{max, min},
    collections::HashSet,
    u8, vec,
};

use crate::{
    actions::{action::Action, operation::Operation, swap::SwapAmountAdjustment},
    statistics::Statistics,
    thorchain::{MsgDeposit, SwapQuote, SwapQuoteRequest},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Coin, Coins, Decimal, Deps, Env, Event, StdError, StdResult, SubMsg, Uint128,
};

use crate::conditions::Condition;

#[cw_serde]
pub struct ThorSwap {
    pub swap_amount: Coin,
    pub minimum_receive_amount: Coin,
    pub maximum_slippage_bps: u128,
    pub adjustment: SwapAmountAdjustment,
    pub affiliate_code: Option<String>,
    pub affiliate_bps: Option<u64>,
}

fn is_secured_asset(denom: &str) -> bool {
    denom.to_lowercase() == "rune" || denom.contains("-")
}

impl Operation for ThorSwap {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<Action> {
        if !is_secured_asset(self.swap_amount.denom.as_str()) {
            return Err(StdError::generic_err(
                "Swap amount must be RUNE or a secured asset",
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

        Ok(Action::ThorSwap(self))
    }

    fn condition(&self, env: &Env) -> Option<Condition> {
        Some(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(1_000u128, self.swap_amount.denom.clone()),
        })
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let mut messages: Vec<SubMsg> = vec![];
        let events: Vec<Event> = vec![];

        let (new_swap_amount, new_minimum_receive_amount) = match self.adjustment.clone() {
            SwapAmountAdjustment::Fixed => {
                let swap_balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), self.swap_amount.denom.clone())?;

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

                (new_swap_amount, new_minimum_receive_amount)
            }
            SwapAmountAdjustment::LinearScalar {
                base_receive_amount,
                minimum_swap_amount,
                scalar,
            } => {
                let quote = SwapQuote::get(
                    deps.querier,
                    &SwapQuoteRequest {
                        from_asset: self.swap_amount.denom.clone(),
                        to_asset: self.minimum_receive_amount.denom.clone(),
                        amount: self.swap_amount.amount,
                        streaming_interval: Uint128::zero(),
                        streaming_quantity: Uint128::zero(),
                        destination: env.contract.address.to_string(),
                        refund_address: env.contract.address.to_string(),
                        affiliate: self
                            .affiliate_code
                            .clone()
                            .map_or_else(std::vec::Vec::new, |c| vec![c]),
                        affiliate_bps: self
                            .affiliate_bps
                            .map_or_else(std::vec::Vec::new, |b| vec![b]),
                    },
                )
                .map_err(|e| StdError::generic_err(format!("Failed to get L1 swap quote: {e}")))?;

                let expected_receive_amount = quote.expected_amount_out;

                let base_price =
                    Decimal::from_ratio(base_receive_amount.amount, self.swap_amount.amount);

                let current_price =
                    Decimal::from_ratio(self.swap_amount.amount, expected_receive_amount);

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

                if scaled_swap_amount.is_zero() {
                    return Ok((Action::ThorSwap(self), messages, events));
                }

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

                (new_swap_amount, new_minimum_receive_amount)
            }
        };

        if new_swap_amount.amount.is_zero() {
            return Ok((Action::ThorSwap(self), messages, events));
        }

        let quote = SwapQuote::get(
            deps.querier,
            &SwapQuoteRequest {
                from_asset: new_swap_amount.denom.clone(),
                to_asset: new_minimum_receive_amount.denom.clone(),
                amount: new_swap_amount.amount,
                streaming_interval: Uint128::zero(),
                streaming_quantity: Uint128::zero(),
                destination: env.contract.address.to_string(),
                refund_address: env.contract.address.to_string(),
                affiliate: self
                    .affiliate_code
                    .clone()
                    .map_or_else(std::vec::Vec::new, |c| vec![c]),
                affiliate_bps: self
                    .affiliate_bps
                    .map_or_else(std::vec::Vec::new, |b| vec![b]),
            },
        )
        .map_err(|e| StdError::generic_err(format!("Failed to get L1 swap quote: {e}")))?;

        if quote.expected_amount_out < new_minimum_receive_amount.amount {
            return Ok((Action::ThorSwap(self), messages, events));
        }

        let swap_msg = SubMsg::reply_always(
            MsgDeposit {
                memo: quote.memo,
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

        Ok((Action::ThorSwap(self), messages, events))
    }

    fn update(
        self,
        _deps: Deps,
        _env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        if let Action::ThorSwap(update) = update {
            return Ok((Action::ThorSwap(update), vec![], vec![]));
        } else {
            return Err(StdError::generic_err(
                "Cannot update swap action with non-swap action",
            ));
        }
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::from([self.minimum_receive_amount.denom.clone()]))
    }

    fn balances(&self, _deps: Deps, _env: &Env, _denoms: &[String]) -> StdResult<Coins> {
        Ok(Coins::default())
    }

    fn withdraw(
        &self,
        _deps: Deps,
        _env: &Env,
        _desired: &Coins,
    ) -> StdResult<(Vec<SubMsg>, Coins)> {
        Ok((vec![], Coins::default()))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::ThorSwap(self), vec![], vec![]))
    }
}
