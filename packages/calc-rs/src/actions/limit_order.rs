use std::{cmp::min, collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, Coins, Decimal, Deps, Env, Event, StdError, StdResult, Uint128,
};
use rujira_rs::fin::{
    BookResponse, ConfigResponse, ExecuteMsg, OrderResponse, Price, QueryMsg, Side,
};

use crate::{
    actions::{
        action::Action,
        operation::{StatefulOperation, StatelessOperation},
    },
    core::Contract,
    statistics::Statistics,
    strategy::{StrategyMsg, StrategyMsgPayload},
};

struct LimitOrderEventData {
    pair_address: Addr,
    side: Side,
    price: Price,
    amount: Uint128,
}

impl LimitOrderEventData {
    pub fn to_event(&self, event_type: &str) -> Event {
        Event::new(event_type)
            .add_attribute("pair_address", self.pair_address.to_string())
            .add_attribute("side", self.side.to_string())
            .add_attribute("price", self.price.to_string())
            .add_attribute("amount", self.amount.to_string())
    }
}

enum LimitOrderEvent {
    SetOrderSkipped { reason: String },
    SetOrder(LimitOrderEventData),
    WithdrawOrderSkipped { reason: String },
    WithdrawOrder(LimitOrderEventData),
}

impl From<LimitOrderEvent> for Event {
    fn from(val: LimitOrderEvent) -> Self {
        match val {
            LimitOrderEvent::SetOrderSkipped { reason } => {
                Event::new("set_order_skipped").add_attribute("reason", reason)
            }
            LimitOrderEvent::SetOrder(data) => data.to_event("set_order"),
            LimitOrderEvent::WithdrawOrderSkipped { reason } => {
                Event::new("withdraw_order_skipped").add_attribute("reason", reason)
            }
            LimitOrderEvent::WithdrawOrder(data) => data.to_event("withdraw_order"),
        }
    }
}

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
pub enum OrderPriceStrategy {
    Fixed(Decimal),
    Offset {
        direction: Direction,
        offset: Offset,
        tolerance: Offset,
    },
}

impl OrderPriceStrategy {
    pub fn should_reset(&self, current_price: Decimal, new_price: Decimal) -> bool {
        match self {
            OrderPriceStrategy::Fixed(_) => current_price != new_price,
            OrderPriceStrategy::Offset { tolerance, .. } => {
                let price_delta = current_price.abs_diff(new_price);
                match tolerance {
                    Offset::Exact(value) => price_delta > *value,
                    Offset::Percent(percent) => {
                        current_price.saturating_mul(Decimal::percent(*percent)) < price_delta
                    }
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
        Ok(match self.clone() {
            OrderPriceStrategy::Fixed(price) => price,
            OrderPriceStrategy::Offset {
                direction, offset, ..
            } => {
                let book = deps.querier.query_wasm_smart::<BookResponse>(
                    pair_address.clone(),
                    &QueryMsg::Book {
                        limit: Some(10),
                        offset: None,
                    },
                )?;

                let book_price = if side == &Side::Base {
                    book.base
                } else {
                    book.quote
                }[0]
                .price;

                match offset {
                    Offset::Exact(offset) => match direction {
                        Direction::Above => book_price.saturating_add(offset),
                        Direction::Below => book_price.saturating_sub(offset),
                    },
                    Offset::Percent(offset) => match direction {
                        Direction::Above => book_price
                            .saturating_mul(Decimal::percent(100u64.saturating_add(offset))),
                        Direction::Below => book_price
                            .saturating_mul(Decimal::percent(100u64.saturating_sub(offset))),
                    },
                }
            }
        })
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
    pub messages: Vec<StrategyMsg>,
    pub events: Vec<Event>,
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
    pub fn refresh(self, deps: Deps, env: &Env, config: &LimitOrder) -> StdResult<SetOrder> {
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
    pub messages: Vec<StrategyMsg>,
    pub events: Vec<Event>,
}

#[cw_serde]
pub struct LimitOrderState<S> {
    pub config: LimitOrder,
    pub state: S,
}

impl LimitOrderState<UnsetOrder> {
    pub fn new(config: LimitOrder) -> Self {
        LimitOrderState {
            config,
            state: UnsetOrder {
                remaining: Uint128::zero(),
                withdrawing: Uint128::zero(),
            },
        }
    }

    pub fn set(self, deps: Deps, env: &Env) -> StdResult<LimitOrderState<SettingOrder>> {
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
        let final_offer = min(available, self.config.max_bid_amount.unwrap_or(available));
        let funding = min(balance.amount + self.state.withdrawing, final_offer);

        if funding.is_zero() && !should_reset {
            return Ok(LimitOrderState {
                config: self.config,
                state: SettingOrder {
                    price,
                    offer: Uint128::zero(),
                    messages: vec![],
                    events: vec![LimitOrderEvent::SetOrderSkipped {
                        reason: "No additional funding available and no price reset needed"
                            .to_string(),
                    }
                    .into()],
                },
            });
        }

        let set_order_msg = StrategyMsg::with_payload(
            Contract(self.config.pair_address.clone()).call(
                to_json_binary(&ExecuteMsg::Order((
                    vec![(
                        self.config.side.clone(),
                        Price::Fixed(price),
                        Some(final_offer),
                    )],
                    None,
                )))?,
                vec![Coin::new(funding, self.config.bid_denom.clone())],
            ),
            StrategyMsgPayload {
                statistics: Statistics::default(),
                events: vec![LimitOrderEvent::SetOrder(LimitOrderEventData {
                    pair_address: self.config.pair_address.clone(),
                    side: self.config.side.clone(),
                    price: Price::Fixed(price),
                    amount: final_offer,
                })
                .into()],
            },
        );

        Ok(LimitOrderState {
            config: self.config,
            state: SettingOrder {
                price,
                offer: final_offer,
                messages: vec![set_order_msg],
                events: vec![],
            },
        })
    }
}

impl LimitOrderState<SettingOrder> {
    pub fn execute(self) -> (Vec<StrategyMsg>, Vec<Event>, LimitOrderState<SetOrder>) {
        (
            self.state.messages,
            self.state.events,
            LimitOrderState {
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

impl LimitOrderState<SetOrder> {
    pub fn withdraw(self, _deps: Deps) -> StdResult<LimitOrderState<WithdrawingOrder>> {
        let withdraw_order_message = StrategyMsg::with_payload(
            Contract(self.config.pair_address.clone()).call(
                to_json_binary(&ExecuteMsg::Order((
                    vec![(
                        self.config.side.clone(),
                        Price::Fixed(self.state.price),
                        Some(Uint128::zero()),
                    )],
                    None,
                )))?,
                vec![],
            ),
            StrategyMsgPayload {
                statistics: Statistics {
                    debited: vec![Coin::new(
                        // Weird if this is smaller than 0, but we handle it safely regardless.
                        self.state.offer.saturating_sub(self.state.remaining),
                        self.config.bid_denom.clone(),
                    )],
                    ..Statistics::default()
                },
                events: vec![LimitOrderEvent::WithdrawOrder(LimitOrderEventData {
                    pair_address: self.config.pair_address.clone(),
                    side: self.config.side.clone(),
                    price: Price::Fixed(self.state.price),
                    amount: self.state.remaining,
                })
                .into()],
            },
        );

        Ok(LimitOrderState {
            config: self.config,
            state: WithdrawingOrder {
                withdrawing: self.state.remaining,
                remaining: Uint128::zero(),
                messages: vec![withdraw_order_message],
                events: vec![],
            },
        })
    }

    pub fn saturating_withdraw(self, deps: Deps) -> StdResult<LimitOrderState<WithdrawingOrder>> {
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
            return self.withdraw(deps);
        }

        Ok(LimitOrderState {
            config: self.config,
            state: WithdrawingOrder {
                withdrawing: Uint128::zero(),
                remaining: self.state.remaining,
                messages: vec![],
                events: vec![LimitOrderEvent::WithdrawOrderSkipped {
                    reason: "No change in target price and no filled amount to claim".to_string(),
                }
                .into()],
            },
        })
    }
}

impl LimitOrderState<WithdrawingOrder> {
    pub fn execute(self) -> (Vec<StrategyMsg>, Vec<Event>, LimitOrderState<UnsetOrder>) {
        (
            self.state.messages,
            self.state.events,
            LimitOrderState {
                config: self.config,
                state: UnsetOrder {
                    remaining: self.state.remaining,
                    withdrawing: self.state.withdrawing,
                },
            },
        )
    }
}

#[cw_serde]
pub struct LimitOrder {
    pub pair_address: Addr,
    pub bid_denom: String,
    pub max_bid_amount: Option<Uint128>,
    pub side: Side,
    pub strategy: OrderPriceStrategy,
    pub current_order: Option<StaleOrder>,
}

impl LimitOrder {
    pub fn get_pair(&self, deps: Deps) -> StdResult<ConfigResponse> {
        deps.querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})
    }

    fn execute_unsafe(
        self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        let mut messages = vec![];
        let mut events: Vec<Event> = vec![];

        let order = if let Some(existing_order) = self.current_order.clone() {
            let existing_order_state = LimitOrderState {
                config: self.clone(),
                state: existing_order.refresh(deps, env, &self)?,
            };

            let (withdraw_messages, withdraw_events, withdrawn_order_state) =
                existing_order_state.saturating_withdraw(deps)?.execute();

            messages.extend(withdraw_messages);
            events.extend(withdraw_events);

            withdrawn_order_state
        } else {
            LimitOrderState::new(self)
        };

        let (set_messages, set_events, set_order_state) = order.set(deps, env)?.execute();

        messages.extend(set_messages);
        events.extend(set_events);

        Ok((
            messages,
            events,
            Action::LimitOrder(LimitOrder {
                current_order: Some(set_order_state.state.cached()),
                ..set_order_state.config
            }),
        ))
    }
}

impl StatelessOperation for LimitOrder {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if let Some(amount) = self.max_bid_amount {
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

        Ok((vec![], vec![], Action::LimitOrder(self)))
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        match self.clone().execute_unsafe(deps, env) {
            Ok((action, messages, events)) => (action, messages, events),
            Err(err) => (
                vec![],
                vec![LimitOrderEvent::SetOrderSkipped {
                    reason: err.to_string(),
                }
                .into()],
                Action::LimitOrder(self),
            ),
        }
    }

    fn escrowed(&self, deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        let pair = deps
            .querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})?;

        Ok(HashSet::from([pair.denoms.ask(&self.side).to_string()]))
    }
}

impl StatefulOperation for LimitOrder {
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
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if !desired.contains(&self.bid_denom) {
            return Ok((vec![], vec![], Action::LimitOrder(self)));
        }

        if let Some(existing_order) = self.current_order.clone() {
            let order_state = LimitOrderState {
                config: self.clone(),
                state: existing_order.refresh(deps, env, &self)?,
            };

            let (messages, events, _) = order_state.withdraw(deps)?.execute();

            // We let the confirm stage remove the current order
            Ok((messages, events, Action::LimitOrder(self)))
        } else {
            Ok((
                vec![],
                vec![LimitOrderEvent::SetOrderSkipped {
                    reason: "No current order to withdraw".to_string(),
                }
                .into()],
                Action::LimitOrder(self),
            ))
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if let Some(existing_order) = self.current_order.clone() {
            let order_state = LimitOrderState {
                config: self.clone(),
                state: existing_order.refresh(deps, env, &self)?,
            };

            let (messages, events, _) = order_state.withdraw(deps)?.execute();

            // We let the confirm stage remove the current order
            Ok((messages, events, Action::LimitOrder(self)))
        } else {
            Ok((
                vec![],
                vec![LimitOrderEvent::SetOrderSkipped {
                    reason: "No current order to withdraw".to_string(),
                }
                .into()],
                Action::LimitOrder(self),
            ))
        }
    }

    fn commit(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if let Some(existing_order) = self.current_order.clone() {
            match existing_order.refresh(deps, env, &self) {
                Ok(_) => Ok((vec![], vec![], Action::LimitOrder(self))),
                Err(_) => Ok((
                    vec![],
                    vec![],
                    Action::LimitOrder(LimitOrder {
                        // Wipe the cached order if it does not exist
                        current_order: None,
                        ..self
                    }),
                )),
            }
        } else {
            Ok((vec![], vec![], Action::LimitOrder(self)))
        }
    }
}
