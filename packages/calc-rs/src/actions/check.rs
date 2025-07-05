use std::collections::HashSet;

use cosmwasm_std::{Coins, Deps, Env, Event, StdError, StdResult, SubMsg};

use crate::{
    actions::{action::Action, operation::Operation},
    conditions::Condition,
};

impl Operation for Condition {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<Action> {
        Ok(Action::Check(self))
    }

    fn condition(&self, _env: &Env) -> Option<Condition> {
        Some(self.clone())
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        self.check(deps, env)?;
        Ok((Action::Check(self), vec![], vec![]))
    }

    fn update(
        self,
        _deps: Deps,
        _env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        if let Action::Check(update) = update {
            Ok((Action::Check(update), vec![], vec![]))
        } else {
            Err(StdError::generic_err("Invalid action type for update"))
        }
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::new())
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
        Ok((Action::Check(self), vec![], vec![]))
    }
}
