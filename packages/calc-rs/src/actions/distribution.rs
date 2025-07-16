use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, BankMsg, Binary, Coin, Coins, CosmosMsg, Decimal, Deps, Env, Event, StdError, StdResult,
    Uint128, WasmMsg,
};

use crate::actions::action::Action;
use crate::actions::operation::StatelessOperation;
use crate::constants::MAX_TOTAL_AFFILIATE_BPS;
use crate::manager::Affiliate;
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

impl From<DistributionEvent> for Event {
    fn from(val: DistributionEvent) -> Self {
        match val {
            DistributionEvent::DistributionSkipped { reason } => {
                Event::new("distribution_skipped").add_attribute("reason", reason)
            }
            DistributionEvent::Distribute { recipient, amount } => Event::new("distribute")
                .add_attribute("recipient", recipient)
                .add_attribute("amount", format!("{amount:?}")),
        }
    }
}

#[cw_serde]
pub enum Recipient {
    Bank { address: Addr },
    Contract { address: Addr, msg: Binary },
    Deposit { memo: String },
    Strategy { contract_address: Addr },
}

impl Recipient {
    pub fn key(&self) -> String {
        match self {
            Recipient::Bank { address }
            | Recipient::Contract { address, .. }
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

impl Distribution {
    pub fn with_affiliates(self, affiliates: &[Affiliate]) -> StdResult<Self> {
        let total_affiliate_bps = affiliates
            .iter()
            .fold(0, |acc, affiliate| acc + affiliate.bps);

        if total_affiliate_bps > MAX_TOTAL_AFFILIATE_BPS {
            return Err(StdError::generic_err(format!(
                "Total affiliate bps cannot exceed {MAX_TOTAL_AFFILIATE_BPS}, got {total_affiliate_bps}"
            )));
        }

        let total_fee_applied_shares = self
            .destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        Ok(Distribution {
            denoms: self.denoms.clone(),
            destinations: [
                self.destinations.clone(),
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
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        let mut balances = Coins::default();

        for denom in &self.denoms {
            balances.add(deps.querier.query_balance(&env.contract.address, denom)?)?;
        }

        if balances.is_empty() {
            return Ok((
                vec![],
                vec![DistributionEvent::DistributionSkipped {
                    reason: "No balances available for distribution".to_string(),
                }
                .into()],
                Action::Distribute(self),
            ));
        }

        let mut messages = vec![];

        let total_shares = self
            .destinations
            .clone()
            .into_iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        for destination in self.destinations.clone() {
            let denom_shares = balances
                .iter()
                .flat_map(|coin| {
                    let shares_amount = coin
                        .amount
                        .mul_floor(Decimal::from_ratio(destination.shares, total_shares));

                    if shares_amount.is_zero() {
                        return None;
                    }

                    Some(Coin::new(shares_amount, coin.denom.clone()))
                })
                .collect::<Vec<_>>();

            let message = match destination.recipient.clone() {
                Recipient::Bank { address, .. } => CosmosMsg::Bank(BankMsg::Send {
                    to_address: address.into(),
                    amount: denom_shares.clone(),
                }),
                Recipient::Strategy { contract_address } => CosmosMsg::Bank(BankMsg::Send {
                    to_address: contract_address.into(),
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

            let distribute_message = StrategyMsg::with_payload(
                message,
                StrategyMsgPayload {
                    statistics: Statistics {
                        credited: vec![(destination.recipient.clone(), denom_shares.clone())],
                        ..Statistics::default()
                    },
                    events: vec![DistributionEvent::Distribute {
                        recipient: destination.recipient.key(),
                        amount: denom_shares,
                    }
                    .into()],
                },
            );

            messages.push(distribute_message);
        }

        Ok((messages, vec![], Action::Distribute(self)))
    }
}

impl StatelessOperation for Distribution {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if self.denoms.is_empty() {
            return Err(StdError::generic_err("Denoms cannot be empty"));
        }

        if self.destinations.is_empty() {
            return Err(StdError::generic_err("Destinations cannot be empty"));
        }

        let has_native_denoms = self.denoms.iter().any(|d| !d.contains('-'));
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
                Recipient::Strategy { contract_address } => {
                    let source_contract_info = deps
                        .querier
                        .query_wasm_contract_info(env.contract.address.clone())?;

                    let funded_contract_code_id = deps
                        .querier
                        .query_wasm_contract_info(contract_address.clone())?
                        .code_id;

                    if source_contract_info.code_id != funded_contract_code_id {
                        return Err(StdError::generic_err(
                            "Funded strategy contract must be a CALC strategy contract",
                        ));
                    }
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

        Ok((vec![], vec![], Action::Distribute(self)))
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        match self.clone().execute_unsafe(deps, env) {
            Ok((action, messages, events)) => (action, messages, events),
            Err(err) => (
                vec![],
                vec![DistributionEvent::DistributionSkipped {
                    reason: err.to_string(),
                }
                .into()],
                Action::Distribute(self),
            ),
        }
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(self.denoms.iter().cloned().collect())
    }
}
