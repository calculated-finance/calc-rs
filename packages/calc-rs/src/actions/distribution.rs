use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, BankMsg, Binary, Coin, Coins, CosmosMsg, Decimal, Deps, Env, Event,
    StdError, StdResult, SubMsg, Uint128, WasmMsg,
};

use crate::actions::action::Action;
use crate::actions::operation::Operation;
use crate::statistics::Statistics;
use crate::thorchain::MsgDeposit;

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
    pub denoms: Vec<String>,
    pub mutable_destinations: Vec<Destination>,
    pub immutable_destinations: Vec<Destination>,
}

impl Operation for Distribution {
    fn init(self, deps: Deps, _env: &Env) -> StdResult<Action> {
        if self.denoms.is_empty() {
            return Err(StdError::generic_err("Denoms cannot be empty"));
        }

        let destinations = self
            .mutable_destinations
            .iter()
            .chain(self.immutable_destinations.iter())
            .collect::<Vec<_>>();

        let has_native_denoms = self.denoms.iter().any(|d| !d.contains('-'));
        let mut total_shares = Uint128::zero();

        for destination in destinations {
            if destination.shares.is_zero() {
                return Err(StdError::generic_err("Destination shares cannot be zero"));
            }

            match &destination.recipient {
                Recipient::Bank { address, .. } | Recipient::Wasm { address, .. } => {
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

        Ok(Action::Distribute(self))
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let mut messages: Vec<SubMsg> = vec![];
        let events: Vec<Event> = vec![];

        let destinations = self
            .mutable_destinations
            .iter()
            .chain(self.immutable_destinations.iter());

        let total_shares = destinations
            .clone()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        for denom in self.denoms.clone() {
            let balance = deps.querier.query_balance(&env.contract.address, &denom)?;

            if balance.amount.is_zero() {
                continue;
            }

            for destination in destinations.clone() {
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
                    SubMsg::reply_always(message, 0).with_payload(to_json_binary(&Statistics {
                        distributed: vec![(destination.recipient.clone(), amount)],
                        ..Statistics::default()
                    })?),
                );
            }
        }

        Ok((Action::Distribute(self), messages, events))
    }

    fn update(
        self,
        _deps: Deps,
        _env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        if let Action::Distribute(update) = update {
            let existing_total_shares = self
                .mutable_destinations
                .iter()
                .chain(self.immutable_destinations.iter())
                .fold(Uint128::zero(), |acc, d| acc + d.shares);

            let new_total_shares = update
                .mutable_destinations
                .iter()
                .chain(update.immutable_destinations.iter())
                .fold(Uint128::zero(), |acc, d| acc + d.shares);

            if new_total_shares != existing_total_shares {
                return Err(StdError::generic_err(
                    "Cannot update distribute action with different total shares",
                ));
            }

            for denom in self.denoms.iter() {
                if !update.denoms.contains(denom) {
                    return Err(StdError::generic_err(format!(
                        "Cannot remove denom {denom} from distribute action"
                    )));
                }
            }

            Ok((
                Action::Distribute(Distribution {
                    immutable_destinations: self.immutable_destinations,
                    ..update
                }),
                vec![],
                vec![],
            ))
        } else {
            Err(StdError::generic_err(
                "Cannot update distribute action with non-distribute action",
            ))
        }
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(self.denoms.iter().cloned().collect())
    }

    fn balances(&self, _deps: Deps, _env: &Env, _denoms: &[String]) -> StdResult<Coins> {
        Ok(Coins::default())
    }

    fn withdraw(
        &self,
        _deps: Deps,
        _env: &Env,
        _desired: &Coins,
    ) -> StdResult<(Vec<SubMsg>, Coins)> {
        Ok((vec![], Coins::default()))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::Distribute(self), vec![], vec![]))
    }
}
