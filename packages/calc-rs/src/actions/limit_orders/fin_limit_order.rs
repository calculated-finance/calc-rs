use std::{cmp::min, collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, Coins, CosmosMsg, Decimal, Deps, Env, StdError, StdResult, Uint128,
};
use rujira_rs::fin::{
    BookResponse, ConfigResponse, ExecuteMsg, OrderResponse, Price, QueryMsg, Side,
};

use crate::{
    actions::action::Action,
    core::Contract,
    manager::Affiliate,
    operation::{Operation, StatefulOperation},
};

#[cw_serde]
pub enum Direction {
    Above,
    Below,
}

#[cw_serde]
pub enum Offset {
    Exact(Decimal),
    Percent(u64),
}

#[cw_serde]
pub enum PriceStrategy {
    Fixed(Decimal),
    Offset {
        direction: Direction,
        offset: Offset,
        tolerance: Option<Offset>,
    },
}

impl PriceStrategy {
    pub fn should_reset(&self, current_price: Decimal, new_price: Decimal) -> bool {
        match self {
            PriceStrategy::Fixed(_) => current_price != new_price,
            PriceStrategy::Offset { tolerance, .. } => {
                if let Some(tolerance) = tolerance {
                    let price_delta = current_price.abs_diff(new_price);
                    match tolerance {
                        Offset::Exact(value) => price_delta > *value,
                        Offset::Percent(percent) => {
                            current_price.saturating_mul(Decimal::percent(*percent)) < price_delta
                        }
                    }
                } else {
                    current_price != new_price
                }
            }
        }
    }

    pub fn get_new_price(
        &self,
        deps: Deps,
        pair_address: &Addr,
        side: &Side,
    ) -> StdResult<Decimal> {
        Ok(match self {
            PriceStrategy::Fixed(price) => price.clone(),
            PriceStrategy::Offset {
                direction, offset, ..
            } => {
                let book_response = deps.querier.query_wasm_smart::<BookResponse>(
                    pair_address.clone(),
                    &QueryMsg::Book {
                        limit: Some(10),
                        offset: None,
                    },
                )?;

                let book = if side == &Side::Base {
                    book_response.base
                } else {
                    book_response.quote
                };

                if book.is_empty() {
                    return Err(StdError::generic_err("Order book is empty"));
                }

                let price = book[0].price;

                match offset.clone() {
                    Offset::Exact(offset) => match direction {
                        Direction::Above => price.saturating_add(offset),
                        Direction::Below => price.saturating_sub(offset),
                    },
                    Offset::Percent(offset) => {
                        match direction {
                            Direction::Above => price
                                .saturating_mul(Decimal::percent(100u64.saturating_add(offset))),
                            Direction::Below => price
                                .saturating_mul(Decimal::percent(100u64.saturating_sub(offset))),
                        }
                    }
                }
            }
        })
    }
}

#[cw_serde]
pub struct FinLimitOrder {
    pub pair_address: Addr,
    pub bid_denom: String,
    pub bid_amount: Option<Uint128>,
    pub side: Side,
    pub strategy: PriceStrategy,
    pub current_order: Option<StaleOrder>,
}

impl FinLimitOrder {
    pub fn get_pair(&self, deps: Deps) -> StdResult<ConfigResponse> {
        deps.querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})
    }

    fn execute_unsafe(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Action)> {
        let mut messages = vec![];

        let order = if let Some(existing_order) = self.current_order.clone() {
            let refreshed_order = existing_order.refresh(deps, env, &self)?;

            let existing_order_state = FinLimitOrderState {
                config: self,
                state: refreshed_order,
            };

            let (withdraw_messages, withdrawn_order_state) =
                existing_order_state.saturating_withdraw(deps)?.execute();

            messages.extend(withdraw_messages);

            withdrawn_order_state
        } else {
            FinLimitOrderState::new(self)
        };

        let (set_messages, set_order_state) = order.set(deps, env)?.execute();

        messages.extend(set_messages);

        Ok((
            messages,
            Action::LimitOrder(FinLimitOrder {
                current_order: Some(set_order_state.state.cached()),
                ..set_order_state.config
            }),
        ))
    }
}

#[cw_serde]
pub struct UnsetOrder {
    remaining: Uint128,
    withdrawing: Uint128,
}

#[cw_serde]
pub struct SettingOrder {
    pub price: Decimal,
    pub offer: Uint128,
    pub messages: Vec<CosmosMsg>,
}

#[cw_serde]
pub struct SetOrder {
    pub price: Decimal,
    pub offer: Uint128,
    pub remaining: Uint128,
    pub filled: Uint128,
}

#[cw_serde]
pub struct StaleOrder {
    pub price: Decimal,
}

impl StaleOrder {
    pub fn refresh(self, deps: Deps, env: &Env, config: &FinLimitOrder) -> StdResult<SetOrder> {
        let order = deps.querier.query_wasm_smart::<OrderResponse>(
            config.pair_address.clone(),
            &QueryMsg::Order((
                env.contract.address.to_string(),
                config.side.clone(),
                Price::Fixed(self.price),
            )),
        )?;

        Ok(SetOrder {
            price: self.price,
            offer: order.offer,
            remaining: order.remaining,
            filled: order.filled,
        })
    }
}

impl SetOrder {
    pub fn cached(self) -> StaleOrder {
        StaleOrder { price: self.price }
    }
}

#[cw_serde]
pub struct WithdrawingOrder {
    pub withdrawing: Uint128,
    pub remaining: Uint128,
    pub messages: Vec<CosmosMsg>,
    pub new_price: Option<Decimal>,
}

#[cw_serde]
pub struct FinLimitOrderState<S> {
    pub config: FinLimitOrder,
    pub state: S,
}

impl FinLimitOrderState<UnsetOrder> {
    pub fn new(config: FinLimitOrder) -> Self {
        FinLimitOrderState {
            config,
            state: UnsetOrder {
                remaining: Uint128::zero(),
                withdrawing: Uint128::zero(),
            },
        }
    }

    pub fn set(self, deps: Deps, env: &Env) -> StdResult<FinLimitOrderState<SettingOrder>> {
        let price = self.config.strategy.get_new_price(
            deps,
            &self.config.pair_address,
            &self.config.side,
        )?;

        let should_reset = if let Some(current_order) = &self.config.current_order {
            self.config
                .strategy
                .should_reset(current_order.price, price)
        } else {
            true
        };

        let balance = deps
            .querier
            .query_balance(env.contract.address.clone(), self.config.bid_denom.clone())?;

        let available = balance.amount + self.state.withdrawing + self.state.remaining;
        let final_offer = min(available, self.config.bid_amount.unwrap_or(available));
        let funding = min(balance.amount + self.state.withdrawing, final_offer);

        if funding.is_zero() && !should_reset {
            return Ok(FinLimitOrderState {
                config: self.config,
                state: SettingOrder {
                    price,
                    offer: Uint128::zero(),
                    messages: vec![],
                },
            });
        }

        let set_order_msg = Contract(self.config.pair_address.clone()).call(
            to_json_binary(&ExecuteMsg::Order((
                vec![(
                    self.config.side.clone(),
                    Price::Fixed(price),
                    Some(final_offer),
                )],
                None,
            )))?,
            vec![Coin::new(funding, self.config.bid_denom.clone())],
        );

        Ok(FinLimitOrderState {
            config: self.config,
            state: SettingOrder {
                price,
                offer: final_offer,
                messages: vec![set_order_msg],
            },
        })
    }
}

impl FinLimitOrderState<SettingOrder> {
    pub fn execute(self) -> (Vec<CosmosMsg>, FinLimitOrderState<SetOrder>) {
        (
            self.state.messages,
            FinLimitOrderState {
                config: self.config,
                state: SetOrder {
                    price: self.state.price,
                    offer: self.state.offer,
                    remaining: self.state.offer,
                    filled: Uint128::zero(),
                },
            },
        )
    }
}

impl FinLimitOrderState<SetOrder> {
    pub fn withdraw(self) -> StdResult<FinLimitOrderState<WithdrawingOrder>> {
        let withdraw_order_message = Contract(self.config.pair_address.clone()).call(
            to_json_binary(&ExecuteMsg::Order((
                vec![(
                    self.config.side.clone(),
                    Price::Fixed(self.state.price),
                    Some(Uint128::zero()),
                )],
                None,
            )))?,
            vec![],
        );

        Ok(FinLimitOrderState {
            config: self.config,
            state: WithdrawingOrder {
                withdrawing: self.state.remaining,
                remaining: Uint128::zero(),
                new_price: None,
                messages: vec![withdraw_order_message],
            },
        })
    }

    pub fn saturating_withdraw(
        self,
        deps: Deps,
    ) -> StdResult<FinLimitOrderState<WithdrawingOrder>> {
        let new_price = self.config.strategy.get_new_price(
            deps,
            &self.config.pair_address,
            &self.config.side,
        )?;

        let should_withdraw = self.state.filled.gt(&Uint128::zero())
            || self
                .config
                .strategy
                .should_reset(self.state.price, new_price);

        if should_withdraw {
            let withdrawing_order = self.withdraw()?;

            return Ok(FinLimitOrderState {
                state: WithdrawingOrder {
                    new_price: Some(new_price),
                    ..withdrawing_order.state
                },
                ..withdrawing_order
            });
        }

        Ok(FinLimitOrderState {
            config: self.config,
            state: WithdrawingOrder {
                withdrawing: Uint128::zero(),
                remaining: self.state.remaining,
                new_price: Some(new_price),
                messages: vec![],
            },
        })
    }
}

impl FinLimitOrderState<WithdrawingOrder> {
    pub fn execute(self) -> (Vec<CosmosMsg>, FinLimitOrderState<UnsetOrder>) {
        (
            self.state.messages,
            FinLimitOrderState {
                config: self.config,
                state: UnsetOrder {
                    remaining: self.state.remaining,
                    withdrawing: self.state.withdrawing,
                },
            },
        )
    }
}

impl Operation<Action> for FinLimitOrder {
    fn init(self, _deps: Deps, _env: &Env, _affiliates: &[Affiliate]) -> StdResult<Action> {
        if let Some(amount) = self.bid_amount {
            if amount.lt(&Uint128::new(1_000)) {
                return Err(StdError::generic_err(
                    "Bid amount cannot be less than 1,000",
                ));
            }
        }

        if self.current_order.is_some() {
            return Err(StdError::generic_err(
                "Cannot initialise a limit order action with a current price already set.",
            ));
        }

        Ok(Action::LimitOrder(self))
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<CosmosMsg>, Action) {
        match self.clone().execute_unsafe(deps, env) {
            Ok((messages, action)) => (messages, action),
            Err(_) => (vec![], Action::LimitOrder(self)),
        }
    }

    fn denoms(&self, deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        let pair = deps
            .querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})?;

        Ok(HashSet::from([
            pair.denoms.base().to_string(),
            pair.denoms.quote().to_string(),
        ]))
    }
}

impl StatefulOperation<Action> for FinLimitOrder {
    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        let pair = deps
            .querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})?;

        if !denoms.contains(pair.denoms.base()) && !denoms.contains(pair.denoms.quote()) {
            return Ok(Coins::default());
        }

        let (remaining, filled) = if let Some(existing_order) = self.current_order.clone() {
            let order_state = existing_order.refresh(deps, env, self)?;
            (order_state.remaining, order_state.filled)
        } else {
            (Uint128::zero(), Uint128::zero())
        };

        Ok(Coins::try_from(vec![
            Coin::new(remaining, self.bid_denom.clone()),
            Coin::new(filled, pair.denoms.ask(&self.side)),
        ])?)
    }

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Vec<CosmosMsg>, Action)> {
        if !desired.contains(&self.bid_denom) {
            return Ok((vec![], Action::LimitOrder(self)));
        }

        if let Some(existing_order) = self.current_order.clone() {
            let order_state = FinLimitOrderState {
                config: self.clone(),
                state: existing_order.refresh(deps, env, &self)?,
            };

            let (messages, _) = order_state.withdraw()?.execute();

            // We let the confirm stage remove the current order
            Ok((messages, Action::LimitOrder(self)))
        } else {
            Ok((vec![], Action::LimitOrder(self)))
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Action)> {
        if let Some(existing_order) = self.current_order.clone() {
            let order_state = FinLimitOrderState {
                config: self.clone(),
                state: existing_order.refresh(deps, env, &self)?,
            };

            let (messages, _) = order_state.withdraw()?.execute();

            // We let the commit stage remove the current order
            Ok((messages, Action::LimitOrder(self)))
        } else {
            Ok((vec![], Action::LimitOrder(self)))
        }
    }

    fn commit(self, deps: Deps, env: &Env) -> StdResult<Action> {
        if let Some(existing_order) = self.current_order.clone() {
            match existing_order.refresh(deps, env, &self) {
                Ok(_) => Ok(Action::LimitOrder(self)),
                Err(_) => Ok(Action::LimitOrder(FinLimitOrder {
                    // Wipe the cached order if it does not exist
                    current_order: None,
                    ..self
                })),
            }
        } else {
            Ok(Action::LimitOrder(self))
        }
    }
}
