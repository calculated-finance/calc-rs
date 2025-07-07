use std::collections::HashSet;

use cosmwasm_std::{Coins, Deps, Env, Event, StdResult, SubMsg};

use crate::{
    actions::{action::Action, operation::Operation},
    conditions::{Conditions, Satisfiable},
};

impl Operation for (Conditions, Box<Action>) {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let (action, messages, events) = self.1.init(_deps, _env)?;

        Ok((
            Action::Conditional((self.0, Box::new(action))),
            messages,
            events,
        ))
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

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        self.1.escrowed(deps, env)
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        self.1.balances(deps, env, denoms)
    }

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        self.1.withdraw(deps, env, desired)
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::Conditional(self), vec![], vec![]))
    }
}
