use calc_rs::types::Distribution;
use cosmwasm_std::{to_json_string, Addr, Coin, Event};

pub enum DomainEvent {
    FundsDistributed {
        contract_address: Addr,
        to: Vec<Distribution>,
    },
    FundsWithdrawn {
        contract_address: Addr,
        to: Addr,
        funds: Vec<Coin>,
    },
}

impl From<DomainEvent> for Event {
    fn from(event: DomainEvent) -> Self {
        match event {
            DomainEvent::FundsDistributed {
                contract_address,
                to,
            } => Event::new("funds_distributed")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "to",
                    to_json_string(&to).expect("Failed to serialize distribution"),
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
        }
    }
}
