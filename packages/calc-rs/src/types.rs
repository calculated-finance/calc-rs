use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, Binary, CheckedMultiplyRatioError, Coin, CosmosMsg, Event, Instantiate2AddressError,
    Response, StdError, StdResult, Timestamp, Uint128, WasmMsg,
};
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

    #[error("Unauthorized")]
    Unauthorized {},
}

pub type ContractResult = Result<Response, ContractError>;

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
}

#[cw_serde]
pub struct DcaStatistics {
    pub amount_deposited: Coin,
    pub amount_withdrawn: Coin,
    pub amount_swapped: Coin,
    pub amount_received: Coin,
}

#[cw_serde]
pub struct NewStatistics {
    pub amount: Coin,
}

pub enum StrategyStatistics {
    Dca(DcaStatistics),
    New(NewStatistics),
}

#[cw_serde]
pub struct Destination {
    pub address: Addr,
    pub shares: Uint128,
    pub label: Option<String>,
}

#[cw_serde]
pub struct DcaStrategy {
    pub owner: Addr,
    pub swap_amount: Coin,
    pub minimum_receive_amount: Coin,
    pub interval_blocks: u64,
    pub exchange_contract: Addr,
    pub scheduler_contract: Addr,
    pub fee_collector: Addr,
    pub conditions: Vec<Condition>,
    pub mutable_destinations: Vec<Destination>,
    pub immutable_destinations: Vec<Destination>,
    pub statistics: DcaStatistics,
}

#[cw_serde]
pub struct NewStrategy {
    pub owner: Addr,
}

#[derive()]
#[cw_serde]
pub enum StrategyConfig {
    Dca(DcaStrategy),
    New(NewStrategy),
}

pub trait Owned {
    fn owner(&self) -> Addr;
}

impl Owned for StrategyConfig {
    fn owner(&self) -> Addr {
        match self {
            StrategyConfig::Dca(dca_strategy) => dca_strategy.owner.clone(),
            StrategyConfig::New(new_strategy) => new_strategy.owner.clone(),
        }
    }
}

#[cw_serde]
pub enum Status {
    Active,
    Paused,
    Archived,
}

#[cw_serde]
pub struct Strategy {
    owner: Addr,
    contract_address: Addr,
    created_at: Timestamp,
    updated_at: Timestamp,
    label: String,
    status: Status,
    config: StrategyConfig,
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
