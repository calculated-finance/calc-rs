use cosmwasm_std::{Coins, CosmosMsg, Deps, Env, StdResult};

use crate::manager::Affiliate;

pub trait Operation<T> {
    fn init(self, deps: Deps, env: &Env, affiliates: &[Affiliate]) -> StdResult<T>;
    fn execute(self, deps: Deps, env: &Env) -> (Vec<CosmosMsg>, T);
}

pub trait StatefulOperation<T> {
    fn commit(self, deps: Deps, env: &Env) -> StdResult<T>;
    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, T)>;
    fn balances(&self, deps: Deps, env: &Env) -> StdResult<Coins>;
}
