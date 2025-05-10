use calc_rs::types::StrategyConfig;
use cosmwasm_std::{Addr, Event};

pub enum DomainEvent {
    /// Event emitted when a new strategy is created
    StrategyCreated {
        contract_address: Addr,
        config: StrategyConfig,
    },
    /// Event emitted when a strategy is executed
    StrategyExecuted { contract_address: Addr },
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
            DomainEvent::StrategyExecuted { contract_address } => Event::new("strategy_executed")
                .add_attribute("contract_address", contract_address.as_str()),
        }
    }
}
