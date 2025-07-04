use std::{cmp::min, collections::HashSet, u8, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, Coins, Decimal, Deps, Env, StdError, StdResult, SubMsg, Uint128,
};
use rujira_rs::fin::{
    BookResponse, ConfigResponse, ExecuteMsg, OrderResponse, Price, QueryMsg, Side,
};

use crate::{
    actions::{action::Action, operation::Operation},
    conditions::Condition,
    core::Contract,
    events::DomainEvent,
    statistics::Statistics,
};

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
pub enum SwapAdjustment {
    Fixed,
    LinearScalar {
        base_receive_amount: Coin,
        minimum_swap_amount: Option<Coin>,
        scalar: Decimal,
    },
}

#[cw_serde]
pub enum OrderPriceStrategy {
    Fixed {
        price: Decimal,
    },
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
        current_price: &Option<Price>,
    ) -> Option<OrderResponse> {
        match self {
            OrderPriceStrategy::Fixed { price } => deps
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
            OrderPriceStrategy::Offset { .. } => current_price.clone().and_then(|price| {
                deps.querier
                    .query_wasm_smart::<OrderResponse>(
                        pair_address,
                        &QueryMsg::Order((env.contract.address.to_string(), side.clone(), price)),
                    )
                    .ok()
            }),
        }
    }
}

#[cw_serde]
pub struct OrderAction {
    pub pair_address: Addr,
    pub bid_denom: String,
    pub bid_amount: Option<Uint128>,
    pub side: Side,
    pub strategy: OrderPriceStrategy,
    pub current_price: Option<Price>,
}

impl OrderAction {
    pub fn get_pair(&self, deps: Deps) -> StdResult<ConfigResponse> {
        deps.querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})
    }
}

impl Operation for OrderAction {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<Action> {
        if let Some(amount) = self.bid_amount {
            if amount.lt(&Uint128::new(1_000)) {
                return Err(StdError::generic_err(
                    "Bid amount cannot be less than 1,000",
                ));
            }
        }

        if let Some(price) = self.current_price.clone() {
            match price {
                Price::Fixed(price) => {
                    if price.is_zero() {
                        return Err(StdError::generic_err("Fixed price cannot be zero"));
                    }
                }
                Price::Oracle(_) => {}
            }
        }

        Ok(Action::Order(OrderAction {
            current_price: match self.strategy {
                OrderPriceStrategy::Fixed { price } => Some(Price::Fixed(price)),
                OrderPriceStrategy::Offset { .. } => None,
            },
            ..self
        }))
    }

    fn condition(&self, env: &Env) -> Option<Condition> {
        self.current_price
            .clone()
            .map(|price| Condition::LimitOrderFilled {
                pair_address: self.pair_address.clone(),
                owner: env.contract.address.clone(),
                side: self.side.clone(),
                price,
            })
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<DomainEvent>)> {
        let mut messages: Vec<SubMsg> = vec![];
        let events: Vec<DomainEvent> = vec![];

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

        let remaining = bid_denom_balance.amount
            + existing_order
                .clone()
                .map_or(Uint128::zero(), |o| o.remaining);

        let new_rate = match self.strategy.clone() {
            OrderPriceStrategy::Fixed { price } => price,
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

        let new_price = Price::Fixed(new_rate);
        let new_bid_amount = min(self.bid_amount.unwrap_or(remaining), remaining);

        let mut orders = if let Some(o) = existing_order.clone() {
            vec![(o.side, o.price, Some(Uint128::zero()))]
        } else {
            vec![]
        };

        if new_bid_amount.gt(&Uint128::zero()) && new_rate.gt(&Decimal::zero()) {
            orders.push((self.side.clone(), new_price.clone(), Some(new_bid_amount)));
        }

        let set_order_msg = SubMsg::reply_always(
            Contract(self.pair_address.clone()).call(
                to_json_binary(&ExecuteMsg::Order((orders, None)))?,
                vec![Coin::new(new_bid_amount, self.bid_denom.clone())],
            ),
            0,
        )
        .with_payload(to_json_binary(&Statistics {
            filled: if let Some(existing_order) = existing_order {
                if existing_order.filled.gt(&Uint128::zero()) {
                    let pair = self.get_pair(deps)?;
                    vec![Coin::new(
                        existing_order.filled,
                        pair.denoms.ask(&self.side),
                    )]
                } else {
                    vec![]
                }
            } else {
                vec![]
            },
            ..Statistics::default()
        })?);

        messages.push(set_order_msg);

        Ok((
            Action::Order(OrderAction {
                current_price: Some(new_price),
                ..self
            }),
            messages,
            events,
        ))
    }

    fn update(
        self,
        deps: Deps,
        env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<DomainEvent>)> {
        match update {
            Action::Order(update) => {
                let (action, messages, events) = update.init(deps, env)?.execute(deps, env)?;
                Ok((action, messages, events))
            }
            _ => Err(StdError::generic_err(
                "Cannot update order action with non-order action",
            )),
        }
    }

    fn escrowed(&self, deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        let pair = deps
            .querier
            .query_wasm_smart::<ConfigResponse>(self.pair_address.clone(), &QueryMsg::Config {})?;

        Ok(HashSet::from([pair.denoms.ask(&self.side).to_string()]))
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &[String]) -> StdResult<Coins> {
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
        desired: &Coins,
    ) -> StdResult<(Action, Vec<SubMsg>, Coins)> {
        let mut withdrawn = Coins::default();
        let mut messages: Vec<SubMsg> = vec![];

        let desired_bid_denom_amount = desired.amount_of(&self.bid_denom);

        if desired_bid_denom_amount.is_zero() {
            return Ok((Action::Order(self), messages, withdrawn));
        }

        let existing_order = self.strategy.existing_order(
            deps,
            env,
            &self.pair_address,
            &self.side,
            &self.current_price,
        );

        if let Some(existing_order) = existing_order {
            let withdrawal_amount = min(existing_order.remaining, desired_bid_denom_amount);
            let new_bid_amount = existing_order.remaining.saturating_sub(withdrawal_amount);

            let withdraw_order_msg = SubMsg::reply_always(
                Contract(self.pair_address.clone()).call(
                    to_json_binary(&ExecuteMsg::Order((
                        vec![(
                            self.side.clone(),
                            existing_order.price,
                            Some(new_bid_amount),
                        )],
                        None,
                    )))?,
                    vec![],
                ),
                0,
            )
            .with_payload(to_json_binary(&Statistics {
                filled: if existing_order.filled.gt(&Uint128::zero()) {
                    let pair = self.get_pair(deps)?;
                    vec![Coin::new(
                        existing_order.filled,
                        pair.denoms.ask(&self.side),
                    )]
                } else {
                    vec![]
                },
                ..Statistics::default()
            })?);

            messages.push(withdraw_order_msg);
            withdrawn.add(Coin::new(withdrawal_amount, self.bid_denom.clone()))?;
        }

        Ok((Action::Order(self), messages, withdrawn))
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<DomainEvent>)> {
        let order = self.strategy.existing_order(
            deps,
            env,
            &self.pair_address,
            &self.side,
            &self.current_price,
        );

        let mut messages = vec![];

        if let Some(order) = order {
            let withdraw_order_msg = SubMsg::reply_always(
                Contract(self.pair_address.clone()).call(
                    to_json_binary(&ExecuteMsg::Order((
                        vec![(self.side.clone(), order.price, None)],
                        None,
                    )))?,
                    vec![],
                ),
                0,
            );

            messages.push(withdraw_order_msg);
        }

        Ok((Action::Order(self), messages, vec![]))
    }
}
