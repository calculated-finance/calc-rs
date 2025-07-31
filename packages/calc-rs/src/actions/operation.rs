use std::collections::HashSet;

use cosmwasm_std::{Coins, Deps, Env, Event, StdResult};

use crate::{actions::action::Action, strategy::StrategyMsg};

pub trait StatelessOperation {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)>;
    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action);
    fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>>;
    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>>;
}

pub trait StatefulOperation {
    fn commit(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)>;
    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins>;
    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)>;
    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)>;
}
