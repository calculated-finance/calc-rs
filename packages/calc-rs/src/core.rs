use std::{
    cmp::{max, min},
    time::Duration,
    u8, vec,
};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, BankMsg, Binary, CheckedFromRatioError, CheckedMultiplyRatioError, Coin,
    CoinsError, CosmosMsg, Decimal, Deps, Env, Instantiate2AddressError, OverflowError, Response,
    StdError, StdResult, Timestamp, Uint128, WasmMsg,
};
use rujira_rs::fin::{
    BookResponse, ConfigResponse, ExecuteMsg, OrderResponse, Price, QueryMsg, Side,
};
use thiserror::Error;

use crate::{
    distributor::DistributorExecuteMsg,
    exchanger::{ExchangeExecuteMsg, ExchangeQueryMsg, ExpectedReceiveAmount, Route},
    manager::{ManagerQueryMsg, Strategy, StrategyStatus},
};

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Instantiate2Address(#[from] Instantiate2AddressError),

    #[error("{0}")]
    CheckedMultiplyRatioError(#[from] CheckedMultiplyRatioError),

    #[error("{0}")]
    OverflowError(#[from] OverflowError),

    #[error("{0}")]
    CheckedFromRatioError(#[from] CheckedFromRatioError),

    #[error("{0}")]
    CoinsError(#[from] CoinsError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Generic error: {0}")]
    Generic(&'static str),
}

impl ContractError {
    pub fn generic_err(msg: impl Into<String>) -> Self {
        ContractError::Std(StdError::generic_err(msg.into()))
    }
}

pub type ContractResult = Result<Response, ContractError>;

pub struct Contract(pub Addr);

impl Contract {
    pub fn addr(&self) -> Addr {
        self.0.clone()
    }

    pub fn call(&self, msg: Binary, funds: Vec<Coin>) -> CosmosMsg {
        WasmMsg::Execute {
            contract_addr: self.addr().into(),
            msg,
            funds,
        }
        .into()
    }
}

#[cw_serde]
pub struct Callback {
    pub contract: Addr,
    pub msg: Binary,
    pub execution_rebate: Vec<Coin>,
}

#[cw_serde]
pub enum LogicalOperator {
    And,
    Or,
}

#[cw_serde]
pub enum Condition {
    TimestampElapsed(Timestamp),
    BlocksCompleted(u64),
    ExchangeLiquidityProvided {
        exchanger_contract: Addr,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
        maximum_slippage_bps: u128,
        route: Option<Route>,
    },
    LimitOrderFilled {
        pair_address: Addr,
        owner: Addr,
        side: Side,
        price: Price,
    },
    BalanceAvailable {
        address: Addr,
        amount: Coin,
    },
    StrategyStatus {
        manager_contract: Addr,
        contract_address: Addr,
        status: StrategyStatus,
    },
    Compound {
        conditions: Vec<Condition>,
        operator: LogicalOperator,
    },
}

impl Condition {
    pub fn check(&self, deps: Deps, env: &Env) -> StdResult<()> {
        match self {
            Condition::TimestampElapsed(timestamp) => {
                if env.block.time >= *timestamp {
                    return Ok(());
                }

                Err(StdError::generic_err(format!(
                    "Timestamp not elapsed: current timestamp ({}) is before required timestamp ({})",
                    env.block.time, timestamp
                )))
            }
            Condition::BlocksCompleted(height) => {
                if env.block.height >= *height {
                    return Ok(());
                }

                Err(StdError::generic_err(format!(
                    "Blocks not completed: current height ({}) is before required height ({})",
                    env.block.height, height
                )))
            }
            Condition::LimitOrderFilled {
                pair_address,
                owner,
                side,
                price,
            } => {
                let order = deps
                    .querier
                    .query_wasm_smart::<OrderResponse>(
                        pair_address,
                        &QueryMsg::Order((owner.to_string(), side.clone(), price.clone())),
                    )
                    .map_err(|e| {
                        StdError::generic_err(format!(
                            "Failed to query order ({:?} {:?} {:?}): {}",
                            owner, side, price, e
                        ))
                    })?;

                if order.remaining.is_zero() {
                    return Ok(());
                }

                Err(StdError::generic_err(format!(
                    "Limit order not filled ({} remaining)",
                    order.remaining
                )))
            }
            Condition::ExchangeLiquidityProvided {
                exchanger_contract,
                swap_amount,
                minimum_receive_amount,
                maximum_slippage_bps,
                route,
            } => {
                let expected_receive_amount =
                    deps.querier.query_wasm_smart::<ExpectedReceiveAmount>(
                        exchanger_contract,
                        &ExchangeQueryMsg::ExpectedReceiveAmount {
                            swap_amount: swap_amount.clone(),
                            target_denom: minimum_receive_amount.denom.clone(),
                            route: route.clone(),
                        },
                    )?;

                if expected_receive_amount.receive_amount.amount < minimum_receive_amount.amount {
                    return Err(StdError::generic_err(format!(
                        "Expected receive amount {} is less than minimum receive amount {}",
                        expected_receive_amount.receive_amount.amount,
                        minimum_receive_amount.amount
                    )));
                }

                if expected_receive_amount.slippage_bps > *maximum_slippage_bps {
                    return Err(StdError::generic_err(format!(
                        "Slippage basis points {} exceeds maximum allowed slippage basis points {}",
                        expected_receive_amount.slippage_bps, maximum_slippage_bps
                    )));
                }

                Ok(())
            }
            Condition::BalanceAvailable { address, amount } => {
                let balance = deps.querier.query_balance(address, amount.denom.clone())?;

                if balance.amount >= amount.amount {
                    return Ok(());
                }

                Err(StdError::generic_err(format!(
                    "Balance available for {} ({}) is less than required ({})",
                    address, balance.amount, amount.amount
                )))
            }
            Condition::StrategyStatus {
                manager_contract,
                contract_address,
                status,
            } => {
                let strategy = deps.querier.query_wasm_smart::<Strategy>(
                    manager_contract,
                    &ManagerQueryMsg::Strategy {
                        address: contract_address.clone(),
                    },
                )?;

                if strategy.status == *status {
                    return Ok(());
                }

                Err(StdError::generic_err(format!(
                    "Strategy not in required status: expected {:?}, got {:?}",
                    status, strategy.status
                )))
            }
            Condition::Compound {
                conditions,
                operator,
            } => match operator {
                LogicalOperator::And => {
                    for condition in conditions {
                        condition.check(deps, env)?;
                    }
                    Ok(())
                }
                LogicalOperator::Or => {
                    for condition in conditions {
                        if condition.check(deps, env).is_ok() {
                            return Ok(());
                        }
                    }
                    Err(StdError::generic_err(format!(
                        "No compound conditions met in: {}",
                        conditions
                            .iter()
                            .map(|c| c.description())
                            .collect::<Vec<_>>()
                            .join(",\n")
                    )))
                }
            },
        }
    }

    pub fn description(&self) -> String {
        match self {
            Condition::TimestampElapsed(timestamp) => format!("timestamp elapsed: {}", timestamp),
            Condition::BlocksCompleted(height) => format!("blocks completed: {}", height),
            Condition::ExchangeLiquidityProvided {
                swap_amount,
                minimum_receive_amount,
                maximum_slippage_bps,
                ..
            } => format!(
                "exchange liquidity provided: swap_amount={}, minimum_receive_amount={}, maximum_slippage_bps={}",
                swap_amount, minimum_receive_amount, maximum_slippage_bps
            ),
            Condition::LimitOrderFilled {
                pair_address,
                owner,
                side,
                price,
            } => format!(
                "limit order filled: pair_address={}, owner={}, side={:?}, price={}",
                pair_address, owner, side, price
            ),
            Condition::BalanceAvailable { address, amount } => format!(
                "balance available: address={}, amount={}",
                address, amount
            ),
            Condition::StrategyStatus {
                contract_address,
                status,
                ..
            } => format!(
                "strategy ({}) is in status: {:?}",
                contract_address, status
            ),
            Condition::Compound { conditions, operator } => {
                match operator {
                    LogicalOperator::And => format!(
                        "All the following conditions are met: [{:#?}]",
                        conditions
                            .iter()
                            .map(|c| c.description())
                            .collect::<Vec<_>>()
                            .join(",\n")
                    ),
                    LogicalOperator::Or => format!(
                        "Any of the following conditions are met: [{:#?}]",
                        conditions
                            .iter()
                            .map(|c| c.description())
                            .collect::<Vec<_>>()
                            .join(",\n")
                    ),
                }
            }
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
pub enum Action {
    FixedSwap {
        exchange_contract: Addr,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
        maximum_slippage_bps: u128,
        route: Option<Route>,
        schedule: Option<Schedule>,
    },
    LinearlyScaledSwap {
        exchange_contract: Addr,
        base_swap_amount: Coin,
        base_receive_amount: Coin,
        minimum_swap_amount: Coin,
        minimum_receive_amount: Coin,
        multiplier: Decimal,
        maximum_slippage_bps: u128,
        route: Option<Route>,
        schedule: Option<Schedule>,
    },
    FixedLimitOrder {
        pair_address: Addr,
        bid_denom: String,
        bid_amount: Option<Uint128>,
        side: Side,
        price: Price,
        schedule: Option<Schedule>,
    },
    DynamicLimitOrder {
        pair_address: Addr,
        bid_denom: String,
        bid_amount: Option<Uint128>,
        side: Side,
        direction: Direction,
        offset: Offset,
        current_price: Option<Price>,
        schedule: Option<Schedule>,
    },
}

impl Action {
    pub fn perform(
        &self,
        deps: Deps,
        env: &Env,
        config: &StrategyConfig,
    ) -> StdResult<(Action, Vec<Condition>, Vec<CosmosMsg>)> {
        let mut action = self.clone();
        let mut messages: Vec<CosmosMsg> = vec![];
        let mut conditions: Vec<Condition> = vec![];

        match self {
            Action::FixedSwap {
                exchange_contract,
                swap_amount,
                minimum_receive_amount,
                maximum_slippage_bps,
                route,
                schedule,
            } => {
                let swap_balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), swap_amount.denom.clone())?;

                let swap_amount = Coin::new(
                    min(swap_balance.amount, swap_amount.amount),
                    swap_amount.denom.clone(),
                );

                if swap_amount.amount.gt(&Uint128::zero()) {
                    return Ok((action, conditions, messages));
                }

                let swap_msg = Contract(exchange_contract.clone()).call(
                    to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: minimum_receive_amount.clone(),
                        maximum_slippage_bps: *maximum_slippage_bps,
                        route: route.clone(),
                        recipient: Some(config.distributor.clone()),
                        on_complete: Some(Callback {
                            msg: to_json_binary(&DistributorExecuteMsg::Distribute {})?,
                            contract: config.distributor.clone(),
                            execution_rebate: config.execution_rebate.clone(),
                        }),
                    })?,
                    vec![swap_amount.clone()],
                );

                messages.push(swap_msg);

                let mut swap_conditions = vec![
                    Condition::BalanceAvailable {
                        address: env.contract.address.clone(),
                        amount: Coin::new(1u128, swap_amount.denom.clone()),
                    },
                    Condition::ExchangeLiquidityProvided {
                        exchanger_contract: exchange_contract.clone(),
                        swap_amount: swap_amount.clone(),
                        minimum_receive_amount: minimum_receive_amount.clone(),
                        maximum_slippage_bps: *maximum_slippage_bps,
                        route: route.clone(),
                    },
                ];

                if let Some(schedule) = schedule {
                    swap_conditions.push(schedule.into_condition(env));
                };

                conditions.push(Condition::Compound {
                    conditions: swap_conditions,
                    operator: LogicalOperator::And,
                });
            }
            Action::LinearlyScaledSwap {
                exchange_contract,
                base_swap_amount,
                base_receive_amount,
                minimum_swap_amount,
                minimum_receive_amount,
                multiplier,
                maximum_slippage_bps,
                route,
                schedule,
            } => {
                let expected_receive_amount =
                    deps.querier.query_wasm_smart::<ExpectedReceiveAmount>(
                        exchange_contract,
                        &ExchangeQueryMsg::ExpectedReceiveAmount {
                            swap_amount: base_swap_amount.clone(),
                            target_denom: base_swap_amount.denom.clone(),
                            route: None,
                        },
                    )?;

                let base_price =
                    Decimal::from_ratio(base_receive_amount.amount, base_swap_amount.amount);

                let current_price = Decimal::from_ratio(
                    base_swap_amount.amount,
                    expected_receive_amount.receive_amount.amount,
                );

                let price_delta = base_price.abs_diff(current_price) / base_price;
                let scaled_price_delta = price_delta * multiplier;

                let scaled_swap_amount = if current_price < base_price {
                    base_swap_amount
                        .amount
                        .mul_floor(Decimal::one() + scaled_price_delta)
                } else {
                    base_swap_amount
                        .amount
                        .mul_floor(Decimal::one() - scaled_price_delta)
                };

                if scaled_swap_amount.is_zero() {
                    return Ok((action, conditions, messages));
                }

                let scaled_minimum_receive_amount = minimum_receive_amount.amount.mul_ceil(
                    Decimal::from_ratio(scaled_swap_amount, base_swap_amount.amount),
                );

                let swap_amount = Coin::new(
                    max(scaled_swap_amount, minimum_swap_amount.amount),
                    base_swap_amount.denom.clone(),
                );

                if swap_amount.amount.is_zero() {
                    return Ok((action, conditions, messages));
                }

                let minimum_receive_amount = Coin::new(
                    scaled_minimum_receive_amount,
                    minimum_receive_amount.denom.clone(),
                );

                let swap_msg = Contract(exchange_contract.clone()).call(
                    to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: minimum_receive_amount.clone(),
                        maximum_slippage_bps: *maximum_slippage_bps,
                        route: route.clone(),
                        recipient: Some(config.distributor.clone()),
                        on_complete: Some(Callback {
                            msg: to_json_binary(&DistributorExecuteMsg::Distribute {})?,
                            contract: config.distributor.clone(),
                            execution_rebate: config.execution_rebate.clone(),
                        }),
                    })?,
                    vec![swap_amount],
                );

                messages.push(swap_msg);

                let mut swap_conditions = vec![
                    Condition::BalanceAvailable {
                        address: env.contract.address.clone(),
                        amount: Coin::new(1u128, base_swap_amount.denom.clone()),
                    },
                    Condition::ExchangeLiquidityProvided {
                        exchanger_contract: exchange_contract.clone(),
                        swap_amount: base_swap_amount.clone(),
                        minimum_receive_amount,
                        maximum_slippage_bps: *maximum_slippage_bps,
                        route: route.clone(),
                    },
                ];

                if let Some(schedule) = schedule {
                    swap_conditions.push(schedule.into_condition(env));
                };

                conditions.push(Condition::Compound {
                    conditions: swap_conditions,
                    operator: LogicalOperator::And,
                });
            }
            Action::FixedLimitOrder {
                pair_address,
                side,
                price,
                bid_denom,
                bid_amount,
                schedule,
            } => {
                let existing_order = deps
                    .querier
                    .query_wasm_smart::<OrderResponse>(
                        pair_address,
                        &QueryMsg::Order((
                            env.contract.address.to_string(),
                            side.clone(),
                            price.clone(),
                        )),
                    )
                    .ok();

                let mut bid_denom_balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), bid_denom.clone())?;

                let mut bank_msg: Option<BankMsg> = None;

                if let Some(existing_order) = existing_order {
                    bid_denom_balance.amount += existing_order.remaining;

                    if existing_order.filled.gt(&Uint128::zero()) {
                        let pair = deps.querier.query_wasm_smart::<ConfigResponse>(
                            pair_address,
                            &QueryMsg::Config {},
                        )?;

                        bank_msg = Some(BankMsg::Send {
                            to_address: config.distributor.to_string(),
                            amount: vec![Coin::new(existing_order.filled, pair.denoms.ask(side))],
                        });
                    }
                };

                let bid_amount = min(
                    bid_amount.unwrap_or(bid_denom_balance.amount),
                    bid_denom_balance.amount,
                );

                if bid_amount.is_zero() {
                    if let Some(bank_msg) = bank_msg {
                        messages.push(bank_msg.into());
                    }

                    return Ok((action, conditions, messages));
                }

                let set_order_msg = Contract(pair_address.clone()).call(
                    to_json_binary(&ExecuteMsg::Order((
                        vec![
                            (side.clone(), price.clone(), Some(Uint128::zero())),
                            (side.clone(), price.clone(), Some(bid_amount)),
                        ],
                        None,
                    )))?,
                    vec![Coin::new(bid_amount, bid_denom.clone())],
                );

                messages.push(set_order_msg);

                if let Some(bank_msg) = bank_msg {
                    messages.push(bank_msg.into());
                }

                let order_filled_condition = Condition::LimitOrderFilled {
                    pair_address: pair_address.clone(),
                    owner: env.contract.address.clone(),
                    side: side.clone(),
                    price: price.clone(),
                };

                if let Some(schedule) = schedule {
                    conditions.push(Condition::Compound {
                        conditions: vec![
                            order_filled_condition.clone(),
                            schedule.into_condition(env),
                        ],
                        operator: LogicalOperator::Or,
                    });
                } else {
                    conditions.push(order_filled_condition);
                }
            }
            Action::DynamicLimitOrder {
                pair_address,
                bid_denom,
                bid_amount,
                side,
                direction,
                offset,
                current_price,
                schedule,
            } => {
                let mut existing_order: Option<OrderResponse> = None;

                if let Some(previous_price) = current_price {
                    existing_order = deps
                        .querier
                        .query_wasm_smart::<OrderResponse>(
                            pair_address,
                            &QueryMsg::Order((
                                env.contract.address.to_string(),
                                side.clone(),
                                previous_price.clone(),
                            )),
                        )
                        .ok();
                }

                let mut bid_denom_balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), bid_denom.clone())?;

                let mut bank_msg: Option<BankMsg> = None;

                if let Some(existing_order) = existing_order.clone() {
                    bid_denom_balance.amount += existing_order.remaining;

                    if existing_order.filled.gt(&Uint128::zero()) {
                        let pair = deps.querier.query_wasm_smart::<ConfigResponse>(
                            pair_address,
                            &QueryMsg::Config {},
                        )?;

                        bank_msg = Some(BankMsg::Send {
                            to_address: config.distributor.to_string(),
                            amount: vec![Coin::new(existing_order.filled, pair.denoms.ask(side))],
                        });
                    }
                };

                let book = deps.querier.query_wasm_smart::<BookResponse>(
                    pair_address,
                    &QueryMsg::Book {
                        limit: Some(1),
                        offset: None,
                    },
                )?;

                let book_price = if *side == Side::Base {
                    book.base
                } else {
                    book.quote
                }[0]
                .price;

                let current_price = match offset {
                    Offset::Exact(offset) => match direction {
                        Direction::Up => book_price.saturating_add(*offset),
                        Direction::Down => book_price.saturating_sub(*offset),
                    },
                    Offset::Bps(offset) => match direction {
                        Direction::Up => book_price
                            .saturating_mul(Decimal::one().saturating_add(Decimal::bps(*offset))),
                        Direction::Down => book_price
                            .saturating_mul(Decimal::one().saturating_sub(Decimal::bps(*offset))),
                    },
                };

                let new_bid_amount = min(
                    bid_amount.unwrap_or(bid_denom_balance.amount),
                    bid_denom_balance.amount,
                );

                if new_bid_amount.is_zero() {
                    if let Some(bank_msg) = bank_msg {
                        messages.push(bank_msg.into());
                    }

                    return Ok((action, conditions, messages));
                }

                if current_price.gt(&Decimal::zero()) {
                    let set_order_msg = Contract(pair_address.clone()).call(
                        to_json_binary(&ExecuteMsg::Order((
                            [
                                existing_order.map_or_else(
                                    || vec![],
                                    |o| vec![(o.side, o.price, Some(Uint128::zero()))],
                                ),
                                vec![(
                                    side.clone(),
                                    Price::Fixed(current_price),
                                    Some(new_bid_amount),
                                )],
                            ]
                            .concat(),
                            None,
                        )))?,
                        vec![Coin::new(new_bid_amount, bid_denom.clone())],
                    );

                    messages.push(set_order_msg);

                    let order_filled_condition = Condition::LimitOrderFilled {
                        pair_address: pair_address.clone(),
                        owner: env.contract.address.clone(),
                        side: side.clone(),
                        price: Price::Fixed(current_price),
                    };

                    if let Some(schedule) = schedule {
                        conditions.push(Condition::Compound {
                            conditions: vec![order_filled_condition, schedule.into_condition(env)],
                            operator: LogicalOperator::Or,
                        });
                    } else {
                        conditions.push(order_filled_condition);
                    }

                    action = Action::DynamicLimitOrder {
                        pair_address: pair_address.clone(),
                        bid_denom: bid_denom.clone(),
                        bid_amount: bid_amount.clone(),
                        side: side.clone(),
                        direction: direction.clone(),
                        offset: offset.clone(),
                        current_price: Some(Price::Fixed(current_price)),
                        schedule: schedule.clone(),
                    }
                }
            }
        };

        Ok((action, conditions, messages))
    }
}

#[cw_serde]
pub struct Behaviour {
    pub actions: Vec<Action>,
    pub conditions: Vec<Condition>,
}

impl Behaviour {
    pub fn execute(
        &mut self,
        deps: Deps,
        env: &Env,
        config: &StrategyConfig,
    ) -> StdResult<Vec<CosmosMsg>> {
        let mut new_actions = vec![];
        let mut new_conditions = vec![];
        let mut all_messages = vec![];

        for action in &self.actions {
            let (action, conditions, messages) = action.perform(deps, env, config)?;

            new_actions.push(action);
            new_conditions.extend(conditions);
            all_messages.extend(messages);
        }

        self.actions = new_actions;
        self.conditions = new_conditions;

        Ok(all_messages)
    }
}

#[cw_serde]
pub enum Schedule {
    Blocks {
        interval: u64,
        previous: Option<u64>,
    },
    Time {
        duration: Duration,
        previous: Option<Timestamp>,
    },
}

impl Schedule {
    pub fn is_due(&self, env: &Env) -> bool {
        match self {
            Schedule::Blocks { interval, previous } => {
                let last_block = previous.unwrap_or(0);
                env.block.height > last_block + interval
            }
            Schedule::Time { duration, previous } => {
                let last_time = previous.unwrap_or(Timestamp::from_seconds(0));
                env.block.time.seconds() > last_time.seconds() + duration.as_secs()
            }
        }
    }

    pub fn into_condition(&self, env: &Env) -> Condition {
        match self {
            Schedule::Blocks { interval, previous } => {
                let last_block = previous.unwrap_or(env.block.height - *interval);
                Condition::BlocksCompleted(last_block + interval)
            }
            Schedule::Time { duration, previous } => {
                let last_time = previous.unwrap_or(Timestamp::from_seconds(
                    env.block.time.seconds() - duration.as_secs(),
                ));
                Condition::TimestampElapsed(Timestamp::from_seconds(
                    last_time.seconds() + duration.as_secs(),
                ))
            }
        }
    }

    pub fn next(&self, env: &Env) -> Self {
        match self {
            Schedule::Blocks { interval, previous } => Schedule::Blocks {
                interval: *interval,
                previous: if let Some(previous) = previous {
                    let next = previous + *interval;
                    if next < env.block.height {
                        Some(env.block.height - (env.block.height - previous) % interval)
                    } else {
                        Some(next)
                    }
                } else {
                    Some(env.block.height)
                },
            },
            Schedule::Time { duration, previous } => Schedule::Time {
                duration: *duration,
                previous: if let Some(previous) = previous {
                    let next = previous.plus_seconds(duration.as_secs());
                    if next < env.block.time {
                        Some(Timestamp::from_seconds(
                            env.block.time.seconds()
                                - (env.block.time.minus_seconds(previous.seconds())).seconds()
                                    % duration.as_secs(),
                        ))
                    } else {
                        Some(next)
                    }
                } else {
                    Some(env.block.time)
                },
            },
        }
    }
}

#[cw_serde]
pub struct StrategyConfig {
    pub owner: Addr,
    pub manager: Addr,
    pub distributor: Addr,
    pub scheduler: Addr,
    pub behaviours: Vec<Behaviour>,
    pub execution_rebate: Vec<Coin>,
}

#[cfg(test)]
mod conditions_tests {
    use std::str::FromStr;

    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        to_json_binary, Addr, Coin, ContractResult, Decimal, StdError, SystemResult, Timestamp,
        Uint128,
    };
    use rujira_rs::fin::{OrderResponse, Price, Side};

    use crate::{
        core::{Condition, StrategyStatus},
        exchanger::ExpectedReceiveAmount,
        manager::Strategy,
    };

    #[test]
    fn timestamp_elapsed_check() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::TimestampElapsed(Timestamp::from_seconds(0))
            .check(deps.as_ref(), &env)
            .is_ok());

        assert!(Condition::TimestampElapsed(env.block.time)
            .check(deps.as_ref(), &env)
            .is_ok());

        assert!(Condition::TimestampElapsed(env.block.time.plus_seconds(1))
            .check(deps.as_ref(), &env)
            .is_err());
    }

    #[test]
    fn blocks_completed_check() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::BlocksCompleted(0)
            .check(deps.as_ref(), &env)
            .is_ok());

        assert!(Condition::BlocksCompleted(env.block.height)
            .check(deps.as_ref(), &env)
            .is_ok());

        assert!(Condition::BlocksCompleted(env.block.height + 1)
            .check(deps.as_ref(), &env)
            .is_err());
    }

    #[test]
    fn balance_available_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(0u128, "rune"),
        }
        .check(deps.as_ref(), &env)
        .is_ok());

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(1u128, "rune"),
        }
        .check(deps.as_ref(), &env)
        .is_err());

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![Coin::new(100u128, "rune")],
        );

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(99u128, "rune"),
        }
        .check(deps.as_ref(), &env)
        .is_ok());

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(100u128, "rune"),
        }
        .check(deps.as_ref(), &env)
        .is_ok());

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(101u128, "rune"),
        }
        .check(deps.as_ref(), &env)
        .is_err());
    }

    #[test]
    fn exchange_liquidity_provided_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&ExpectedReceiveAmount {
                    receive_amount: Coin::new(100u128, "rune"),
                    slippage_bps: 10,
                })
                .unwrap(),
            ))
        });

        assert!(Condition::ExchangeLiquidityProvided {
            exchanger_contract: env.contract.address.clone(),
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(101u128, "rune"),
            maximum_slippage_bps: 10,
            route: None,
        }
        .check(deps.as_ref(), &env)
        .is_err());

        assert!(Condition::ExchangeLiquidityProvided {
            exchanger_contract: env.contract.address.clone(),
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(100u128, "rune"),
            maximum_slippage_bps: 9,
            route: None,
        }
        .check(deps.as_ref(), &env)
        .is_err());

        assert!(Condition::ExchangeLiquidityProvided {
            exchanger_contract: env.contract.address.clone(),
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(100u128, "rune"),
            maximum_slippage_bps: 10,
            route: None,
        }
        .check(deps.as_ref(), &env)
        .is_ok());
    }

    #[test]
    fn limit_order_filled_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&OrderResponse {
                    remaining: Uint128::new(100),
                    filled: Uint128::new(100),
                    owner: "owner".to_string(),
                    side: Side::Base,
                    price: Price::Fixed(Decimal::from_str("1.0").unwrap()),
                    rate: Decimal::from_str("1.0").unwrap(),
                    updated_at: Timestamp::from_seconds(env.block.time.seconds()),
                    offer: Uint128::new(21029),
                })
                .unwrap(),
            ))
        });

        assert_eq!(
            Condition::LimitOrderFilled {
                owner: Addr::unchecked("owner"),
                pair_address: Addr::unchecked("pair"),
                side: Side::Base,
                price: Price::Fixed(Decimal::from_str("1.0").unwrap()),
            }
            .check(deps.as_ref(), &env)
            .unwrap_err(),
            StdError::generic_err("Limit order not filled (100 remaining)",)
        );

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&OrderResponse {
                    remaining: Uint128::new(0),
                    filled: Uint128::new(100),
                    owner: "owner".to_string(),
                    side: Side::Base,
                    price: Price::Fixed(Decimal::from_str("1.0").unwrap()),
                    rate: Decimal::from_str("1.0").unwrap(),
                    updated_at: Timestamp::from_seconds(env.block.time.seconds()),
                    offer: Uint128::new(21029),
                })
                .unwrap(),
            ))
        });

        assert!(Condition::LimitOrderFilled {
            owner: Addr::unchecked("owner"),
            pair_address: Addr::unchecked("pair"),
            side: Side::Base,
            price: Price::Fixed(Decimal::from_str("1.0").unwrap()),
        }
        .check(deps.as_ref(), &env)
        .is_ok());
    }

    #[test]
    fn strategy_status_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&Strategy {
                    id: 1,
                    contract_address: Addr::unchecked("strategy"),
                    status: StrategyStatus::Active,
                    owner: Addr::unchecked("owner"),
                    created_at: 0,
                    updated_at: 0,
                    label: "label".to_string(),
                    affiliates: vec![],
                })
                .unwrap(),
            ))
        });

        let strategy_address = Addr::unchecked("strategy");

        assert!(Condition::StrategyStatus {
            manager_contract: Addr::unchecked("manager"),
            contract_address: strategy_address.clone(),
            status: StrategyStatus::Active,
        }
        .check(deps.as_ref(), &env)
        .is_ok());

        assert!(Condition::StrategyStatus {
            manager_contract: Addr::unchecked("manager"),
            contract_address: strategy_address.clone(),
            status: StrategyStatus::Paused,
        }
        .check(deps.as_ref(), &env)
        .is_err());
    }
}

#[cfg(test)]
mod schedule_tests {
    use std::time::Duration;

    use cosmwasm_std::{testing::mock_env, Timestamp};

    use crate::core::{Condition, Schedule};

    #[test]
    fn updates_to_next_scheduled_block() {
        let env = mock_env();

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: None
            }
            .next(&env),
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height)
            }
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
            .next(&env),
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5 + 10)
            }
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 15)
            }
            .next(&env),
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 155)
            }
            .next(&env),
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
        );
    }

    #[test]
    fn updates_to_next_scheduled_time() {
        let env = mock_env();

        assert_eq!(
            Schedule::Time {
                duration: std::time::Duration::from_secs(10),
                previous: None
            }
            .next(&env),
            Schedule::Time {
                duration: std::time::Duration::from_secs(10),
                previous: Some(env.block.time)
            }
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .next(&env),
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.plus_seconds(5))
            }
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(15))
            }
            .next(&env),
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(Timestamp::from_seconds(env.block.time.seconds() - 5))
            }
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(155))
            }
            .next(&env),
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(Timestamp::from_seconds(env.block.time.seconds() - 5))
            }
        );
    }

    #[test]
    fn gets_next_block_condition() {
        let env = mock_env();

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: None
            }
            .into_condition(&env),
            Condition::BlocksCompleted(env.block.height)
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height)
            }
            .into_condition(&env),
            Condition::BlocksCompleted(env.block.height + 10)
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
            .into_condition(&env),
            Condition::BlocksCompleted(env.block.height - 5 + 10)
        );
    }

    #[test]
    fn gets_next_time_condition() {
        let env = mock_env();

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: None
            }
            .into_condition(&env),
            Condition::TimestampElapsed(Timestamp::from_seconds(env.block.time.seconds()))
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time)
            }
            .into_condition(&env),
            Condition::TimestampElapsed(Timestamp::from_seconds(env.block.time.seconds() + 10))
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .into_condition(&env),
            Condition::TimestampElapsed(Timestamp::from_seconds(env.block.time.seconds() - 5 + 10))
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(155))
            }
            .into_condition(&env),
            Condition::TimestampElapsed(Timestamp::from_seconds(
                env.block.time.seconds() - 155 + 10
            ))
        );
    }

    #[test]
    fn block_schedule_is_due() {
        let env = mock_env();

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: None
            }
            .is_due(&env),
            true
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 10,
                previous: Some(env.block.height - 5)
            }
            .is_due(&env),
            false
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 5,
                previous: Some(env.block.height - 6)
            }
            .is_due(&env),
            true
        );

        assert_eq!(
            Schedule::Blocks {
                interval: 5,
                previous: Some(env.block.height - 5)
            }
            .is_due(&env),
            false
        );
    }

    #[test]
    fn time_schedule_is_due() {
        let env = mock_env();

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: None
            }
            .is_due(&env),
            true
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .is_due(&env),
            false
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(5),
                previous: Some(env.block.time.minus_seconds(6))
            }
            .is_due(&env),
            true
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(5),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .is_due(&env),
            false
        );
    }
}
