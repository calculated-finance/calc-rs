use std::{time::Duration, u8};

use anybuf::Anybuf;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, AnyMsg, Binary, CanonicalAddr, CheckedFromRatioError, CheckedMultiplyRatioError, Coin,
    CoinsError, CosmosMsg, Deps, Env, Instantiate2AddressError, OverflowError, Response, StdError,
    StdResult, Timestamp, WasmMsg,
};
use cw_storage_plus::{Key, Prefixer, PrimaryKey};
use rujira_rs::{
    fin::{OrderResponse, Price, QueryMsg, Side},
    Layer1Asset, NativeAsset, SecuredAsset,
};
use thiserror::Error;

use crate::{
    exchanger::{ExchangeQueryMsg, ExpectedReceiveAmount, Route},
    manager::{ManagerQueryMsg, Strategy},
};

pub const DEPOSIT_FEE: u128 = 2_000_000; // 0.02 RUNE

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

#[cw_serde]
pub enum StrategyStatus {
    Active,
    Paused,
    Archived,
}

impl<'a> Prefixer<'a> for StrategyStatus {
    fn prefix(&self) -> Vec<Key> {
        vec![Key::Val8([self.clone() as u8])]
    }
}

impl<'a> PrimaryKey<'a> for StrategyStatus {
    type Prefix = Self;
    type SubPrefix = Self;
    type Suffix = ();
    type SuperSuffix = ();

    fn key(&self) -> Vec<Key> {
        vec![Key::Val8([self.clone() as u8])]
    }
}

#[cw_serde]
pub enum Condition {
    TimestampElapsed(Timestamp),
    BlocksCompleted(u64),
    LimitOrderFilled {
        owner: Addr,
        pair_address: Addr,
        side: Side,
        price: Price,
    },
    ExchangeLiquidityProvided {
        exchanger_contract: Addr,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
        maximum_slippage_bps: u128,
        route: Option<Route>,
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
            Condition::LimitOrderFilled {
                owner,
                pair_address,
                side,
                price,
            } => format!(
                "limit order filled: owner={}, pair_address={}, side={:?}, price={}",
                owner, pair_address, side, price
            ),
            Condition::ExchangeLiquidityProvided {
                swap_amount,
                minimum_receive_amount,
                maximum_slippage_bps,
                ..
            } => format!(
                "exchange liquidity provided: swap_amount={}, minimum_receive_amount={}, maximum_slippage_bps={}",
                swap_amount, minimum_receive_amount, maximum_slippage_bps
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
                "strategy ({}) in status: {:?}",
                contract_address, status
            ),
        }
    }
}

pub fn layer_1_asset(denom: &NativeAsset) -> StdResult<Layer1Asset> {
    let denom_string = denom.denom_string();

    if denom_string.contains("rune") {
        return Ok(Layer1Asset::new("THOR", "RUNE"));
    }

    let (chain, symbol) = denom_string
        .split_once('-')
        .ok_or_else(|| StdError::generic_err(format!("Invalid layer 1 asset: {}", denom)))?;

    Ok(Layer1Asset::new(
        &chain.to_ascii_uppercase(),
        &symbol.to_ascii_uppercase(),
    ))
}

pub fn secured_asset(asset: &Layer1Asset) -> StdResult<SecuredAsset> {
    match asset.denom_string().to_uppercase().split_once(".") {
        Some((chain, symbol)) => {
            if chain == "THOR" && symbol == "RUNE" {
                return Ok(SecuredAsset::new("THOR", "RUNE"));
            }
            Ok(SecuredAsset::new(chain, symbol))
        }
        None => Err(StdError::generic_err(format!(
            "Invalid layer 1 asset: {}",
            asset.denom_string()
        ))),
    }
}

pub struct MsgDeposit {
    pub memo: String,
    pub coins: Vec<Coin>,
    pub signer: CanonicalAddr,
}

impl From<MsgDeposit> for CosmosMsg {
    fn from(value: MsgDeposit) -> Self {
        let coins: Vec<Anybuf> = value
            .coins
            .iter()
            .map(|c| {
                let asset = layer_1_asset(&NativeAsset::new(&c.denom))
                    .unwrap()
                    .denom_string()
                    .to_ascii_uppercase();
                let (chain, symbol) = asset.split_once('.').unwrap();

                Anybuf::new()
                    .append_message(
                        1,
                        &Anybuf::new()
                            .append_string(1, chain)
                            .append_string(2, symbol)
                            .append_string(3, symbol)
                            .append_bool(4, false)
                            .append_bool(5, false)
                            .append_bool(6, c.denom.to_lowercase() != "rune"),
                    )
                    .append_string(2, c.amount.to_string())
            })
            .collect();

        let value = Anybuf::new()
            .append_repeated_message(1, &coins)
            .append_string(2, value.memo)
            .append_bytes(3, value.signer.to_vec());

        CosmosMsg::Any(AnyMsg {
            type_url: "/types.MsgDeposit".to_string(),
            value: value.as_bytes().into(),
        })
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
