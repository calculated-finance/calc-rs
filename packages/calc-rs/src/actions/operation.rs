use std::collections::HashSet;

use cosmwasm_std::{Coins, Deps, Env, Event, StdResult, SubMsg};

use crate::{actions::action::Action, conditions::Condition};

pub trait Operation {
    fn init(self, deps: Deps, env: &Env) -> StdResult<Action>;
    fn condition(&self, env: &Env) -> Option<Condition>;
    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)>;
    fn update(
        self,
        deps: Deps,
        env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)>;
    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>>;
    fn balances(&self, deps: Deps, env: &Env, denoms: &[String]) -> StdResult<Coins>;
    fn withdraw(&self, deps: Deps, env: &Env, desired: &Coins) -> StdResult<(Vec<SubMsg>, Coins)>;
    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)>;
}
