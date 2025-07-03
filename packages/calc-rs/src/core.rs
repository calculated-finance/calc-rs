use std::{
    cmp::{max, min},
    collections::{HashMap, HashSet},
    time::Duration,
    u8, vec,
};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, to_json_string, Addr, BankMsg, Binary, CheckedFromRatioError,
    CheckedMultiplyRatioError, Coin, Coins, CoinsError, CosmosMsg, Decimal, Deps, Env, Event,
    Instantiate2AddressError, OverflowError, Response, StdError, StdResult, SubMsg, Timestamp,
    Uint128, WasmMsg,
};
use rujira_rs::fin::{
    BookResponse, ConfigResponse, ExecuteMsg, OrderResponse, Price, QueryMsg, Side,
};
use thiserror::Error;

use crate::{
    distributor::{Destination, Recipient},
    exchanger::{ExchangeExecuteMsg, ExchangeQueryMsg, ExpectedReceiveAmount, Route},
    manager::{Affiliate, ManagerQueryMsg, Strategy, StrategyStatus},
    thorchain::MsgDeposit,
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
    TimeElapsed(Timestamp),
    BlocksCompleted(u64),
    CanSwap {
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
            Condition::TimeElapsed(timestamp) => {
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
            Condition::CanSwap {
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
            Condition::TimeElapsed(timestamp) => format!("timestamp elapsed: {}", timestamp),
            Condition::BlocksCompleted(height) => format!("blocks completed: {}", height),
            Condition::CanSwap {
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
                        "All the following conditions are met: [\n\t{}\n]",
                        conditions
                            .iter()
                            .map(|c| c.description())
                            .collect::<Vec<_>>()
                            .join(",\n\t")
                    ),
                    LogicalOperator::Or => format!(
                        "Any of the following conditions are met: [\n\t{}\n]",
                        conditions
                            .iter()
                            .map(|c| c.description())
                            .collect::<Vec<_>>()
                            .join(",\n\t")
                    ),
                }
            }
        }
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
                previous.map_or(true, |previous| env.block.height >= previous + interval)
            }
            Schedule::Time { duration, previous } => previous.map_or(true, |previous| {
                env.block.time.seconds() >= previous.seconds() + duration.as_secs()
            }),
        }
    }

    pub fn into_condition(&self, env: &Env) -> Condition {
        match self {
            Schedule::Blocks { interval, previous } => Condition::BlocksCompleted(
                previous.map_or(env.block.height, |previous| previous + interval),
            ),
            Schedule::Time { duration, previous } => {
                Condition::TimeElapsed(previous.map_or(env.block.time, |previous| {
                    previous.plus_seconds(duration.as_secs())
                }))
            }
        }
    }

    pub fn next(&self, env: &Env) -> Self {
        match self {
            Schedule::Blocks { interval, previous } => Schedule::Blocks {
                interval: *interval,
                previous: Some(previous.map_or(env.block.height, |previous| {
                    let next = previous + *interval;
                    if next < env.block.height - interval {
                        let blocks_completed = env.block.height - previous;
                        env.block.height + blocks_completed % interval
                    } else {
                        next
                    }
                })),
            },
            Schedule::Time { duration, previous } => Schedule::Time {
                duration: *duration,
                previous: Some(previous.map_or(env.block.time, |previous| {
                    let duration = duration.as_secs();
                    let next = previous.plus_seconds(duration);
                    if next < env.block.time.minus_seconds(duration) {
                        let time_elapsed = env.block.time.seconds() - previous.seconds();
                        env.block.time.plus_seconds(time_elapsed % duration)
                    } else {
                        next
                    }
                })),
            },
        }
    }
}

#[cw_serde]
pub struct Statistics {
    pub swapped: Vec<Coin>,
    pub filled: Vec<Coin>,
    pub distributed: Vec<(Recipient, Vec<Coin>)>,
    pub withdrawn: Vec<Coin>,
}

impl Default for Statistics {
    fn default() -> Self {
        Statistics {
            swapped: vec![],
            filled: vec![],
            distributed: vec![],
            withdrawn: vec![],
        }
    }
}

impl Statistics {
    pub fn add(&mut self, other: Statistics) {
        self.add_unsafe(other).unwrap_or_default();
    }

    fn add_unsafe(&mut self, other: Statistics) -> StdResult<()> {
        let mut swapped = Coins::try_from(self.swapped.clone()).unwrap_or(Coins::default());
        let mut filled = Coins::try_from(self.filled.clone()).unwrap_or(Coins::default());
        let mut withdrawn = Coins::try_from(self.withdrawn.clone()).unwrap_or(Coins::default());

        for coin in other.swapped {
            swapped.add(coin)?;
        }

        for coin in other.filled {
            filled.add(coin)?;
        }

        for coin in other.withdrawn {
            withdrawn.add(coin)?;
        }

        let mut recipients_map: HashMap<String, Recipient> = HashMap::new();
        let mut distributed_map: HashMap<String, Coins> = HashMap::new();

        for (recipient, amounts) in self
            .distributed
            .iter()
            .chain(other.distributed.clone().iter())
            .into_iter()
        {
            recipients_map
                .entry(recipient.key())
                .or_insert_with(|| recipient.clone());

            distributed_map
                .entry(recipient.key())
                .and_modify(|coins| {
                    for amount in amounts {
                        coins.add(amount.clone()).unwrap_or_default();
                    }
                })
                .or_insert(Coins::try_from(amounts.clone())?);
        }

        let mut distributed: Vec<(Recipient, Vec<Coin>)> = Vec::new();

        for (key, coins) in distributed_map.into_iter() {
            let recipient = recipients_map
                .get(&key)
                .expect("Recipient should exist in map");
            distributed.push((recipient.clone(), coins.into_vec()));
        }

        self.swapped = swapped.to_vec();
        self.filled = filled.to_vec();
        self.distributed = distributed;
        self.withdrawn = withdrawn.to_vec();

        Ok(())
    }
}

#[cw_serde]
pub enum DomainEvent {
    StrategyCreated {
        contract_address: Addr,
        config: StrategyConfig,
    },
    StrategyUpdated {
        contract_address: Addr,
        old_config: StrategyConfig,
        new_config: StrategyConfig,
    },
    FundsWithdrawn {
        contract_address: Addr,
        to: Addr,
        funds: Vec<Coin>,
    },
    ExecutionAttempted {
        contract_address: Addr,
        pair_address: Addr,
        side: Side,
        price: Price,
    },
    ExecutionSucceeded {
        contract_address: Addr,
        statistics: Statistics,
    },
    ExecutionFailed {
        contract_address: Addr,
        reason: String,
    },
    ExecutionSkipped {
        contract_address: Addr,
        reason: String,
    },
    SchedulingAttempted {
        contract_address: Addr,
        conditions: Vec<Condition>,
    },
    SchedulingSucceeded {
        contract_address: Addr,
    },
    SchedulingFailed {
        contract_address: Addr,
        reason: String,
    },
    SchedulingSkipped {
        contract_address: Addr,
        reason: String,
    },
}

impl From<DomainEvent> for Event {
    fn from(event: DomainEvent) -> Self {
        match event {
            DomainEvent::StrategyCreated {
                contract_address,
                config,
            } => Event::new("_strategy_created")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "config",
                    to_json_string(&config).expect("Failed to serialize config"),
                ),
            DomainEvent::StrategyUpdated {
                contract_address,
                old_config,
                new_config,
            } => Event::new("_strategy_updated")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "old_config",
                    to_json_string(&old_config).expect("Failed to serialize old config"),
                )
                .add_attribute(
                    "new_config",
                    to_json_string(&new_config).expect("Failed to serialize new config"),
                ),
            DomainEvent::FundsWithdrawn {
                contract_address,
                to,
                funds,
            } => Event::new("funds_withdrawn")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("to", to.as_str())
                .add_attribute(
                    "funds",
                    to_json_string(&funds).expect("Failed to serialize withdrawn funds"),
                ),
            DomainEvent::ExecutionAttempted {
                contract_address,
                pair_address,
                side,
                price,
            } => Event::new("execution_attempted")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("pair_address", pair_address.as_str())
                .add_attribute("side", side.to_string())
                .add_attribute("price", price.to_string()),
            DomainEvent::ExecutionSucceeded {
                contract_address,
                statistics,
            } => Event::new("execution_succeeded")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "statistics",
                    to_json_string(&statistics).expect("Failed to serialize statistics"),
                ),
            DomainEvent::ExecutionFailed {
                contract_address,
                reason,
            } => Event::new("execution_failed")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
            DomainEvent::ExecutionSkipped {
                contract_address,
                reason,
            } => Event::new("execution_skipped")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
            DomainEvent::SchedulingAttempted {
                contract_address,
                conditions,
            } => Event::new("scheduling_attempted")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "conditions",
                    to_json_string(&conditions).expect("Failed to serialize conditions"),
                ),
            DomainEvent::SchedulingSucceeded { contract_address } => {
                Event::new("scheduling_succeeded")
                    .add_attribute("contract_address", contract_address.as_str())
            }
            DomainEvent::SchedulingFailed {
                contract_address,
                reason,
            } => Event::new("scheduling_failed")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
            DomainEvent::SchedulingSkipped {
                contract_address,
                reason,
            } => Event::new("scheduling_skipped")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
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
                        Price::Fixed(price.clone()),
                    )),
                )
                .ok(),
            OrderPriceStrategy::Offset { .. } => current_price
                .clone()
                .map(|price| {
                    deps.querier
                        .query_wasm_smart::<OrderResponse>(
                            pair_address,
                            &QueryMsg::Order((
                                env.contract.address.to_string(),
                                side.clone(),
                                price,
                            )),
                        )
                        .ok()
                })
                .flatten(),
        }
    }
}

#[cw_serde]
pub enum Action {
    Swap {
        exchange_contract: Addr,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
        maximum_slippage_bps: u128,
        adjustment: SwapAdjustment,
        route: Option<Route>,
        schedule: Option<Schedule>,
    },
    Order {
        pair_address: Addr,
        bid_denom: String,
        bid_amount: Option<Uint128>,
        side: Side,
        strategy: OrderPriceStrategy,
        current_price: Option<Price>,
        schedule: Option<Schedule>,
    },
    Distribute {
        denoms: Vec<String>,
        mutable_destinations: Vec<Destination>,
        immutable_destinations: Vec<Destination>,
        conditions: Vec<Condition>,
    },
}

impl Action {
    pub fn validate(&self, deps: Deps) -> StdResult<()> {
        match self {
            Action::Swap {
                exchange_contract,
                swap_amount,
                minimum_receive_amount,
                maximum_slippage_bps,
                route,
                ..
            } => {
                if swap_amount.amount.is_zero() {
                    return Err(StdError::generic_err("Swap amount cannot be zero"));
                }

                if *maximum_slippage_bps > 10_000 {
                    return Err(StdError::generic_err(
                        "Maximum slippage basis points cannot exceed 10,000",
                    ));
                }

                if let Some(route) = route {
                    match route {
                        Route::FinMarket { address } => {
                            let pair = deps.querier.query_wasm_smart::<ConfigResponse>(
                                exchange_contract,
                                &QueryMsg::Config {},
                            )?;

                            let denoms = [pair.denoms.base(), pair.denoms.quote()];

                            if !denoms.contains(&swap_amount.denom.as_str()) {
                                return Err(StdError::generic_err(format!(
                                    "Pair at {} does not support swapping from {}",
                                    address, swap_amount.denom
                                )));
                            }

                            if !denoms.contains(&minimum_receive_amount.denom.as_str()) {
                                return Err(StdError::generic_err(format!(
                                    "Pair at {} does not support swapping into {}",
                                    address, minimum_receive_amount.denom
                                )));
                            }
                        }
                        Route::Thorchain {} => {}
                    }
                }

                Ok(())
            }
            Action::Order {
                bid_amount,
                current_price,
                ..
            } => {
                if let Some(amount) = bid_amount {
                    if amount.lt(&Uint128::new(1_000)) {
                        return Err(StdError::generic_err(
                            "Bid amount cannot be less than 1,000",
                        ));
                    }
                }

                if let Some(price) = current_price {
                    match price {
                        Price::Fixed(price) => {
                            if price.is_zero() {
                                return Err(StdError::generic_err("Fixed price cannot be zero"));
                            }
                        }
                        Price::Oracle(_) => {}
                    }
                }

                Ok(())
            }
            Action::Distribute {
                denoms,
                mutable_destinations,
                immutable_destinations,
                ..
            } => {
                if denoms.is_empty() {
                    return Err(StdError::generic_err("Denoms cannot be empty"));
                }

                let destinations = mutable_destinations
                    .iter()
                    .chain(immutable_destinations.iter())
                    .collect::<Vec<_>>();

                let has_native_denoms = denoms.iter().any(|d| !d.contains("-"));
                let mut total_shares = Uint128::zero();

                for destination in destinations {
                    if destination.shares.is_zero() {
                        return Err(StdError::generic_err("Destination shares cannot be zero"));
                    }

                    match &destination.recipient {
                        Recipient::Bank { address, .. } | Recipient::Wasm { address, .. } => {
                            deps.api.addr_validate(&address.to_string()).map_err(|_| {
                                StdError::generic_err(format!(
                                    "Invalid destination address: {}",
                                    address
                                ))
                            })?;
                        }
                        Recipient::Deposit { memo } => {
                            if has_native_denoms {
                                return Err(StdError::generic_err(format!(
                                    "Only secured assets can be deposited with memo {}",
                                    memo
                                )));
                            }
                        }
                    }

                    total_shares += destination.shares;
                }

                if total_shares < Uint128::new(10_000) {
                    return Err(StdError::generic_err(
                        "Total shares must be at least 10,000",
                    ));
                }

                Ok(())
            }
        }
    }

    pub fn init(&self, deps: Deps, affiliates: &Vec<Affiliate>) -> StdResult<Action> {
        let action = match self {
            Action::Distribute {
                denoms,
                mutable_destinations,
                immutable_destinations,
                conditions,
            } => {
                let total_shares = mutable_destinations
                    .iter()
                    .chain(immutable_destinations.iter())
                    .into_iter()
                    .fold(Uint128::zero(), |acc, d| acc + d.shares);

                let total_fee_bps = affiliates
                    .iter()
                    .fold(0, |acc, affiliate| acc + affiliate.bps);

                let total_shares_with_fees =
                    total_shares.mul_ceil(Decimal::bps(10_000 + total_fee_bps));

                let fee_destinations = affiliates
                    .iter()
                    .map(|affiliate| Destination {
                        recipient: Recipient::Bank {
                            address: affiliate.address.clone(),
                        },
                        shares: total_shares_with_fees.mul_floor(Decimal::bps(affiliate.bps)),
                        label: Some(format!("{} fee", affiliate.code).to_string()),
                    })
                    .collect::<Vec<_>>();

                Action::Distribute {
                    denoms: denoms.clone(),
                    mutable_destinations: mutable_destinations.clone(),
                    immutable_destinations: [immutable_destinations.clone(), fee_destinations]
                        .concat(),
                    conditions: conditions.clone(),
                }
            }
            Action::Swap { .. } | Action::Order { .. } => self.clone(),
        };

        action.validate(deps)?;
        Ok(action)
    }

    pub fn execute(
        &self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(Action, Vec<Condition>, Vec<SubMsg>, Statistics)> {
        let mut stats = Statistics::default();
        let mut action = self.clone();
        let mut messages: Vec<SubMsg> = vec![];
        let mut conditions: Vec<Condition> = vec![];

        match self {
            Action::Swap {
                exchange_contract,
                swap_amount,
                minimum_receive_amount,
                maximum_slippage_bps,
                adjustment,
                route,
                schedule,
            } => {
                let (new_swap_amount, new_minimum_receive_amount) = match adjustment {
                    SwapAdjustment::Fixed => {
                        let swap_balance = deps.querier.query_balance(
                            env.contract.address.clone(),
                            swap_amount.denom.clone(),
                        )?;

                        let new_swap_amount = Coin::new(
                            min(swap_balance.amount, swap_amount.amount),
                            swap_amount.denom.clone(),
                        );

                        let new_minimum_receive_amount = Coin::new(
                            minimum_receive_amount.amount.mul_floor(Decimal::from_ratio(
                                new_swap_amount.amount,
                                swap_amount.amount,
                            )),
                            minimum_receive_amount.denom.clone(),
                        );

                        (new_swap_amount, new_minimum_receive_amount)
                    }
                    SwapAdjustment::LinearScalar {
                        base_receive_amount,
                        minimum_swap_amount,
                        scalar,
                    } => {
                        let expected_receive_amount =
                            deps.querier.query_wasm_smart::<ExpectedReceiveAmount>(
                                exchange_contract,
                                &ExchangeQueryMsg::ExpectedReceiveAmount {
                                    swap_amount: swap_amount.clone(),
                                    target_denom: swap_amount.denom.clone(),
                                    route: None,
                                },
                            )?;

                        let base_price =
                            Decimal::from_ratio(base_receive_amount.amount, swap_amount.amount);

                        let current_price = Decimal::from_ratio(
                            swap_amount.amount,
                            expected_receive_amount.receive_amount.amount,
                        );

                        let price_delta = base_price.abs_diff(current_price) / base_price;
                        let scaled_price_delta = price_delta * scalar;

                        let scaled_swap_amount = if current_price < base_price {
                            swap_amount
                                .amount
                                .mul_floor(Decimal::one() + scaled_price_delta)
                        } else {
                            swap_amount
                                .amount
                                .mul_floor(Decimal::one() - scaled_price_delta)
                        };

                        if scaled_swap_amount.is_zero() {
                            return Ok((action, conditions, messages, stats));
                        }

                        let new_swap_amount = Coin::new(
                            max(
                                scaled_swap_amount,
                                minimum_swap_amount
                                    .clone()
                                    .unwrap_or(Coin::new(0u128, swap_amount.denom.clone()))
                                    .amount,
                            ),
                            swap_amount.denom.clone(),
                        );

                        let new_minimum_receive_amount = Coin::new(
                            minimum_receive_amount.amount.mul_ceil(Decimal::from_ratio(
                                new_swap_amount.amount,
                                swap_amount.amount,
                            )),
                            minimum_receive_amount.denom.clone(),
                        );

                        (new_swap_amount, new_minimum_receive_amount)
                    }
                };

                if new_swap_amount.amount.eq(&Uint128::zero()) {
                    return Ok((action, conditions, messages, stats));
                }

                let swap_msg = SubMsg::reply_always(
                    Contract(exchange_contract.clone()).call(
                        to_json_binary(&ExchangeExecuteMsg::Swap {
                            minimum_receive_amount: new_minimum_receive_amount.clone(),
                            maximum_slippage_bps: *maximum_slippage_bps,
                            route: route.clone(),
                            recipient: None,
                            on_complete: None,
                        })?,
                        vec![new_swap_amount.clone()],
                    ),
                    0,
                );

                messages.push(swap_msg);

                let mut swap_conditions = vec![
                    Condition::BalanceAvailable {
                        address: env.contract.address.clone(),
                        amount: Coin::new(1u128, swap_amount.denom.clone()),
                    },
                    Condition::CanSwap {
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

                stats.add(Statistics {
                    swapped: vec![new_swap_amount],
                    ..stats.clone()
                });
            }
            Action::Order {
                pair_address,
                side,
                bid_denom,
                bid_amount,
                strategy,
                current_price,
                schedule,
            } => {
                let existing_order =
                    strategy.existing_order(deps, env, pair_address, side, current_price);

                let bid_denom_balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), bid_denom.clone())?
                    .amount;

                let remaining = bid_denom_balance
                    + existing_order
                        .clone()
                        .map_or(Uint128::zero(), |o| o.remaining);

                let new_rate = match strategy {
                    OrderPriceStrategy::Fixed { price } => price.clone(),
                    OrderPriceStrategy::Offset {
                        direction, offset, ..
                    } => {
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

                        let new_price = match offset {
                            Offset::Exact(offset) => match direction {
                                Direction::Up => book_price.saturating_add(*offset),
                                Direction::Down => book_price.saturating_sub(*offset),
                            },
                            Offset::Bps(offset) => match direction {
                                Direction::Up => book_price.saturating_mul(
                                    Decimal::one().saturating_add(Decimal::bps(*offset)),
                                ),
                                Direction::Down => book_price.saturating_mul(
                                    Decimal::one().saturating_sub(Decimal::bps(*offset)),
                                ),
                            },
                        };

                        new_price
                    }
                };

                let new_price = Price::Fixed(new_rate);
                let new_bid_amount = min(bid_amount.unwrap_or(remaining), remaining);

                let mut orders = existing_order.map_or_else(
                    || vec![],
                    |o| vec![(o.side, o.price, Some(Uint128::zero()))],
                );

                if new_bid_amount.gt(&Uint128::zero()) && new_rate.gt(&Decimal::zero()) {
                    orders.push((side.clone(), new_price.clone(), Some(new_bid_amount)));

                    let order_filled_condition = Condition::LimitOrderFilled {
                        pair_address: pair_address.clone(),
                        owner: env.contract.address.clone(),
                        side: side.clone(),
                        price: new_price.clone(),
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

                let set_order_msg = SubMsg::reply_always(
                    Contract(pair_address.clone()).call(
                        to_json_binary(&ExecuteMsg::Order((orders, None)))?,
                        vec![Coin::new(new_bid_amount, bid_denom.clone())],
                    ),
                    0,
                );

                messages.push(set_order_msg);

                // if filled.gt(&Uint128::zero()) {
                //     let pair = deps
                //         .querier
                //         .query_wasm_smart::<ConfigResponse>(pair_address, &QueryMsg::Config {})?;

                //     let filled_amount = Coin::new(filled, pair.denoms.ask(side));

                //     let distribute_msg = SubMsg::reply_never(BankMsg::Send {
                //         to_address: config.distributor.to_string(),
                //         amount: vec![filled_amount.clone()],
                //     });

                //     messages.push(distribute_msg);

                //     stats.add(Statistics {
                //         filled: vec![filled_amount],
                //         ..stats.clone()
                //     });
                // }

                action = Action::Order {
                    pair_address: pair_address.clone(),
                    bid_denom: bid_denom.clone(),
                    bid_amount: bid_amount.clone(),
                    side: side.clone(),
                    strategy: strategy.clone(),
                    current_price: Some(new_price),
                    schedule: schedule.clone(),
                };
            }
            Action::Distribute {
                denoms,
                mutable_destinations,
                immutable_destinations,
                conditions,
            } => {
                if conditions.iter().all(|c| c.check(deps, &env).is_ok()) {
                    let destinations = mutable_destinations
                        .iter()
                        .chain(immutable_destinations.iter());

                    let total_shares = destinations
                        .clone()
                        .fold(Uint128::zero(), |acc, d| acc + d.shares);

                    for denom in denoms.clone() {
                        let balance = deps.querier.query_balance(&env.contract.address, &denom)?;

                        if balance.amount.is_zero() {
                            continue;
                        }

                        for destination in destinations.clone() {
                            let amount = vec![Coin::new(
                                balance.amount.mul_floor(Decimal::from_ratio(
                                    destination.shares,
                                    total_shares,
                                )),
                                balance.denom.clone(),
                            )];

                            let message = match destination.recipient.clone() {
                                Recipient::Bank { address, .. } => CosmosMsg::Bank(BankMsg::Send {
                                    to_address: address.into(),
                                    amount: amount.clone(),
                                }),
                                Recipient::Wasm { address, msg, .. } => {
                                    CosmosMsg::Wasm(WasmMsg::Execute {
                                        contract_addr: address.into(),
                                        msg,
                                        funds: amount.clone(),
                                    })
                                }
                                Recipient::Deposit { memo } => MsgDeposit {
                                    memo: memo,
                                    coins: amount.clone(),
                                    signer: deps
                                        .api
                                        .addr_canonicalize(env.contract.address.as_str())?,
                                }
                                .into_cosmos_msg()?,
                            };

                            messages.push(SubMsg::reply_always(message, 0));
                        }
                    }
                }
            }
        };

        Ok((action, conditions, messages, stats))
    }

    pub fn escrowed(&self, deps: Deps) -> StdResult<Vec<String>> {
        Ok(match self {
            Action::Distribute { denoms, .. } => denoms.clone(),
            Action::Swap { swap_amount, .. } => vec![swap_amount.denom.clone()],
            Action::Order {
                pair_address, side, ..
            } => {
                let pair = deps
                    .querier
                    .query_wasm_smart::<ConfigResponse>(pair_address, &QueryMsg::Config {})?;

                vec![pair.denoms.ask(side).to_string()] // TODO: Test!
            }
        })
    }

    pub fn balances(&self, deps: Deps, env: &Env) -> StdResult<Coins> {
        Ok(match self {
            Action::Order {
                pair_address,
                side,
                bid_denom,
                bid_amount: _,
                strategy,
                current_price,
                schedule: _,
            } => {
                let existing_order =
                    strategy.existing_order(deps, env, pair_address, side, current_price);

                let pair = deps
                    .querier
                    .query_wasm_smart::<ConfigResponse>(pair_address, &QueryMsg::Config {})?;

                existing_order.map_or(Ok(Coins::default()), |o| {
                    Coins::try_from(vec![
                        Coin::new(o.remaining, bid_denom),
                        Coin::new(o.filled, pair.denoms.ask(side)),
                    ])
                })?
            }
            Action::Swap { .. } | Action::Distribute { .. } => Coins::default(),
        })
    }

    pub fn withdraw(
        &self,
        deps: Deps,
        env: &Env,
        // config: &StrategyConfig,
        desired: &Coins,
    ) -> StdResult<(Coins, Vec<SubMsg>)> {
        let mut withdrawn = Coins::default();
        let mut messages = vec![];

        match self {
            Action::Order {
                pair_address,
                bid_denom,
                side,
                strategy,
                current_price,
                ..
            } => {
                let optimistic_withdraw_amount = desired.amount_of(&bid_denom);

                if optimistic_withdraw_amount.is_zero() {
                    return Ok((withdrawn, messages));
                }

                let order = strategy.existing_order(deps, env, pair_address, side, current_price);

                if let Some(order) = order {
                    let actual_withdraw_amount = min(order.remaining, optimistic_withdraw_amount);
                    let new_bid_amount = order.remaining.saturating_sub(actual_withdraw_amount);

                    let withdraw_order_msg = SubMsg::reply_always(
                        Contract(pair_address.clone()).call(
                            to_json_binary(&ExecuteMsg::Order((
                                vec![(side.clone(), order.price, Some(new_bid_amount))],
                                None,
                            )))?,
                            vec![],
                        ),
                        0,
                    );

                    messages.push(withdraw_order_msg);
                    withdrawn.add(Coin::new(actual_withdraw_amount, bid_denom.clone()))?;
                }
            }
            _ => {}
        };

        Ok((withdrawn, messages))
    }
}

#[cw_serde]
pub struct Behaviour {
    pub actions: Vec<Action>,
    pub conditions: Vec<Condition>,
    pub statistics: Statistics,
}

impl Behaviour {
    pub fn execute(&mut self, deps: Deps, env: &Env) -> StdResult<Vec<SubMsg>> {
        let mut new_actions = vec![];
        let mut new_conditions = vec![];
        let mut all_messages = vec![];

        for action in &self.actions {
            let (action, conditions, messages, stats) = action.execute(deps, env)?;

            new_actions.push(action);
            new_conditions.extend(conditions);
            all_messages.extend(messages);

            self.statistics.add(stats);
        }

        self.actions = new_actions;
        self.conditions = new_conditions;

        Ok(all_messages)
    }

    pub fn withdraw(
        &mut self,
        deps: Deps,
        env: &Env,
        config: &StrategyConfig,
        desired: &mut Coins,
    ) -> StdResult<Vec<SubMsg>> {
        let mut actual = Coins::default();
        let mut sub_messages: Vec<SubMsg> = vec![];

        for action in &self.actions {
            let (withdrawal_amounts, withdrawal_messages) =
                action.withdraw(deps, &env, &desired)?;

            for withdrawal_amount in withdrawal_amounts {
                actual.add(withdrawal_amount.clone())?;
                desired.sub(withdrawal_amount.clone())?;
            }

            sub_messages.extend(withdrawal_messages);

            if desired.is_empty() {
                break;
            }
        }

        self.statistics.add(Statistics {
            withdrawn: actual.to_vec(),
            ..self.statistics.clone()
        });

        let bank_msg = SubMsg::reply_never(BankMsg::Send {
            to_address: config.owner.to_string(),
            amount: actual.to_vec(),
        });

        sub_messages.push(bank_msg);

        Ok(sub_messages)
    }
}

#[cw_serde]
pub struct StrategyConfig {
    pub owner: Addr,
    pub manager: Addr,
    pub scheduler: Addr,
    pub behaviours: Vec<Behaviour>,
    pub statistics: Statistics,
    pub execution_rebate: Vec<Coin>,
}

impl StrategyConfig {
    pub fn init(&mut self, deps: Deps, affiliates: &Vec<Affiliate>) -> StdResult<()> {
        for behaviour in self.behaviours.iter_mut() {
            let mut valid_actions = vec![];

            for action in behaviour.actions.iter() {
                valid_actions.push(action.init(deps, affiliates)?);
            }

            behaviour.actions = valid_actions;
        }

        Ok(())
    }

    pub fn execute(&mut self, deps: Deps, env: &Env) -> StdResult<Vec<SubMsg>> {
        let mut messages: Vec<SubMsg> = vec![];

        for behaviour in self.behaviours.clone().iter_mut() {
            if behaviour
                .conditions
                .iter()
                .all(|c| c.check(deps, env).is_ok())
            {
                messages.extend(behaviour.execute(deps, env)?);
            }
        }

        Ok(messages)
    }

    pub fn escrowed(&self, deps: Deps) -> StdResult<HashSet<String>> {
        let mut escrowed: HashSet<String> = HashSet::new();

        for behaviour in self.behaviours.iter() {
            for action in behaviour.actions.iter() {
                let action_escrowed = action.escrowed(deps)?;
                for denom in action_escrowed {
                    escrowed.insert(denom);
                }
            }
        }

        Ok(escrowed)
    }

    pub fn balances(&self, deps: Deps, env: &Env, include: Vec<String>) -> StdResult<Vec<Coin>> {
        let mut balances = Coins::default();

        for denom in include {
            let balance = deps
                .querier
                .query_balance(env.contract.address.clone(), denom)?;

            balances.add(balance)?;
        }

        for behaviour in self.behaviours.iter() {
            for action in behaviour.actions.iter() {
                for balance in action.balances(deps, env)? {
                    balances.add(balance)?;
                }
            }
        }

        Ok(balances.to_vec())
    }

    pub fn withdraw(
        &mut self,
        deps: Deps,
        env: &Env,
        mut desired: Coins,
    ) -> StdResult<Vec<SubMsg>> {
        let mut messages: Vec<SubMsg> = vec![];
        let mut actual = Coins::default();

        let escrowed_denoms = self.escrowed(deps)?;

        for desired_amount in desired.to_vec() {
            if escrowed_denoms.contains(&desired_amount.denom) {
                return Err(StdError::generic_err(format!(
                    "Cannot withdraw escrowed denom: {}",
                    desired_amount.denom
                )));
            }

            let balance = deps
                .querier
                .query_balance(env.contract.address.clone(), desired_amount.denom.clone())?;

            let withdrawal_amount = Coin::new(
                min(balance.amount, desired_amount.amount),
                desired_amount.denom.clone(),
            );

            actual.add(withdrawal_amount.clone())?;
            desired.sub(withdrawal_amount.clone())?;

            self.statistics.add(Statistics {
                withdrawn: vec![withdrawal_amount],
                ..self.statistics.clone()
            });
        }

        let bank_msg = SubMsg::reply_never(BankMsg::Send {
            to_address: self.owner.to_string(),
            amount: actual.to_vec(),
        });

        messages.push(bank_msg);

        if !desired.is_empty() {
            for mut behaviour in self.behaviours.clone().into_iter() {
                let withdrawal_messages = behaviour.withdraw(deps, &env, &self, &mut desired)?;

                messages.extend(withdrawal_messages);

                if desired.is_empty() {
                    break;
                }
            }
        }

        Ok(messages)
    }
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

        assert!(Condition::TimeElapsed(Timestamp::from_seconds(0))
            .check(deps.as_ref(), &env)
            .is_ok());

        assert!(Condition::TimeElapsed(env.block.time)
            .check(deps.as_ref(), &env)
            .is_ok());

        assert!(Condition::TimeElapsed(env.block.time.plus_seconds(1))
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

        assert!(Condition::CanSwap {
            exchanger_contract: env.contract.address.clone(),
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(101u128, "rune"),
            maximum_slippage_bps: 10,
            route: None,
        }
        .check(deps.as_ref(), &env)
        .is_err());

        assert!(Condition::CanSwap {
            exchanger_contract: env.contract.address.clone(),
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(100u128, "rune"),
            maximum_slippage_bps: 9,
            route: None,
        }
        .check(deps.as_ref(), &env)
        .is_err());

        assert!(Condition::CanSwap {
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
            Condition::TimeElapsed(Timestamp::from_seconds(env.block.time.seconds()))
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time)
            }
            .into_condition(&env),
            Condition::TimeElapsed(Timestamp::from_seconds(env.block.time.seconds() + 10))
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(5))
            }
            .into_condition(&env),
            Condition::TimeElapsed(Timestamp::from_seconds(env.block.time.seconds() - 5 + 10))
        );

        assert_eq!(
            Schedule::Time {
                duration: Duration::from_secs(10),
                previous: Some(env.block.time.minus_seconds(155))
            }
            .into_condition(&env),
            Condition::TimeElapsed(Timestamp::from_seconds(env.block.time.seconds() - 155 + 10))
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
