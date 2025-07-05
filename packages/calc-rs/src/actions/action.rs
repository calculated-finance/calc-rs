use std::{collections::HashSet, u8, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdResult, SubMsg};

use crate::{
    actions::{
        behaviour::Behaviour, distribution::Distribution, fin_swap::FinSwap,
        limit_order::LimitOrder, operation::Operation, schedule::Schedule, swap::Swap,
        thor_swap::ThorSwap,
    },
    conditions::Condition,
};

#[cw_serde]
pub enum Action {
    CheckCondition(Condition),
    ExecuteStrategy(Schedule),
    FinSwap(FinSwap),
    ThorSwap(ThorSwap),
    Swap(Swap),
    SetLimitOrder(LimitOrder),
    Distribute(Distribution),
    Compose(Behaviour),
}

impl Action {
    pub fn size(&self) -> usize {
        match self {
            Action::Compose(action) => action.size(),
            _ => 1,
        }
    }
}

impl Operation for Action {
    fn init(self, deps: Deps, env: &Env) -> StdResult<Action> {
        match self {
            Action::CheckCondition(condition) => condition.init(deps, env),
            Action::ExecuteStrategy(action) => action.init(deps, env),
            Action::FinSwap(action) => action.init(deps, env),
            Action::ThorSwap(action) => action.init(deps, env),
            Action::Swap(action) => action.init(deps, env),
            Action::SetLimitOrder(action) => action.init(deps, env),
            Action::Distribute(action) => action.init(deps, env),
            Action::Compose(action) => action.init(deps, env),
        }
    }

    fn condition(&self, env: &Env) -> Option<Condition> {
        match self {
            Action::CheckCondition(condition) => condition.condition(env),
            Action::ExecuteStrategy(action) => action.condition(env),
            Action::FinSwap(action) => action.condition(env),
            Action::ThorSwap(action) => action.condition(env),
            Action::Swap(action) => action.condition(env),
            Action::SetLimitOrder(action) => action.condition(env),
            Action::Distribute(action) => action.condition(env),
            Action::Compose(action) => action.condition(env),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            Action::CheckCondition(condition) => condition.execute(deps, env),
            Action::ExecuteStrategy(action) => action.execute(deps, env),
            Action::FinSwap(action) => action.execute(deps, env),
            Action::ThorSwap(action) => action.execute(deps, env),
            Action::Swap(action) => action.execute(deps, env),
            Action::SetLimitOrder(action) => action.execute(deps, env),
            Action::Distribute(action) => action.execute(deps, env),
            Action::Compose(action) => action.execute(deps, env),
        }
    }

    fn update(
        self,
        deps: Deps,
        env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            Action::CheckCondition(condition) => condition.update(deps, env, update),
            Action::ExecuteStrategy(action) => action.update(deps, env, update),
            Action::FinSwap(action) => action.update(deps, env, update),
            Action::ThorSwap(action) => action.update(deps, env, update),
            Action::Swap(action) => action.update(deps, env, update),
            Action::SetLimitOrder(action) => action.update(deps, env, update),
            Action::Distribute(action) => action.update(deps, env, update),
            Action::Compose(action) => action.update(deps, env, update),
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Action::CheckCondition(condition) => condition.escrowed(deps, env),
            Action::ExecuteStrategy(action) => action.escrowed(deps, env),
            Action::FinSwap(action) => action.escrowed(deps, env),
            Action::ThorSwap(action) => action.escrowed(deps, env),
            Action::Swap(action) => action.escrowed(deps, env),
            Action::SetLimitOrder(action) => action.escrowed(deps, env),
            Action::Distribute(action) => action.escrowed(deps, env),
            Action::Compose(action) => action.escrowed(deps, env),
        }
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &[String]) -> StdResult<Coins> {
        match self {
            Action::CheckCondition(condition) => condition.balances(deps, env, denoms),
            Action::ExecuteStrategy(action) => action.balances(deps, env, denoms),
            Action::FinSwap(action) => action.balances(deps, env, denoms),
            Action::ThorSwap(action) => action.balances(deps, env, denoms),
            Action::Swap(action) => action.balances(deps, env, denoms),
            Action::SetLimitOrder(action) => action.balances(deps, env, denoms),
            Action::Distribute(action) => action.balances(deps, env, denoms),
            Action::Compose(action) => action.balances(deps, env, denoms),
        }
    }

    fn withdraw(&self, deps: Deps, env: &Env, desired: &Coins) -> StdResult<(Vec<SubMsg>, Coins)> {
        match self {
            Action::CheckCondition(condition) => condition.withdraw(deps, env, desired),
            Action::ExecuteStrategy(action) => action.withdraw(deps, env, desired),
            Action::FinSwap(action) => action.withdraw(deps, env, desired),
            Action::ThorSwap(action) => action.withdraw(deps, env, desired),
            Action::Swap(action) => action.withdraw(deps, env, desired),
            Action::SetLimitOrder(action) => action.withdraw(deps, env, desired),
            Action::Distribute(action) => action.withdraw(deps, env, desired),
            Action::Compose(action) => action.withdraw(deps, env, desired),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            Action::CheckCondition(condition) => condition.cancel(deps, env),
            Action::ExecuteStrategy(action) => action.cancel(deps, env),
            Action::FinSwap(action) => action.cancel(deps, env),
            Action::ThorSwap(action) => action.cancel(deps, env),
            Action::Swap(action) => action.cancel(deps, env),
            Action::SetLimitOrder(action) => action.cancel(deps, env),
            Action::Distribute(action) => action.cancel(deps, env),
            Action::Compose(action) => action.cancel(deps, env),
        }
    }
}
