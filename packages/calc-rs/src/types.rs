use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Binary, Coin, CosmosMsg, Event, Response, StdError, StdResult, WasmMsg};
use cw_utils::{Duration, Expiration};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},
}

pub type ContractResult = core::result::Result<Response, ContractError>;

#[cw_serde]
pub enum Schedule {
    Regular {
        duration: Duration,
        start_time: Option<Expiration>,
    },
}

#[derive()]
#[cw_serde]
pub enum StrategyConfig {
    Dca {
        owner: Addr,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
        exchange_contract: Addr,
        interval_blocks: u64,
        next_execution_block: Option<u64>,
    },
    New {},
}

#[cw_serde]
pub enum StrategyStatus {
    Active,
    Paused,
    Archived,
}

#[cw_serde]
pub struct Strategy {
    config: StrategyConfig,
    status: StrategyStatus,
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
        config: StrategyConfig,
    },
    FundsDeposited {
        contract_address: Addr,
        from: Addr,
        amount: Vec<Coin>,
    },
    FundsWithdrawn {
        contract_address: Addr,
        to: Addr,
        amount: Vec<Coin>,
    },
    ExecutionSucceeded {
        contract_address: Addr,
    },
    ExecutionFailed {
        contract_address: Addr,
        reason: String,
    },
    ExecutionSkipped {
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
                config,
            } => Event::new("strategy_updated")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("config", format!("{:?}", config)),
            DomainEvent::FundsDeposited {
                contract_address,
                from,
                amount,
            } => Event::new("funds_deposited")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("from", from.as_str())
                .add_attribute("amount", format!("{:?}", amount)),
            DomainEvent::FundsWithdrawn {
                contract_address,
                to,
                amount,
            } => Event::new("funds_withdrawn")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("to", to.as_str())
                .add_attribute("amount", format!("{:?}", amount)),
            DomainEvent::ExecutionSucceeded { contract_address } => {
                Event::new("execution_succeeded")
                    .add_attribute("contract_address", contract_address.as_str())
            }
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
