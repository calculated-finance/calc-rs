use std::collections::HashSet;

use cosmwasm_std::{Coins, Deps, Env, Event, StdResult};

use crate::{manager::Affiliate, strategy::StrategyMsg};

pub trait Operation<T>: Send + Sync + Clone
where
    T: Send + Sync + Clone,
{
    fn init(self, deps: Deps, env: &Env, affiliates: &[Affiliate]) -> StdResult<T>;
    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, T);
    fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>>;
    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>>;
    fn commit(self, deps: Deps, env: &Env) -> StdResult<T>;
    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins>;
    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, T)>;
    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, T)>;
}
