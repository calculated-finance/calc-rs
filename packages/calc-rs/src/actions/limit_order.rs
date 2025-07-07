use std::{cmp::min, collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, Coins, Decimal, Deps, Env, Event, StdError, StdResult, SubMsg,
    Uint128,
};
use rujira_rs::fin::{
    BookResponse, ConfigResponse, ExecuteMsg, OrderResponse, Price, QueryMsg, Side,
};

use crate::{
    actions::{action::Action, operation::Operation},
    constants::UPDATE_STATS_REPLY_ID,
    core::Contract,
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
pub enum OrderPriceStrategy {
    Fixed(Decimal),
    Oracle(i16),
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
            OrderPriceStrategy::Oracle(offset) => deps
                .querier
                .query_wasm_smart::<OrderResponse>(
                    pair_address,
                    &QueryMsg::Order((
                        env.contract.address.to_string(),
                        side.clone(),
                        Price::Oracle(*offset),
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
pub struct LimitOrder {
    pub pair_address: Addr,
    pub bid_denom: String,
    pub bid_amount: Option<Uint128>,
    pub side: Side,
    pub strategy: OrderPriceStrategy,
    pub current_price: Option<Price>,
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
}

impl Operation for LimitOrder {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
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

        Ok((
            Action::SetLimitOrder(LimitOrder {
                current_price: match self.strategy {
                    OrderPriceStrategy::Fixed(price) => Some(Price::Fixed(price)),
                    OrderPriceStrategy::Oracle(offset) => Some(Price::Oracle(offset)),
                    OrderPriceStrategy::Offset { .. } => None,
                },
                ..self
            }),
            vec![],
            vec![], // TODO: set order
        ))
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
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
            OrderPriceStrategy::Fixed(price) => Price::Fixed(price),
            OrderPriceStrategy::Oracle(offset) => Price::Oracle(offset),
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

                Price::Fixed(match offset {
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
                })
            }
        };

        if bid_denom_balance.amount.is_zero() {
            if let Some(current_price) = self.current_price.clone() {
                if new_price == current_price {
                    // We have no more bid denom & we are not adjusting the price
                    return Ok((Action::SetLimitOrder(self), vec![], vec![]));
                }

                if let OrderPriceStrategy::Offset { tolerance, .. } = self.strategy.clone() {
                    if let (Price::Fixed(current_price), Price::Fixed(new_price)) =
                        (current_price, &new_price)
                    {
                        let price_delta = new_price.abs_diff(current_price);

                        let tolerance_threshold = match tolerance {
                            Offset::Exact(tolerance_val) => tolerance_val,
                            Offset::Bps(tolerance_bps) => {
                                current_price.saturating_mul(Decimal::bps(tolerance_bps))
                            }
                        };

                        if price_delta <= tolerance_threshold {
                            // Price change is within tolerance, no need to update the order
                            return Ok((Action::SetLimitOrder(self), vec![], vec![]));
                        }
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

        let new_price_is_valid = match new_price {
            Price::Fixed(price) => price.gt(&Decimal::zero()),
            _ => true,
        };

        if new_price_is_valid && new_bid_amount.gt(&Uint128::zero()) {
            orders.push((self.side.clone(), new_price.clone(), Some(new_bid_amount)));
        }

        let set_order_msg = SubMsg::reply_always(
            Contract(self.pair_address.clone()).call(
                to_json_binary(&ExecuteMsg::Order((orders, None)))?,
                vec![Coin::new(new_bid_amount, self.bid_denom.clone())],
            ),
            UPDATE_STATS_REPLY_ID,
        )
        .with_payload(to_json_binary(
            &if let Some(existing_order) = existing_order {
                self.statistics(deps, &existing_order)?
            } else {
                Statistics::default()
            },
        )?);

        Ok((
            Action::SetLimitOrder(LimitOrder {
                current_price: Some(new_price),
                ..self
            }),
            vec![set_order_msg],
            vec![],
        ))
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
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        if !desired.contains(&self.bid_denom) {
            return Ok((Action::SetLimitOrder(self), vec![], vec![]));
        }

        let existing_order = self.strategy.existing_order(
            deps,
            env,
            &self.pair_address,
            &self.side,
            &self.current_price,
        );

        if let Some(existing_order) = existing_order {
            let withdraw_order_msg = SubMsg::reply_always(
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
                UPDATE_STATS_REPLY_ID,
            )
            .with_payload(to_json_binary(&self.statistics(deps, &existing_order)?)?);

            return Ok((
                Action::SetLimitOrder(LimitOrder {
                    current_price: None,
                    ..self
                }),
                vec![withdraw_order_msg],
                vec![],
            ));
        }

        Ok((Action::SetLimitOrder(self), vec![], vec![]))
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let existing_order = self.strategy.existing_order(
            deps,
            env,
            &self.pair_address,
            &self.side,
            &self.current_price,
        );

        let mut messages = vec![];

        if let Some(existing_order) = existing_order {
            let withdraw_order_msg = SubMsg::reply_always(
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
                UPDATE_STATS_REPLY_ID,
            )
            .with_payload(to_json_binary(&self.statistics(deps, &existing_order)?)?);

            messages.push(withdraw_order_msg);
        }

        Ok((Action::SetLimitOrder(self), messages, vec![]))
    }
}
