use std::{collections::HashSet, u8, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdResult, SubMsg};

use crate::{
    actions::{
        behaviour::Behaviour, crank::Schedule, operation::Operation, order::Order,
        recipients::Recipients, swap::Swap,
    },
    conditions::Condition,
};

#[cw_serde]
pub enum Action {
    Check(Condition),
    Crank(Schedule),
    Perform(Swap),
    Set(Order),
    DistributeTo(Recipients),
    Exhibit(Behaviour),
}

impl Action {
    pub fn size(&self) -> usize {
        match self {
            Action::Exhibit(action) => action.size(),
            _ => 1,
        }
    }
}

impl Operation for Action {
    fn init(self, deps: Deps, env: &Env) -> StdResult<Action> {
        match self {
            Action::Check(condition) => condition.init(deps, env),
            Action::Crank(action) => action.init(deps, env),
            Action::Perform(action) => action.init(deps, env),
            Action::Set(action) => action.init(deps, env),
            Action::DistributeTo(action) => action.init(deps, env),
            Action::Exhibit(action) => action.init(deps, env),
        }
    }

    fn condition(&self, env: &Env) -> Option<Condition> {
        match self {
            Action::Check(condition) => condition.condition(env),
            Action::Crank(action) => action.condition(env),
            Action::Perform(action) => action.condition(env),
            Action::Set(action) => action.condition(env),
            Action::DistributeTo(action) => action.condition(env),
            Action::Exhibit(action) => action.condition(env),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            Action::Check(condition) => condition.execute(deps, env),
            Action::Crank(action) => action.execute(deps, env),
            Action::Perform(action) => action.execute(deps, env),
            Action::Set(action) => action.execute(deps, env),
            Action::DistributeTo(action) => action.execute(deps, env),
            Action::Exhibit(action) => action.execute(deps, env),
        }
    }

    fn update(
        self,
        deps: Deps,
        env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            Action::Check(condition) => condition.update(deps, env, update),
            Action::Crank(action) => action.update(deps, env, update),
            Action::Perform(action) => action.update(deps, env, update),
            Action::Set(action) => action.update(deps, env, update),
            Action::DistributeTo(action) => action.update(deps, env, update),
            Action::Exhibit(action) => action.update(deps, env, update),
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Action::Check(condition) => condition.escrowed(deps, env),
            Action::Crank(action) => action.escrowed(deps, env),
            Action::Perform(action) => action.escrowed(deps, env),
            Action::Set(action) => action.escrowed(deps, env),
            Action::DistributeTo(action) => action.escrowed(deps, env),
            Action::Exhibit(action) => action.escrowed(deps, env),
        }
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &[String]) -> StdResult<Coins> {
        match self {
            Action::Check(condition) => condition.balances(deps, env, denoms),
            Action::Crank(action) => action.balances(deps, env, denoms),
            Action::Perform(action) => action.balances(deps, env, denoms),
            Action::Set(action) => action.balances(deps, env, denoms),
            Action::DistributeTo(action) => action.balances(deps, env, denoms),
            Action::Exhibit(action) => action.balances(deps, env, denoms),
        }
    }

    fn withdraw(&self, deps: Deps, env: &Env, desired: &Coins) -> StdResult<(Vec<SubMsg>, Coins)> {
        match self {
            Action::Check(condition) => condition.withdraw(deps, env, desired),
            Action::Crank(action) => action.withdraw(deps, env, desired),
            Action::Perform(action) => action.withdraw(deps, env, desired),
            Action::Set(action) => action.withdraw(deps, env, desired),
            Action::DistributeTo(action) => action.withdraw(deps, env, desired),
            Action::Exhibit(action) => action.withdraw(deps, env, desired),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            Action::Check(condition) => condition.cancel(deps, env),
            Action::Crank(action) => action.cancel(deps, env),
            Action::Perform(action) => action.cancel(deps, env),
            Action::Set(action) => action.cancel(deps, env),
            Action::DistributeTo(action) => action.cancel(deps, env),
            Action::Exhibit(action) => action.cancel(deps, env),
        }
    }
}
