use calc_rs::{
    core::Condition,
    stoploss::{StopLossConfig, StopLossStatistics},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_string, Addr, Coin, Event};
use rujira_rs::fin::{Price, Side};

#[cw_serde]
pub struct Statistics {
    pub swapped: Coin,
    pub withdrawn: Vec<Coin>,
}

#[cw_serde]
pub enum DomainEvent {
    StrategyCreated {
        contract_address: Addr,
        config: StopLossConfig,
    },
    StrategyUpdated {
        contract_address: Addr,
        old_config: StopLossConfig,
        new_config: StopLossConfig,
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
    ExecutionAttempted {
        contract_address: Addr,
        pair_address: Addr,
        side: Side,
        price: Price,
    },
    ExecutionSucceeded {
        contract_address: Addr,
        statistics: StopLossStatistics,
    },
    ExecutionFailed {
        contract_address: Addr,
        reason: String,
    },
    SchedulingAttempted {
        contract_address: Addr,
        conditions: Vec<Condition>,
    },
    ExecutionSkipped {
        contract_address: Addr,
        reason: String,
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
            DomainEvent::FundsDeposited {
                contract_address,
                from,
                funds,
            } => Event::new("funds_deposited")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("from", from.as_str())
                .add_attribute(
                    "funds",
                    to_json_string(&funds).expect("Failed to serialize funds"),
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
