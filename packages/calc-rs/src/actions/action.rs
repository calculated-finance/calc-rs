use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdResult, SubMsg};

use crate::{
    actions::{
        distribution::Distribution, fin_swap::FinSwap, fund_strategy::FundStrategy,
        limit_order::LimitOrder, operation::Operation, schedule::Schedule, swap::OptimalSwap,
        thor_swap::ThorSwap,
    },
    conditions::Conditions,
};

#[cw_serde]
pub enum Action {
    FinSwap(FinSwap),
    ThorSwap(ThorSwap),
    OptimalSwap(OptimalSwap),
    SetLimitOrder(LimitOrder),
    Distribute(Distribution),
    Schedule(Schedule),
    FundStrategy(FundStrategy),
    Conditional((Conditions, Box<Action>)),
    Many(Vec<Action>),
}

impl Action {
    pub fn size(&self) -> usize {
        match self {
            Action::Schedule(action) => action.action.size(),
            Action::Conditional((_, action)) => action.size(),
            Action::Many(actions) => actions.iter().map(|a| a.size()).sum(),
            _ => 1,
        }
    }
}

impl Operation for Action {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            Action::FinSwap(action) => action.init(deps, env),
            Action::ThorSwap(action) => action.init(deps, env),
            Action::OptimalSwap(action) => action.init(deps, env),
            Action::SetLimitOrder(action) => action.init(deps, env),
            Action::Distribute(action) => action.init(deps, env),
            Action::Schedule(action) => action.init(deps, env),
            Action::FundStrategy(action) => action.init(deps, env),
            Action::Conditional(condition) => condition.init(deps, env),
            Action::Many(action) => action.init(deps, env),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            Action::FinSwap(action) => action.execute(deps, env),
            Action::ThorSwap(action) => action.execute(deps, env),
            Action::OptimalSwap(action) => action.execute(deps, env),
            Action::SetLimitOrder(action) => action.execute(deps, env),
            Action::Distribute(action) => action.execute(deps, env),
            Action::Schedule(action) => action.execute(deps, env),
            Action::FundStrategy(action) => action.execute(deps, env),
            Action::Conditional(condition) => condition.execute(deps, env),
            Action::Many(action) => action.execute(deps, env),
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Action::FinSwap(action) => action.escrowed(deps, env),
            Action::ThorSwap(action) => action.escrowed(deps, env),
            Action::OptimalSwap(action) => action.escrowed(deps, env),
            Action::SetLimitOrder(action) => action.escrowed(deps, env),
            Action::Distribute(action) => action.escrowed(deps, env),
            Action::Schedule(action) => action.escrowed(deps, env),
            Action::FundStrategy(action) => action.escrowed(deps, env),
            Action::Conditional(condition) => condition.escrowed(deps, env),
            Action::Many(action) => action.escrowed(deps, env),
        }
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        match self {
            Action::FinSwap(action) => action.balances(deps, env, denoms),
            Action::ThorSwap(action) => action.balances(deps, env, denoms),
            Action::OptimalSwap(action) => action.balances(deps, env, denoms),
            Action::SetLimitOrder(action) => action.balances(deps, env, denoms),
            Action::Distribute(action) => action.balances(deps, env, denoms),
            Action::Schedule(action) => action.balances(deps, env, denoms),
            Action::FundStrategy(action) => action.balances(deps, env, denoms),
            Action::Conditional(condition) => condition.balances(deps, env, denoms),
            Action::Many(action) => action.balances(deps, env, denoms),
        }
    }

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            Action::FinSwap(action) => action.withdraw(deps, env, desired),
            Action::ThorSwap(action) => action.withdraw(deps, env, desired),
            Action::OptimalSwap(action) => action.withdraw(deps, env, desired),
            Action::SetLimitOrder(action) => action.withdraw(deps, env, desired),
            Action::Distribute(action) => action.withdraw(deps, env, desired),
            Action::Schedule(action) => action.withdraw(deps, env, desired),
            Action::FundStrategy(action) => action.withdraw(deps, env, desired),
            Action::Conditional(condition) => condition.withdraw(deps, env, desired),
            Action::Many(action) => action.withdraw(deps, env, desired),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            Action::FinSwap(action) => action.cancel(deps, env),
            Action::ThorSwap(action) => action.cancel(deps, env),
            Action::OptimalSwap(action) => action.cancel(deps, env),
            Action::SetLimitOrder(action) => action.cancel(deps, env),
            Action::Distribute(action) => action.cancel(deps, env),
            Action::Schedule(action) => action.cancel(deps, env),
            Action::FundStrategy(action) => action.cancel(deps, env),
            Action::Conditional(condition) => condition.cancel(deps, env),
            Action::Many(action) => action.cancel(deps, env),
        }
    }
}
