use std::{time::Duration, u8};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, Binary, CheckedFromRatioError, CheckedMultiplyRatioError, Coin, CoinsError, CosmosMsg,
    Deps, Env, Instantiate2AddressError, OverflowError, Response, StdError, StdResult, Timestamp,
    WasmMsg,
};
use rujira_rs::fin::{OrderResponse, Price, QueryMsg, Side};
use thiserror::Error;

use crate::{
    exchanger::{ExchangeQueryMsg, ExpectedReceiveAmount, Route},
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
        owner: Addr,
        pair_address: Addr,
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
                owner,
                pair_address,
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
                owner,
                pair_address,
                side,
                price,
            } => format!(
                "limit order filled: owner={}, pair_address={}, side={:?}, price={}",
                owner, pair_address, side, price
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
