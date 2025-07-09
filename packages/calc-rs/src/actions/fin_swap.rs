use std::{
    cmp::{max, min},
    collections::HashSet,
    vec,
};

use crate::{
    actions::{action::Action, operation::StatelessOperation, optimal_swap::SwapAmountAdjustment},
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
    SwapSkipped {
        reason: String,
    },
    Swap {
        swap_amount: Coin,
        expected_receive_amount: Coin,
    },
}

impl From<FinSwapEvent> for Event {
    fn from(val: FinSwapEvent) -> Self {
        match val {
            FinSwapEvent::SwapSkipped { reason } => {
                Event::new("fin_swap_skipped").add_attribute("reason", reason)
            }
            FinSwapEvent::Swap {
                swap_amount,
                expected_receive_amount,
            } => Event::new("fin_swap_attempted")
                .add_attribute("swap_amount", swap_amount.to_string())
                .add_attribute(
                    "expected_receive_amount",
                    expected_receive_amount.to_string(),
                ),
        }
    }
}

#[cw_serde]
pub struct FinSwap {
    pub pair_address: Addr,
    pub swap_amount: Coin,
    pub minimum_receive_amount: Coin,
    pub maximum_slippage_bps: u128,
    pub adjustment: SwapAmountAdjustment,
}

impl FinSwap {
    pub fn execute_unsafe(
        self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        let (new_swap_amount, new_minimum_receive_amount) = match self.adjustment.clone() {
            SwapAmountAdjustment::Fixed => {
                let swap_balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), self.swap_amount.denom.clone())?;

                let new_swap_amount = Coin::new(
                    min(swap_balance.amount, self.swap_amount.amount),
                    self.swap_amount.denom.clone(),
                );

                let new_minimum_receive_amount =
                    self.minimum_receive_amount
                        .amount
                        .mul_floor(Decimal::from_ratio(
                            new_swap_amount.amount,
                            self.swap_amount.amount,
                        ));

                (new_swap_amount, new_minimum_receive_amount)
            }
            SwapAmountAdjustment::LinearScalar {
                base_receive_amount,
                minimum_swap_amount,
                scalar,
            } => {
                let swap_balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), self.swap_amount.denom.clone())?;

                let new_swap_amount = Coin::new(
                    min(swap_balance.amount, self.swap_amount.amount),
                    self.swap_amount.denom.clone(),
                );

                let expected_receive_amount = get_expected_amount_out(deps, &self)?;

                let base_price =
                    Decimal::from_ratio(base_receive_amount.amount, self.swap_amount.amount);

                let current_price =
                    Decimal::from_ratio(self.swap_amount.amount, expected_receive_amount.amount);

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
                            .unwrap_or(Coin::new(0u128, self.swap_amount.denom.clone()))
                            .amount,
                    ),
                    self.swap_amount.denom.clone(),
                );

                let new_minimum_receive_amount =
                    self.minimum_receive_amount
                        .amount
                        .mul_ceil(Decimal::from_ratio(
                            new_swap_amount.amount,
                            self.swap_amount.amount,
                        ));

                (new_swap_amount, new_minimum_receive_amount)
            }
        };

        if new_swap_amount.amount.is_zero() {
            return Ok((
                vec![],
                vec![FinSwapEvent::SwapSkipped {
                    reason: "Swap amount after adjustment is zero".to_string(),
                }
                .into()],
                Action::FinSwap(self),
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

        let spot_price = if new_swap_amount.denom == pair.denoms.base() {
            Decimal::one() / mid_price
        } else {
            mid_price
        };

        let expected_amount_out = get_expected_amount_out(
            deps,
            &FinSwap {
                swap_amount: new_swap_amount.clone(),
                minimum_receive_amount: Coin::new(
                    new_minimum_receive_amount,
                    self.minimum_receive_amount.denom.clone(),
                ),
                ..self.clone()
            },
        )?;

        if expected_amount_out.amount < new_minimum_receive_amount {
            return Ok((
                vec![],
                vec![FinSwapEvent::SwapSkipped {
                    reason: format!(
                        "Expected amount out {} for swapping {} is less than adjusted minimum receive amount {}",
                        expected_amount_out.amount, new_swap_amount.amount, new_minimum_receive_amount
                    ),
                }
                .into()],
                Action::FinSwap(self),
            ));
        }

        let optimal_return_amount = max(
            expected_amount_out.amount,
            new_swap_amount
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

        if slippage_bps.gt(&Uint128::new(self.maximum_slippage_bps)) {
            return Ok((
                vec![],
                vec![FinSwapEvent::SwapSkipped {
                    reason: format!(
                        "Slippage of {} bps exceeds maximum allowed of {} bps",
                        slippage_bps, self.maximum_slippage_bps
                    ),
                }
                .into()],
                Action::FinSwap(self),
            ));
        }

        let swap_msg = StrategyMsg::with_payload(
            Contract(self.pair_address.clone()).call(
                to_json_binary(&ExecuteMsg::Swap(SwapRequest {
                    min_return: Some(new_minimum_receive_amount),
                    to: None,
                    callback: None,
                }))?,
                vec![new_swap_amount.clone()],
            ),
            StrategyMsgPayload {
                statistics: Statistics {
                    swapped: vec![new_swap_amount.clone()],
                    ..Statistics::default()
                },
                events: vec![FinSwapEvent::Swap {
                    swap_amount: new_swap_amount.clone(),
                    expected_receive_amount: expected_amount_out.clone(),
                }
                .into()],
            },
        );

        Ok((vec![swap_msg], vec![], Action::FinSwap(self)))
    }
}

impl StatelessOperation for FinSwap {
    fn init(self, deps: Deps, _env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if self.swap_amount.amount.is_zero() {
            return Err(StdError::generic_err("Swap amount cannot be zero"));
        }

        if self.maximum_slippage_bps > 10_000 {
            return Err(StdError::generic_err(
                "Maximum slippage basis points cannot exceed 10,000",
            ));
        }

        let pair = deps
            .querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})?;

        let denoms = [pair.denoms.base(), pair.denoms.quote()];

        if !denoms.contains(&self.swap_amount.denom.as_str()) {
            return Err(StdError::generic_err(format!(
                "Pair at {} does not support swapping from {}",
                self.pair_address, self.swap_amount.denom
            )));
        }

        if !denoms.contains(&self.minimum_receive_amount.denom.as_str()) {
            return Err(StdError::generic_err(format!(
                "Pair at {} does not support swapping into {}",
                self.pair_address, self.minimum_receive_amount.denom
            )));
        }

        Ok((vec![], vec![], Action::FinSwap(self)))
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        match self.clone().execute_unsafe(deps, env) {
            Ok((action, messages, events)) => (action, messages, events),
            Err(err) => (
                vec![],
                vec![FinSwapEvent::SwapSkipped {
                    reason: err.to_string(),
                }
                .into()],
                Action::FinSwap(self),
            ),
        }
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::from([self.minimum_receive_amount.denom.clone()]))
    }
}

pub fn get_expected_amount_out(deps: Deps, swap: &FinSwap) -> StdResult<Coin> {
    let simulation = deps.querier.query_wasm_smart::<SimulationResponse>(
        swap.pair_address.clone(),
        &QueryMsg::Simulate(swap.swap_amount.clone()),
    )?;

    Ok(Coin::new(
        simulation.returned,
        swap.swap_amount.denom.clone(),
    ))
}
