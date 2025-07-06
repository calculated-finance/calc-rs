use std::collections::HashSet;

use cosmwasm_std::{Coins, Deps, Env, Event, StdError, StdResult, SubMsg};

use crate::{
    actions::{action::Action, operation::Operation},
    conditions::{Conditions, Satisfiable},
};

impl Operation for (Conditions, Box<Action>) {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<Action> {
        Ok(Action::Conditional(self))
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        if self.0.is_satisfied(deps, env) {
            let (action, msgs, events) = self.1.execute(deps, env)?;
            Ok((
                Action::Conditional((self.0, Box::new(action))),
                msgs,
                events,
            ))
        } else {
            Ok((Action::Conditional(self), vec![], vec![]))
        }
    }

    fn update(
        self,
        _deps: Deps,
        _env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        if let Action::Conditional(update) = update {
            Ok((Action::Conditional(update), vec![], vec![]))
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
        Ok((Action::Conditional(self), vec![], vec![]))
    }
}
