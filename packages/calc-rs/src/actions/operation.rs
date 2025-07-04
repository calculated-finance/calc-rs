use std::collections::HashSet;

use cosmwasm_std::{Coins, Deps, Env, StdResult, SubMsg};

use crate::{conditions::Condition, events::DomainEvent};

pub trait Operation<T: Sized> {
    fn init(self, deps: Deps, env: &Env) -> StdResult<T>;
    fn condition(self, env: &Env) -> Option<Condition>;
    fn execute(self, deps: Deps, env: &Env) -> StdResult<(T, Vec<SubMsg>, Vec<DomainEvent>)>;
    fn update(
        self,
        deps: Deps,
        env: &Env,
        update: T,
    ) -> StdResult<(T, Vec<SubMsg>, Vec<DomainEvent>)>;
    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>>;
    fn balances(&self, deps: Deps, env: &Env, denoms: &[String]) -> StdResult<Coins>;
    fn withdraw(self, deps: Deps, env: &Env, desired: &Coins)
        -> StdResult<(T, Vec<SubMsg>, Coins)>;
    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(T, Vec<SubMsg>, Vec<DomainEvent>)>;
}
