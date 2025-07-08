use std::{cmp::min, collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, Coins, Decimal, Deps, Env, Event, StdError, StdResult, Uint128,
};
use rujira_rs::fin::{
    BookResponse, ConfigResponse, ExecuteMsg, OrderResponse, Price, QueryMsg, Side,
};

use crate::{
    actions::{action::Action, operation::Operation},
    conditions::Condition,
    core::Contract,
    scheduler::SchedulerExecuteMsg,
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
    WithdrawOrder(LimitOrderEventData),
}

impl From<LimitOrderEvent> for Event {
    fn from(val: LimitOrderEvent) -> Self {
        match val {
            LimitOrderEvent::SetOrderSkipped { reason } => {
                Event::new("set_order_skipped").add_attribute("reason", reason)
            }
            LimitOrderEvent::SetOrder(data) => data.to_event("set_order"),
            LimitOrderEvent::WithdrawOrder(data) => data.to_event("withdraw_order"),
        }
    }
}

#[cw_serde]
pub enum Direction {
    Up,
    Down,
}

#[cw_serde]
pub enum Offset {
    Exact(Decimal),
    Bps(u64),
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
    pub fn existing_order(
        &self,
        deps: Deps,
        env: &Env,
        pair_address: &Addr,
        side: &Side,
        current_price: &Option<Decimal>,
    ) -> Option<OrderResponse> {
        match self {
            OrderPriceStrategy::Fixed(price) => deps
                .querier
                .query_wasm_smart::<OrderResponse>(
                    pair_address,
                    &QueryMsg::Order((
                        env.contract.address.to_string(),
                        side.clone(),
                        Price::Fixed(*price),
                    )),
                )
                .ok(),
            OrderPriceStrategy::Offset { .. } => {
                if let Some(price) = current_price {
                    deps.querier
                        .query_wasm_smart::<OrderResponse>(
                            pair_address,
                            &QueryMsg::Order((
                                env.contract.address.to_string(),
                                side.clone(),
                                Price::Fixed(*price),
                            )),
                        )
                        .ok()
                } else {
                    None
                }
            }
        }
    }
}

#[cw_serde]
pub struct LimitOrder {
    pub pair_address: Addr,
    pub bid_denom: String,
    pub bid_amount: Option<Uint128>,
    pub side: Side,
    pub strategy: OrderPriceStrategy,
    pub current_price: Option<Decimal>,
    pub scheduler: Addr,
    pub execution_rebate: Vec<Coin>,
}

impl LimitOrder {
    pub fn get_pair(&self, deps: Deps) -> StdResult<ConfigResponse> {
        deps.querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})
    }

    fn statistics(&self, deps: Deps, order: &OrderResponse) -> StdResult<Statistics> {
        let mut statistics = Statistics {
            swapped: vec![Coin::new(
                order.offer.abs_diff(order.remaining),
                self.bid_denom.clone(),
            )],
            ..Statistics::default()
        };

        if order.filled.gt(&Uint128::zero()) {
            let pair = self.get_pair(deps)?;
            statistics.filled = vec![Coin::new(order.filled, pair.denoms.ask(&order.side))];
        }

        Ok(statistics)
    }

    fn execute_unsafe(
        self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        let existing_order = self.strategy.existing_order(
            deps,
            env,
            &self.pair_address,
            &self.side,
            &self.current_price,
        );

        let bid_denom_balance = deps
            .querier
            .query_balance(env.contract.address.clone(), self.bid_denom.clone())?;

        let new_price = match self.strategy.clone() {
            OrderPriceStrategy::Fixed(price) => price,
            OrderPriceStrategy::Offset {
                direction, offset, ..
            } => {
                let book = deps.querier.query_wasm_smart::<BookResponse>(
                    self.pair_address.clone(),
                    &QueryMsg::Book {
                        limit: Some(1),
                        offset: None,
                    },
                )?;

                let book_price = if self.side == Side::Base {
                    book.base
                } else {
                    book.quote
                }[0]
                .price;

                match offset {
                    Offset::Exact(offset) => match direction {
                        Direction::Up => book_price.saturating_add(offset),
                        Direction::Down => book_price.saturating_sub(offset),
                    },
                    Offset::Bps(offset) => match direction {
                        Direction::Up => book_price
                            .saturating_mul(Decimal::one().saturating_add(Decimal::bps(offset))),
                        Direction::Down => book_price
                            .saturating_mul(Decimal::one().saturating_sub(Decimal::bps(offset))),
                    },
                }
            }
        };

        if bid_denom_balance.amount.is_zero() {
            if let Some(current_price) = self.current_price {
                if new_price == current_price {
                    return Ok((
                        vec![],
                        vec![LimitOrderEvent::SetOrderSkipped {
                            reason: "No balance available to deposit".to_string(),
                        }
                        .into()],
                        Action::SetLimitOrder(self),
                    ));
                }

                if let OrderPriceStrategy::Offset { tolerance, .. } = self.strategy.clone() {
                    let price_delta = new_price.abs_diff(current_price);

                    let tolerance_threshold = match tolerance {
                        Offset::Exact(tolerance_val) => tolerance_val,
                        Offset::Bps(tolerance_bps) => {
                            current_price.saturating_mul(Decimal::bps(tolerance_bps))
                        }
                    };

                    if price_delta <= tolerance_threshold {
                        return Ok((
                            vec![],
                            vec![LimitOrderEvent::SetOrderSkipped {
                                reason: "Current price is within deviation tolerance".to_string(),
                            }
                            .into()],
                            Action::SetLimitOrder(self),
                        ));
                    }
                }
            }
        }

        let remaining = bid_denom_balance.amount
            + existing_order
                .clone()
                .map_or(Uint128::zero(), |o| o.remaining);

        let new_bid_amount = min(self.bid_amount.unwrap_or(remaining), remaining);

        let mut orders = if let Some(o) = existing_order.clone() {
            vec![(o.side, o.price, Some(Uint128::zero()))]
        } else {
            vec![]
        };

        if new_price.gt(&Decimal::zero()) && new_bid_amount.gt(&Uint128::zero()) {
            orders.push((
                self.side.clone(),
                Price::Fixed(new_price),
                Some(new_bid_amount),
            ));
        }

        let set_order_msg = StrategyMsg::with_payload(
            Contract(self.pair_address.clone()).call(
                to_json_binary(&ExecuteMsg::Order((orders, None)))?,
                vec![Coin::new(new_bid_amount, self.bid_denom.clone())],
            ),
            StrategyMsgPayload {
                statistics: if let Some(existing_order) = existing_order {
                    self.statistics(deps, &existing_order)?
                } else {
                    Statistics::default()
                },
                events: vec![LimitOrderEvent::SetOrder(LimitOrderEventData {
                    pair_address: self.pair_address.clone(),
                    side: self.side.clone(),
                    price: Price::Fixed(new_price),
                    amount: new_bid_amount,
                })
                .into()],
                ..StrategyMsgPayload::default()
            },
        );

        let create_trigger_msg =
            StrategyMsg::without_payload(Contract(self.scheduler.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::Create(Condition::LimitOrderFilled {
                    pair_address: self.pair_address.clone(),
                    owner: env.contract.address.clone(),
                    side: self.side.clone(),
                    price: Price::Fixed(new_price),
                    rate: new_price,
                }))?,
                vec![],
            ));

        Ok((
            vec![set_order_msg, create_trigger_msg],
            vec![],
            Action::SetLimitOrder(LimitOrder {
                current_price: Some(new_price),
                ..self
            }),
        ))
    }
}

impl Operation for LimitOrder {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if let Some(amount) = self.bid_amount {
            if amount.lt(&Uint128::new(1_000)) {
                return Err(StdError::generic_err(
                    "Bid amount cannot be less than 1,000",
                ));
            }
        }

        if self.current_price.is_some() {
            return Err(StdError::generic_err(
                "Cannot create limit order with a current price set.",
            ));
        }

        Ok((vec![], vec![], Action::SetLimitOrder(self)))
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
                Action::SetLimitOrder(self),
            ),
        }
    }

    fn escrowed(&self, deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        let pair = deps
            .querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})?;

        Ok(HashSet::from([pair.denoms.ask(&self.side).to_string()]))
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        if !denoms.contains(&self.bid_denom) {
            return Ok(Coins::default());
        }

        let existing_order = self.strategy.existing_order(
            deps,
            env,
            &self.pair_address,
            &self.side,
            &self.current_price,
        );

        let pair = deps
            .querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})?;

        Ok(existing_order.map_or(Ok(Coins::default()), |o| {
            Coins::try_from(vec![
                Coin::new(o.remaining, self.bid_denom.clone()),
                Coin::new(o.filled, pair.denoms.ask(&self.side)),
            ])
        })?)
    }

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if !desired.contains(&self.bid_denom) {
            return Ok((vec![], vec![], Action::SetLimitOrder(self)));
        }

        let existing_order = self.strategy.existing_order(
            deps,
            env,
            &self.pair_address,
            &self.side,
            &self.current_price,
        );

        if let Some(existing_order) = existing_order {
            let withdraw_order_msg = StrategyMsg::with_payload(
                Contract(self.pair_address.clone()).call(
                    to_json_binary(&ExecuteMsg::Order((
                        vec![(
                            self.side.clone(),
                            existing_order.price.clone(),
                            Some(Uint128::zero()),
                        )],
                        None,
                    )))?,
                    vec![],
                ),
                StrategyMsgPayload {
                    statistics: self.statistics(deps, &existing_order)?,
                    events: vec![LimitOrderEvent::WithdrawOrder(LimitOrderEventData {
                        pair_address: self.pair_address.clone(),
                        side: self.side.clone(),
                        price: existing_order.price.clone(),
                        amount: existing_order.remaining,
                    })
                    .into()],
                    ..StrategyMsgPayload::default()
                },
            );

            return Ok((
                vec![withdraw_order_msg],
                vec![],
                Action::SetLimitOrder(LimitOrder {
                    current_price: None,
                    ..self
                }),
            ));
        }

        Ok((
            vec![],
            vec![],
            Action::SetLimitOrder(LimitOrder {
                current_price: None,
                ..self
            }),
        ))
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        let existing_order = self.strategy.existing_order(
            deps,
            env,
            &self.pair_address,
            &self.side,
            &self.current_price,
        );

        let mut messages = vec![];

        if let Some(existing_order) = existing_order {
            let withdraw_order_msg = StrategyMsg::with_payload(
                Contract(self.pair_address.clone()).call(
                    to_json_binary(&ExecuteMsg::Order((
                        vec![(
                            self.side.clone(),
                            existing_order.price.clone(),
                            Some(Uint128::zero()),
                        )],
                        None,
                    )))?,
                    vec![],
                ),
                StrategyMsgPayload {
                    statistics: self.statistics(deps, &existing_order)?,
                    events: vec![LimitOrderEvent::WithdrawOrder(LimitOrderEventData {
                        pair_address: self.pair_address.clone(),
                        side: self.side.clone(),
                        price: existing_order.price.clone(),
                        amount: existing_order.remaining,
                    })
                    .into()],
                    ..StrategyMsgPayload::default()
                },
            );

            messages.push(withdraw_order_msg);
        }

        Ok((
            messages,
            vec![],
            Action::SetLimitOrder(LimitOrder {
                current_price: None,
                ..self
            }),
        ))
    }
}
