use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, BankMsg, Binary, Coin, Coins, CosmosMsg, Decimal, Deps, Env, Event,
    StdError, StdResult, SubMsg, Uint128, WasmMsg,
};

use crate::actions::action::Action;
use crate::actions::operation::Operation;
use crate::constants::UPDATE_STATS_REPLY_ID;
use crate::statistics::Statistics;
use crate::thorchain::MsgDeposit;

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
    fn init(self, deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
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

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let mut messages: Vec<SubMsg> = vec![];
        let events: Vec<Event> = vec![];

        let total_shares = self
            .destinations
            .clone()
            .into_iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        for denom in self.denoms.clone() {
            let balance = deps.querier.query_balance(&env.contract.address, &denom)?;

            if balance.amount.is_zero() {
                continue;
            }

            for destination in self.destinations.clone() {
                let amount = vec![Coin::new(
                    balance
                        .amount
                        .mul_floor(Decimal::from_ratio(destination.shares, total_shares)),
                    balance.denom.clone(),
                )];

                let message = match destination.recipient.clone() {
                    Recipient::Bank { address, .. } => CosmosMsg::Bank(BankMsg::Send {
                        to_address: address.into(),
                        amount: amount.clone(),
                    }),
                    Recipient::Strategy { contract_address } => CosmosMsg::Bank(BankMsg::Send {
                        to_address: contract_address.into(),
                        amount: amount.clone(),
                    }),
                    Recipient::Wasm { address, msg, .. } => CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: address.into(),
                        msg,
                        funds: amount.clone(),
                    }),
                    Recipient::Deposit { memo } => MsgDeposit {
                        memo,
                        coins: amount.clone(),
                        signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
                    }
                    .into_cosmos_msg()?,
                };

                messages.push(
                    SubMsg::reply_always(message, UPDATE_STATS_REPLY_ID).with_payload(
                        to_json_binary(&Statistics {
                            distributed: vec![(destination.recipient.clone(), amount)],
                            ..Statistics::default()
                        })?,
                    ),
                );
            }
        }

        Ok((Action::Distribute(self), messages, events))
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
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::Distribute(self), vec![], vec![]))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::Distribute(self), vec![], vec![]))
    }
}
