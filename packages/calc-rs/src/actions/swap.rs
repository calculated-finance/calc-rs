use std::{
    cmp::{max, min},
    collections::HashSet,
    u8, vec,
};

use crate::{
    actions::{action::Action, operation::Operation},
    core::Contract,
    exchanger::{ExchangerExecuteMsg, ExchangerQueryMsg, ExpectedReceiveAmount, Route},
    statistics::Statistics,
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, Coins, Decimal, Deps, Env, Event, StdError, StdResult, SubMsg,
};
use rujira_rs::fin::{ConfigResponse, QueryMsg};

use crate::conditions::Condition;

#[cw_serde]
pub enum SwapAmountAdjustment {
    Fixed,
    LinearScalar {
        base_receive_amount: Coin,
        minimum_swap_amount: Option<Coin>,
        scalar: Decimal,
    },
}

#[cw_serde]
pub struct Swap {
    pub exchange_contract: Addr,
    pub swap_amount: Coin,
    pub minimum_receive_amount: Coin,
    pub maximum_slippage_bps: u128,
    pub adjustment: SwapAmountAdjustment,
    pub route: Option<Route>,
}

impl Operation for Swap {
    fn init(self, deps: Deps, _env: &Env) -> StdResult<Action> {
        if self.swap_amount.amount.is_zero() {
            return Err(StdError::generic_err("Swap amount cannot be zero"));
        }

        if self.maximum_slippage_bps > 10_000 {
            return Err(StdError::generic_err(
                "Maximum slippage basis points cannot exceed 10,000",
            ));
        }

        if let Some(route) = self.route.clone() {
            match route {
                Route::FinMarket { address } => {
                    let pair = deps.querier.query_wasm_smart::<ConfigResponse>(
                        address.clone(),
                        &QueryMsg::Config {},
                    )?;

                    let denoms = [pair.denoms.base(), pair.denoms.quote()];

                    if !denoms.contains(&self.swap_amount.denom.as_str()) {
                        return Err(StdError::generic_err(format!(
                            "Pair at {} does not support swapping from {}",
                            address, self.swap_amount.denom
                        )));
                    }

                    if !denoms.contains(&self.minimum_receive_amount.denom.as_str()) {
                        return Err(StdError::generic_err(format!(
                            "Pair at {} does not support swapping into {}",
                            address, self.minimum_receive_amount.denom
                        )));
                    }
                }
                Route::Thorchain {} => {}
            }
        }

        Ok(Action::Perform(self))
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
                let expected_receive_amount =
                    deps.querier.query_wasm_smart::<ExpectedReceiveAmount>(
                        self.exchange_contract.clone(),
                        &ExchangerQueryMsg::ExpectedReceiveAmount {
                            swap_amount: self.swap_amount.clone(),
                            target_denom: self.swap_amount.denom.clone(),
                            route: None,
                        },
                    )?;

                let base_price =
                    Decimal::from_ratio(base_receive_amount.amount, self.swap_amount.amount);

                let current_price = Decimal::from_ratio(
                    self.swap_amount.amount,
                    expected_receive_amount.receive_amount.amount,
                );

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
                    return Ok((Action::Perform(self), messages, events));
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
            return Ok((Action::Perform(self), messages, events));
        }

        let swap_msg = SubMsg::reply_always(
            Contract(self.exchange_contract.clone()).call(
                to_json_binary(&ExchangerExecuteMsg::Swap {
                    minimum_receive_amount: new_minimum_receive_amount.clone(),
                    maximum_slippage_bps: self.maximum_slippage_bps,
                    route: self.route.clone(),
                    recipient: None,
                    on_complete: None,
                })?,
                vec![new_swap_amount.clone()],
            ),
            0,
        )
        .with_payload(to_json_binary(&Statistics {
            swapped: vec![new_swap_amount],
            ..Statistics::default()
        })?);

        messages.push(swap_msg);

        Ok((Action::Perform(self), messages, events))
    }

    fn update(
        self,
        _deps: Deps,
        _env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        if let Action::Perform(update) = update {
            return Ok((Action::Perform(update), vec![], vec![]));
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
        Ok((Action::Perform(self), vec![], vec![]))
    }
}
