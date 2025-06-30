use std::collections::HashMap;

use crate::{core::Condition, thorchain::MsgDeposit};
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    to_json_string, Addr, BankMsg, Binary, Coin, CosmosMsg, Deps, Env, Event, StdResult, Uint128,
    WasmMsg,
};

#[cw_serde]
pub enum Recipient {
    Bank { address: Addr },
    Wasm { address: Addr, msg: Binary },
    Deposit { memo: String },
}

impl Recipient {
    pub fn key(&self) -> String {
        match self {
            Recipient::Bank { address } | Recipient::Wasm { address, .. } => address.to_string(),
            Recipient::Deposit { memo } => memo.clone(),
        }
    }
}

#[cw_serde]
pub struct Destination {
    pub shares: Uint128,
    pub recipient: Recipient,
    pub label: Option<String>,
}

#[cw_serde]
pub struct Distribution {
    pub destination: Destination,
    pub amount: Vec<Coin>,
}

impl Distribution {
    pub fn get_msg(self, deps: Deps, env: &Env) -> StdResult<CosmosMsg> {
        match self.destination.recipient {
            Recipient::Bank { address, .. } => Ok(BankMsg::Send {
                to_address: address.into(),
                amount: self.amount,
            }
            .into()),
            Recipient::Wasm { address, msg, .. } => Ok(WasmMsg::Execute {
                contract_addr: address.into(),
                msg,
                funds: self.amount,
            }
            .into()),
            Recipient::Deposit { memo } => Ok(MsgDeposit {
                memo: memo,
                coins: self.amount,
                signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
            }
            .into_cosmos_msg()?),
        }
    }
}

#[cw_serde]
pub struct DistributorConfig {
    pub owner: Addr,
    pub denoms: Vec<String>,
    pub mutable_destinations: Vec<Destination>,
    pub immutable_destinations: Vec<Destination>,
    pub conditions: Vec<Condition>,
}

#[cw_serde]
pub struct DistributorStatistics {
    pub distributed: HashMap<String, Vec<Coin>>,
    pub withdrawn: Vec<Coin>,
}

#[cw_serde]
pub struct DistributorInstantiateMsg {
    pub owner: Addr,
    pub denoms: Vec<String>,
    pub mutable_destinations: Vec<Destination>,
    pub immutable_destinations: Vec<Destination>,
}

#[cw_serde]
pub enum DistributorExecuteMsg {
    Distribute {},
    Withdraw { amounts: Vec<Coin> },
    Update(DistributorConfig),
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum DistributorQueryMsg {
    #[returns(DistributorConfig)]
    Config {},
    #[returns(DistributorStatistics)]
    Statistics {},
}

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

#[cfg(test)]
mod distribution_tests {
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        to_json_binary, Api, BankMsg, Coin, CosmosMsg, Uint128, WasmMsg,
    };

    use crate::{
        distributor::{Destination, Distribution, Recipient},
        thorchain::MsgDeposit,
    };

    #[test]
    fn builds_accurate_distribution_msg() {
        let deps = mock_dependencies();
        let env = mock_env();
        let address = deps.api.addr_make("cosmos1xyz");
        let amount = Coin::new(100u128, "gaia-atom");

        assert_eq!(
            Distribution {
                amount: vec![amount.clone()],
                destination: Destination {
                    recipient: Recipient::Bank {
                        address: address.clone()
                    },
                    shares: Uint128::new(10000),
                    label: None,
                },
            }
            .get_msg(deps.as_ref(), &env)
            .unwrap(),
            CosmosMsg::Bank(BankMsg::Send {
                to_address: address.to_string(),
                amount: vec![amount.clone()],
            })
        );

        let msg = to_json_binary(&"test-message").unwrap();

        assert_eq!(
            Distribution {
                amount: vec![amount.clone()],
                destination: Destination {
                    recipient: Recipient::Wasm {
                        address: address.clone(),
                        msg: msg.clone()
                    },
                    shares: Uint128::new(10000),
                    label: None,
                },
            }
            .get_msg(deps.as_ref(), &env)
            .unwrap(),
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: address.to_string(),
                msg: msg.clone(),
                funds: vec![amount.clone()],
            })
        );

        let memo = "=:random-memo:test".to_string();

        assert_eq!(
            Distribution {
                amount: vec![amount.clone()],
                destination: Destination {
                    recipient: Recipient::Deposit { memo: memo.clone() },
                    shares: Uint128::new(10000),
                    label: None
                }
            }
            .get_msg(deps.as_ref(), &env)
            .unwrap(),
            MsgDeposit {
                memo,
                coins: vec![amount],
                signer: deps
                    .api
                    .addr_canonicalize(env.contract.address.as_str())
                    .unwrap()
            }
            .into_cosmos_msg()
            .unwrap()
        );
    }
}
