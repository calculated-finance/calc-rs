use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdError, StdResult};

use crate::{
    actions::{action::Action, operation::Operation},
    condition::Condition,
    core::Threshold,
    strategy::StrategyMsg,
};

#[cw_serde]
pub struct Conditional {
    pub threshold: Threshold,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
}

impl Operation<Action> for Conditional {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if self.conditions.is_empty() {
            return Err(StdError::generic_err("No conditions provided"));
        }

        if self
            .conditions
            .iter()
            .fold(0, |acc, condition| acc + condition.size())
            > 20
        {
            return Err(StdError::generic_err(
                "Condition size exceeds maximum limit of 20",
            ));
        }

        Ok((vec![], vec![], Action::Conditional(self)))
    }

    fn execute(self, _deps: Deps, _env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        (vec![], vec![], Action::Conditional(self))
    }

    fn denoms(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::new())
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::new())
    }

    fn commit(self, _deps: Deps, _env: &Env) -> StdResult<Action> {
        Ok(Action::Conditional(self))
    }

    fn balances(&self, _deps: Deps, _env: &Env, _denoms: &HashSet<String>) -> StdResult<Coins> {
        Ok(Coins::default())
    }

    fn withdraw(
        self,
        _deps: Deps,
        _env: &Env,
        _desired: &HashSet<String>,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        Ok((vec![], vec![], Action::Conditional(self)))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        Ok((vec![], vec![], Action::Conditional(self)))
    }
}
