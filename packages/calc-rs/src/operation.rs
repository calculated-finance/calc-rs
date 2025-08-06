use std::collections::HashSet;

use cosmwasm_std::{Coins, CosmosMsg, Deps, Env, StdResult};

use crate::manager::Affiliate;

pub trait Operation<T> {
    fn init(self, deps: Deps, env: &Env, affiliates: &[Affiliate]) -> StdResult<T>;
    fn execute(self, deps: Deps, env: &Env) -> (Vec<CosmosMsg>, T);
    fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>>;
}

pub trait StatefulOperation<T> {
    fn commit(self, deps: Deps, env: &Env) -> StdResult<T>;
    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Vec<CosmosMsg>, T)>;
    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, T)>;
    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins>;
}
