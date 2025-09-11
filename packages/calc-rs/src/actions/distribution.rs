use std::collections::HashSet;
use std::vec;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, BankMsg, Binary, Coin, Coins, CosmosMsg, Decimal, Deps, Env, StdError, StdResult,
    Uint128, WasmMsg,
};

use crate::manager::Affiliate;
use crate::operation::Operation;
use crate::thorchain::MsgDeposit;

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
    pub distributions: Option<Vec<Coin>>,
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
                        distributions: None,
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

        let mut remaining = balances.clone();
        let mut messages = vec![];

        let total_shares = self
            .destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        let destinations_count = self.destinations.len();
        let final_destination = destinations_count.saturating_sub(1);

        let mut destinations = Vec::<Destination>::with_capacity(destinations_count);

        for (i, mut destination) in self.destinations.into_iter().enumerate() {
            let share_ratio = Decimal::from_ratio(destination.shares, total_shares);

            let denom_shares = balances
                .iter()
                .flat_map(|coin| {
                    let amount = if i == final_destination {
                        remaining.amount_of(&coin.denom)
                    } else {
                        coin.amount.mul_floor(share_ratio)
                    };

                    if amount.is_zero() {
                        Err(StdError::generic_err(format!(
                            "No remaining balance to distribute for denom {}",
                            coin.denom
                        )))
                    } else {
                        let distribution = Coin::new(amount, coin.denom.clone());
                        remaining.sub(distribution.clone())?;

                        Ok(distribution)
                    }
                })
                .collect::<Vec<_>>();

            if denom_shares.is_empty() {
                continue;
            }

            let distribute_message = match destination.recipient.clone() {
                Recipient::Bank { address, .. } => CosmosMsg::Bank(BankMsg::Send {
                    to_address: address.into(),
                    amount: denom_shares.clone(),
                }),
                Recipient::Contract { address, msg, .. } => CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: address.into(),
                    msg,
                    funds: denom_shares.clone(),
                }),
                Recipient::Deposit { memo } => MsgDeposit {
                    memo,
                    coins: denom_shares.clone(),
                    signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
                }
                .into_cosmos_msg()?,
            };

            messages.push(distribute_message);

            let mut distributions =
                Coins::try_from(destination.distributions.take().unwrap_or_default())?;

            for distribution in denom_shares {
                distributions.add(distribution)?;
            }

            destinations.push(Destination {
                distributions: Some(distributions.into_vec()),
                ..destination
            });
        }

        Ok((
            messages,
            Distribution {
                denoms: self.denoms,
                destinations,
            },
        ))
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

        let unique_denoms = HashSet::<String>::from_iter(self.denoms.clone())
            .into_iter()
            .collect::<Vec<_>>();

        if unique_denoms.len() != self.denoms.len() {
            return Err(StdError::generic_err("Denoms cannot contain duplicates"));
        }

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
                Recipient::Deposit { .. } => {}
            }

            total_shares += destination.shares;
        }

        if total_shares < MINIMUM_TOTAL_SHARES {
            return Err(StdError::generic_err(format!(
                "Total shares must be at least {MINIMUM_TOTAL_SHARES}"
            )));
        }

        self.with_affiliates(affiliates)
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Distribution)> {
        self.execute_unsafe(deps, env)
    }
}
