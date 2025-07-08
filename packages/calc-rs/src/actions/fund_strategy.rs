use std::collections::HashSet;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{BankMsg, Coins, Deps, Env, Event, StdError, StdResult};

use crate::{
    actions::{action::Action, operation::Operation},
    statistics::Statistics,
    strategy::{StrategyMsg, StrategyMsgPayload},
};

enum FundStrategyEvent {
    Fund {
        contract_address: String,
        denoms: Vec<String>,
        total_funds: Coins,
    },
}

impl Into<Event> for FundStrategyEvent {
    fn into(self) -> Event {
        match self {
            FundStrategyEvent::Fund {
                contract_address,
                denoms,
                total_funds,
            } => Event::new("fund_strategy")
                .add_attribute("contract_address", contract_address)
                .add_attribute("denoms", denoms.join(", "))
                .add_attribute("total_funds", total_funds.to_string()),
        }
    }
}

#[cw_serde]
pub struct FundStrategy {
    contract_address: String,
    denoms: HashSet<String>,
}

impl Operation for FundStrategy {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        if self.denoms.is_empty() {
            return Err(StdError::generic_err(
                "Fund strategy denoms cannot be empty",
            ));
        }

        let source_contract_info = deps
            .querier
            .query_wasm_contract_info(env.contract.address.clone())?;

        let funded_contract_code_id = deps
            .querier
            .query_wasm_contract_info(self.contract_address.clone())?
            .code_id;

        if source_contract_info.code_id != funded_contract_code_id {
            return Err(StdError::generic_err(
                "Funded strategy contract must be a CALC strategy contract",
            ));
        }

        Ok((Action::FundStrategy(self), vec![], vec![]))
    }

    fn execute(self, deps: Deps, _env: &Env) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        let mut funds = Coins::default();

        for denom in &self.denoms {
            funds.add(deps.querier.query_balance(&self.contract_address, denom)?)?;
        }

        let bank_msg = StrategyMsg::with_payload(
            BankMsg::Send {
                to_address: self.contract_address.clone(),
                amount: funds.to_vec(),
            }
            .into(),
            StrategyMsgPayload {
                statistics: Statistics {
                    swapped: funds.to_vec(),
                    ..Statistics::default()
                },
                events: vec![FundStrategyEvent::Fund {
                    contract_address: self.contract_address.clone(),
                    denoms: self.denoms.iter().cloned().collect(),
                    total_funds: funds.clone(),
                }
                .into()],
                ..StrategyMsgPayload::default()
            },
        );

        Ok((Action::FundStrategy(self), vec![bank_msg], vec![]))
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::new())
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
        Ok((Action::FundStrategy(self), vec![], vec![]))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        Ok((Action::FundStrategy(self), vec![], vec![]))
    }
}
