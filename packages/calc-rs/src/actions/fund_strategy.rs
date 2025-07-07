use std::collections::HashSet;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, BankMsg, Coins, Deps, Env, Event, StdError, StdResult, SubMsg};

use crate::{
    actions::{action::Action, operation::Operation},
    constants::UPDATE_STATS_REPLY_ID,
    statistics::Statistics,
};

#[cw_serde]
pub struct FundStrategy {
    contract_address: String,
    denoms: HashSet<String>,
}

impl Operation for FundStrategy {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
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

    fn execute(self, deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let mut funds = Coins::default();

        for denom in &self.denoms {
            funds.add(deps.querier.query_balance(&self.contract_address, denom)?)?;
        }

        let bank_msg = SubMsg::reply_always(
            BankMsg::Send {
                to_address: self.contract_address.clone(),
                amount: funds.to_vec(),
            },
            UPDATE_STATS_REPLY_ID,
        )
        .with_payload(to_json_binary(&Statistics {
            swapped: funds.to_vec(),
            ..Statistics::default()
        })?);

        Ok((
            Action::FundStrategy(self),
            vec![SubMsg::from(bank_msg)],
            vec![],
        ))
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
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::FundStrategy(self), vec![], vec![]))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::FundStrategy(self), vec![], vec![]))
    }
}
