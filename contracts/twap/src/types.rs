use calc_rs::{twap::TwapConfig, types::Condition};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_string, Addr, Coin, Event};

#[cw_serde]
pub struct TwapStatistics {
    pub swapped: Coin,
    pub withdrawn: Vec<Coin>,
}

#[cw_serde]
pub enum DomainEvent {
    TwapStrategyCreated {
        contract_address: Addr,
        config: TwapConfig,
    },
    TwapStrategyUpdated {
        contract_address: Addr,
        old_config: TwapConfig,
        new_config: TwapConfig,
    },
    TwapFundsDeposited {
        contract_address: Addr,
        from: Addr,
        funds: Vec<Coin>,
    },
    TwapFundsWithdrawn {
        contract_address: Addr,
        to: Addr,
        funds: Vec<Coin>,
    },
    TwapExecutionAttempted {
        contract_address: Addr,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
        maximum_slippage_bps: u128,
    },
    TwapExecutionSucceeded {
        contract_address: Addr,
        statistics: TwapStatistics,
    },
    TwapExecutionFailed {
        contract_address: Addr,
        reason: String,
    },
    TwapSchedulingAttempted {
        contract_address: Addr,
        conditions: Vec<Condition>,
    },
    TwapExecutionSkipped {
        contract_address: Addr,
        reason: String,
    },
    TwapSchedulingSucceeded {
        contract_address: Addr,
    },
    TwapSchedulingFailed {
        contract_address: Addr,
        reason: String,
    },
    TwapSchedulingSkipped {
        contract_address: Addr,
        reason: String,
    },
}

impl From<DomainEvent> for Event {
    fn from(event: DomainEvent) -> Self {
        match event {
            DomainEvent::TwapStrategyCreated {
                contract_address,
                config,
            } => Event::new("twap_strategy_created")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "config",
                    to_json_string(&config).expect("Failed to serialize config"),
                ),
            DomainEvent::TwapStrategyUpdated {
                contract_address,
                old_config,
                new_config,
            } => Event::new("twap_strategy_updated")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "old_config",
                    to_json_string(&old_config).expect("Failed to serialize old config"),
                )
                .add_attribute(
                    "new_config",
                    to_json_string(&new_config).expect("Failed to serialize new config"),
                ),
            DomainEvent::TwapFundsDeposited {
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
            DomainEvent::TwapFundsWithdrawn {
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
            DomainEvent::TwapExecutionAttempted {
                contract_address,
                swap_amount,
                minimum_receive_amount,
                maximum_slippage_bps,
            } => Event::new("execution_attempted")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("swap_amount", swap_amount.to_string())
                .add_attribute("minimum_receive_amount", minimum_receive_amount.to_string())
                .add_attribute("maximum_slippage_bps", maximum_slippage_bps.to_string()),
            DomainEvent::TwapExecutionSucceeded {
                contract_address,
                statistics,
            } => Event::new("execution_succeeded")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "statistics",
                    to_json_string(&statistics).expect("Failed to serialize statistics"),
                ),
            DomainEvent::TwapExecutionFailed {
                contract_address,
                reason,
            } => Event::new("execution_failed")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
            DomainEvent::TwapExecutionSkipped {
                contract_address,
                reason,
            } => Event::new("execution_skipped")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
            DomainEvent::TwapSchedulingAttempted {
                contract_address,
                conditions,
            } => Event::new("scheduling_attempted")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "conditions",
                    to_json_string(&conditions).expect("Failed to serialize conditions"),
                ),
            DomainEvent::TwapSchedulingSucceeded { contract_address } => {
                Event::new("scheduling_succeeded")
                    .add_attribute("contract_address", contract_address.as_str())
            }
            DomainEvent::TwapSchedulingFailed {
                contract_address,
                reason,
            } => Event::new("scheduling_failed")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
            DomainEvent::TwapSchedulingSkipped {
                contract_address,
                reason,
            } => Event::new("scheduling_skipped")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
        }
    }
}
