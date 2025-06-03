use std::{time::Duration, u8};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, Binary, CheckedFromRatioError, CheckedMultiplyRatioError, Coin, CoinsError, CosmosMsg,
    Env, Event, HexBinary, Instantiate2AddressError, OverflowError, Response, StdError, StdResult,
    Timestamp, Uint128, WasmMsg,
};
use cw_storage_plus::{Key, Prefixer, PrimaryKey};
use rujira_rs::CallbackData;
use thiserror::Error;

#[derive(Error, Debug)]
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
    Generic(String),
}

pub type ContractResult = Result<Response, ContractError>;

#[cw_serde]
pub struct ManagerConfig {
    pub admin: Addr,
    pub checksum: HexBinary,
    pub code_id: u64,
}

#[cw_serde]
pub enum Condition {
    Timestamp {
        timestamp: Timestamp,
    },
    BlockHeight {
        height: u64,
    },
    LimitOrder {
        swap_amount: Coin,
        minimum_receive_amount: Coin,
    },
}

#[cw_serde]
pub enum ConditionFilter {
    Owner {
        address: Addr,
    },
    Timestamp {
        start: Option<Timestamp>,
        end: Option<Timestamp>,
    },
    BlockHeight {
        start: Option<u64>,
        end: Option<u64>,
    },
    LimitOrder {},
}

#[cw_serde]
pub struct Trigger {
    pub id: u64,
    pub owner: Addr,
    pub condition: Condition,
    pub callback: CallbackData,
    pub to: Addr,
    pub execution_rebate: Vec<Coin>,
}

pub trait Executable {
    fn can_execute(&self, env: Env) -> bool;
    fn execute(&self, env: Env) -> ContractResult;
}

impl Executable for Trigger {
    fn can_execute(&self, env: Env) -> bool {
        match self.condition {
            Condition::BlockHeight { height } => height > env.block.height,
            Condition::LimitOrder { .. } => false,
            Condition::Timestamp { timestamp } => timestamp > env.block.time,
        }
    }

    fn execute(&self, env: Env) -> ContractResult {
        if !self.can_execute(env) {
            return Err(ContractError::Std(StdError::generic_err(format!(
                "Condition not met: {:?}",
                self.condition
            ))));
        }

        let mut messages: Vec<CosmosMsg> = vec![];

        match self.condition {
            Condition::Timestamp { .. } | Condition::BlockHeight { .. } => {
                let execute_message = Contract(self.to.clone())
                    .call(self.callback.clone().into_json_binary(), vec![])?;

                messages.push(execute_message);
            }
            Condition::LimitOrder { .. } => {
                return Err(ContractError::Std(StdError::generic_err(
                    "Limit order condition not implemented",
                )));
            }
        }

        Ok(Response::default().add_messages(messages))
    }
}

#[cw_serde]
pub struct DcaStatistics {
    pub amount_deposited: Coin,
    pub amount_swapped: Coin,
    pub amount_received: Coin,
}

#[cw_serde]
pub struct NewStatistics {
    pub amount: Coin,
}

#[cw_serde]
pub enum StrategyStatistics {
    Dca(DcaStatistics),
    New(NewStatistics),
}

#[cw_serde]
pub struct Destination {
    pub address: Addr,
    pub shares: Uint128,
    pub msg: Option<Binary>,
    pub label: Option<String>,
}

#[cw_serde]
pub enum DcaSchedule {
    Blocks {
        interval: u64,
        previous: Option<u64>,
    },
    Time {
        duration: Duration,
        previous: Option<Timestamp>,
    },
}

#[cw_serde]
pub struct DcaStrategyConfig {
    pub owner: Addr,
    pub swap_amount: Coin,
    pub minimum_receive_amount: Coin,
    pub schedule: DcaSchedule,
    pub exchange_contract: Addr,
    pub scheduler_contract: Addr,
    pub fee_collector: Addr,
    pub affiliate_code: Option<String>,
    pub mutable_destinations: Vec<Destination>,
    pub immutable_destinations: Vec<Destination>,
    pub statistics: DcaStatistics,
}

#[cw_serde]
pub struct CustomStrategyConfig {
    pub owner: Addr,
}

#[derive()]
#[cw_serde]
pub enum StrategyConfig {
    Dca(DcaStrategyConfig),
    Custom(CustomStrategyConfig),
}

pub trait Owned {
    fn owner(&self) -> Addr;
}

impl Owned for StrategyConfig {
    fn owner(&self) -> Addr {
        match self {
            StrategyConfig::Dca(dca_strategy) => dca_strategy.owner.clone(),
            StrategyConfig::Custom(new_strategy) => new_strategy.owner.clone(),
        }
    }
}

#[cw_serde]
pub enum Status {
    Active,
    Paused,
    Archived,
}

impl<'a> Prefixer<'a> for Status {
    fn prefix(&self) -> Vec<Key> {
        vec![Key::Val8([self.clone() as u8])]
    }
}

impl<'a> PrimaryKey<'a> for Status {
    type Prefix = Self;
    type SubPrefix = Self;
    type Suffix = ();
    type SuperSuffix = ();

    fn key(&self) -> Vec<Key> {
        vec![Key::Val8([self.clone() as u8])]
    }
}

#[cw_serde]
pub struct Affiliate {
    pub code: String,
    pub address: Addr,
    pub bps: u64,
}

#[cw_serde]
pub struct Strategy {
    pub owner: Addr,
    pub contract_address: Addr,
    pub created_at: u64,
    pub updated_at: u64,
    pub executions: u64,
    pub label: String,
    pub status: Status,
    pub affiliates: Vec<Affiliate>,
}

pub enum DomainEvent {
    StrategyCreated {
        contract_address: Addr,
        config: StrategyConfig,
    },
    StrategyPaused {
        contract_address: Addr,
        reason: String,
    },
    StrategyArchived {
        contract_address: Addr,
    },
    StrategyResumed {
        contract_address: Addr,
    },
    StrategyUpdated {
        contract_address: Addr,
        old_config: StrategyConfig,
        new_config: StrategyConfig,
    },
    FundsDeposited {
        contract_address: Addr,
        from: Addr,
        funds: Vec<Coin>,
    },
    FundsWithdrawn {
        contract_address: Addr,
        to: Addr,
        funds: Vec<Coin>,
    },
    ExecutionSucceeded {
        contract_address: Addr,
        statistics: StrategyStatistics,
    },
    ExecutionFailed {
        contract_address: Addr,
        reason: String,
    },
    ExecutionSkipped {
        contract_address: Addr,
        reason: String,
    },
    SchedulingSucceeded {
        contract_address: Addr,
        conditions: Vec<Condition>,
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
            } => Event::new("strategy_created")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("config", format!("{:?}", config)),
            DomainEvent::StrategyPaused {
                contract_address,
                reason,
            } => Event::new("strategy_paused")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
            DomainEvent::StrategyResumed { contract_address } => Event::new("strategy_resumed")
                .add_attribute("contract_address", contract_address.as_str()),
            DomainEvent::StrategyArchived { contract_address } => Event::new("strategy_archived")
                .add_attribute("contract_address", contract_address.as_str()),
            DomainEvent::StrategyUpdated {
                contract_address,
                old_config,
                new_config,
            } => Event::new("strategy_updated")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("old_config", format!("{:?}", old_config))
                .add_attribute("new_config", format!("{:?}", new_config)),
            DomainEvent::FundsDeposited {
                contract_address,
                from,
                funds: amount,
            } => Event::new("funds_deposited")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("from", from.as_str())
                .add_attribute("amount", format!("{:?}", amount)),
            DomainEvent::FundsWithdrawn {
                contract_address,
                to,
                funds: amount,
            } => Event::new("funds_withdrawn")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("to", to.as_str())
                .add_attribute("amount", format!("{:?}", amount)),
            DomainEvent::ExecutionSucceeded {
                contract_address,
                statistics,
            } => Event::new("execution_succeeded")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("statistics", format!("{:?}", statistics)),
            DomainEvent::ExecutionFailed {
                contract_address,
                reason: error,
            } => Event::new("execution_failed")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("error", error),
            DomainEvent::ExecutionSkipped {
                contract_address,
                reason,
            } => Event::new("execution_skipped")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
            DomainEvent::SchedulingSucceeded {
                contract_address,
                conditions,
            } => Event::new("scheduling_succeeded")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("conditions", format!("{:?}", conditions)),
            DomainEvent::SchedulingFailed {
                contract_address,
                reason: error,
            } => Event::new("scheduling_failed")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("error", error),
            DomainEvent::SchedulingSkipped {
                contract_address,
                reason,
            } => Event::new("scheduling_skipped")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
        }
    }
}

pub struct Contract(pub Addr);

impl Contract {
    pub fn addr(&self) -> Addr {
        self.0.clone()
    }

    pub fn call(&self, msg: Binary, funds: Vec<Coin>) -> StdResult<CosmosMsg> {
        Ok(WasmMsg::Execute {
            contract_addr: self.addr().into(),
            msg,
            funds,
        }
        .into())
    }
}
