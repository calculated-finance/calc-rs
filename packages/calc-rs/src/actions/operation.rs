use std::collections::HashSet;

use cosmwasm_std::{Coins, Deps, Env, Event, StdResult, SubMsg};

use crate::actions::action::Action;

pub trait Operation {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)>;
    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)>;
    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>>;
    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins>;
    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)>;
    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)>;
}
