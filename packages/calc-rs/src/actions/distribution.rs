use std::collections::HashSet;
use std::vec;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, BankMsg, Binary, Coin, Coins, CosmosMsg, Decimal, Deps, Env, StdError, StdResult,
    Uint128, WasmMsg,
};

use crate::manager::Affiliate;
use crate::operation::Operation;
use crate::thorchain::{is_secured_asset, MsgDeposit};

const MINIMUM_TOTAL_SHARES: Uint128 = Uint128::new(10_000);

#[cw_serde]
pub enum Recipient {
    Bank { address: Addr },
    Contract { address: Addr, msg: Binary },
    Deposit { memo: String },
}

impl Recipient {
    pub fn key(&self) -> &str {
        match self {
            Recipient::Bank { address } | Recipient::Contract { address, .. } => address.as_str(),
            Recipient::Deposit { memo } => memo.as_str(),
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

impl Distribution {
    pub fn with_affiliates(self, affiliates: &[Affiliate]) -> StdResult<Self> {
        let total_fee_applied_shares = self
            .destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        Ok(Distribution {
            denoms: self.denoms,
            destinations: [
                self.destinations,
                affiliates
                    .iter()
                    .map(|affiliate| Destination {
                        recipient: Recipient::Bank {
                            address: affiliate.address.clone(),
                        },
                        shares: total_fee_applied_shares.mul_ceil(Decimal::bps(affiliate.bps)),
                        label: Some(affiliate.label.clone()),
                    })
                    .collect(),
            ]
            .concat(),
        })
    }

    pub fn execute_unsafe(
        self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(Vec<CosmosMsg>, Distribution)> {
        let mut balances = Coins::default();

        for denom in &self.denoms {
            balances.add(
                deps.querier
                    .query_balance(env.contract.address.as_ref(), denom)?,
            )?;
        }

        if balances.is_empty() {
            return Ok((vec![], self));
        }

        let mut messages = vec![];

        let total_shares = self
            .destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        for destination in &self.destinations {
            let share_ratio = Decimal::from_ratio(destination.shares, total_shares);

            let denom_shares = balances
                .iter()
                .flat_map(|coin| {
                    let amount = coin.amount.mul_floor(share_ratio);
                    if amount.is_zero() {
                        None
                    } else {
                        Some(Coin::new(amount, coin.denom.clone()))
                    }
                })
                .collect::<Vec<_>>();

            if denom_shares.is_empty() {
                continue;
            }

            let distribute_message = match destination.recipient.clone() {
                Recipient::Bank { address, .. } => CosmosMsg::Bank(BankMsg::Send {
                    to_address: address.into(),
                    amount: denom_shares,
                }),
                Recipient::Contract { address, msg, .. } => CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: address.into(),
                    msg,
                    funds: denom_shares,
                }),
                Recipient::Deposit { memo } => MsgDeposit {
                    memo,
                    coins: denom_shares,
                    signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
                }
                .into_cosmos_msg()?,
            };

            messages.push(distribute_message);
        }

        Ok((messages, self))
    }
}

impl Operation<Distribution> for Distribution {
    fn init(self, deps: Deps, _env: &Env, affiliates: &[Affiliate]) -> StdResult<Distribution> {
        if self.denoms.is_empty() {
            return Err(StdError::generic_err("Denoms cannot be empty"));
        }

        if self.destinations.is_empty() {
            return Err(StdError::generic_err("Destinations cannot be empty"));
        }

        let denoms = HashSet::<String>::from_iter(self.denoms.clone())
            .into_iter()
            .collect::<Vec<_>>();

        let has_native_denoms = denoms.iter().any(|d| !is_secured_asset(d));
        let mut total_shares = Uint128::zero();

        for destination in self.destinations.iter() {
            if destination.shares.is_zero() {
                return Err(StdError::generic_err("Destination shares cannot be zero"));
            }

            match &destination.recipient {
                Recipient::Bank { address, .. } | Recipient::Contract { address, .. } => {
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

        if total_shares < MINIMUM_TOTAL_SHARES {
            return Err(StdError::generic_err(format!(
                "Total shares must be at least {MINIMUM_TOTAL_SHARES}"
            )));
        }

        Ok(Distribution {
            denoms,
            ..Distribution::with_affiliates(self, affiliates)?
        })
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<CosmosMsg>, Distribution) {
        match self.clone().execute_unsafe(deps, env) {
            Ok((messages, action)) => (messages, action),
            Err(_) => (vec![], self),
        }
    }
}
