use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, BankMsg, Binary, Coin, Coins, CosmosMsg, Decimal, Deps, Env, Event, StdError, StdResult,
    Uint128, WasmMsg,
};

use crate::actions::action::Action;
use crate::actions::operation::Operation;
use crate::statistics::Statistics;
use crate::strategy::{StrategyMsg, StrategyMsgPayload};
use crate::thorchain::MsgDeposit;

enum DistributionEvent {
    DistributionSkipped {
        reason: String,
    },
    Distribute {
        recipient: String,
        amount: Vec<Coin>,
    },
}

impl Into<Event> for DistributionEvent {
    fn into(self) -> Event {
        match self {
            DistributionEvent::DistributionSkipped { reason } => {
                Event::new("distribution_skipped").add_attribute("reason", reason)
            }
            DistributionEvent::Distribute { recipient, amount } => Event::new("distribute")
                .add_attribute("recipient", recipient)
                .add_attribute("amount", format!("{:?}", amount)),
        }
    }
}

#[cw_serde]
pub enum Recipient {
    Bank { address: Addr },
    Wasm { address: Addr, msg: Binary },
    Deposit { memo: String },
    Strategy { contract_address: Addr },
}

impl Recipient {
    pub fn key(&self) -> String {
        match self {
            Recipient::Bank { address }
            | Recipient::Wasm { address, .. }
            | Recipient::Strategy {
                contract_address: address,
            } => address.to_string(),
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
    pub denoms: Vec<String>,
    pub destinations: Vec<Destination>,
}

impl Operation for Distribution {
    fn init(self, deps: Deps, _env: &Env) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        if self.denoms.is_empty() {
            return Err(StdError::generic_err("Denoms cannot be empty"));
        }

        let has_native_denoms = self.denoms.iter().any(|d| !d.contains('-'));
        let mut total_shares = Uint128::zero();

        for destination in self.destinations.iter() {
            if destination.shares.is_zero() {
                return Err(StdError::generic_err("Destination shares cannot be zero"));
            }

            match &destination.recipient {
                Recipient::Bank { address, .. }
                | Recipient::Wasm { address, .. }
                | Recipient::Strategy {
                    contract_address: address,
                } => {
                    deps.api.addr_validate(address.as_ref()).map_err(|_| {
                        StdError::generic_err(format!("Invalid destination address: {address}"))
                    })?;
                }
                Recipient::Deposit { memo } => {
                    if has_native_denoms {
                        return Err(StdError::generic_err(format!(
                            "Only secured assets can be deposited with memo {memo}"
                        )));
                    }
                }
            }

            total_shares += destination.shares;
        }

        if total_shares < Uint128::new(10_000) {
            return Err(StdError::generic_err(
                "Total shares must be at least 10,000",
            ));
        }

        Ok((Action::Distribute(self), vec![], vec![]))
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        let total_shares = self
            .destinations
            .clone()
            .into_iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        let mut balances = Coins::default();

        for denom in &self.denoms {
            balances.add(deps.querier.query_balance(&env.contract.address, denom)?)?;
        }

        if balances.is_empty() {
            return Ok((
                Action::Distribute(self),
                vec![],
                vec![DistributionEvent::DistributionSkipped {
                    reason: "No balances available for distribution".to_string(),
                }
                .into()],
            ));
        }

        let mut messages: Vec<StrategyMsg> = vec![];

        for destination in self.destinations.clone() {
            let amounts = balances
                .iter()
                .map(|coin| {
                    Coin::new(
                        coin.amount
                            .mul_floor(Decimal::from_ratio(destination.shares, total_shares)),
                        coin.denom.clone(),
                    )
                })
                .collect::<Vec<_>>();

            let message = match destination.recipient.clone() {
                Recipient::Bank { address, .. } => CosmosMsg::Bank(BankMsg::Send {
                    to_address: address.into(),
                    amount: amounts.clone(),
                }),
                Recipient::Strategy { contract_address } => CosmosMsg::Bank(BankMsg::Send {
                    to_address: contract_address.into(),
                    amount: amounts.clone(),
                }),
                Recipient::Wasm { address, msg, .. } => CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: address.into(),
                    msg,
                    funds: amounts.clone(),
                }),
                Recipient::Deposit { memo } => MsgDeposit {
                    memo,
                    coins: amounts.clone(),
                    signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
                }
                .into_cosmos_msg()?,
            };

            let distribute_message = StrategyMsg::with_payload(
                message,
                StrategyMsgPayload {
                    statistics: Statistics {
                        distributed: vec![(destination.recipient.clone(), amounts.clone())],
                        ..Statistics::default()
                    },
                    events: vec![DistributionEvent::Distribute {
                        recipient: destination.recipient.key(),
                        amount: amounts,
                    }
                    .into()],
                    ..StrategyMsgPayload::default()
                },
            );

            messages.push(distribute_message);
        }

        Ok((Action::Distribute(self), messages, vec![]))
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(self.denoms.iter().cloned().collect())
    }

    fn balances(&self, _deps: Deps, _env: &Env, _denoms: &HashSet<String>) -> StdResult<Coins> {
        Ok(Coins::default())
    }

    fn withdraw(
        self,
        _deps: Deps,
        _env: &Env,
        _desired: &HashSet<String>,
    ) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        Ok((Action::Distribute(self), vec![], vec![]))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        Ok((Action::Distribute(self), vec![], vec![]))
    }
}
