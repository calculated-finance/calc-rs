use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdResult};

use crate::{
    actions::{
        conditional::Conditional,
        distribution::Distribution,
        fin_swap::FinSwap,
        limit_order::LimitOrder,
        operation::{StatefulOperation, StatelessOperation},
        optimal_swap::OptimalSwap,
        schedule::Schedule,
        thor_swap::ThorSwap,
    },
    strategy::StrategyMsg,
};

#[cw_serde]
pub enum Action {
    FinSwap(FinSwap),
    ThorSwap(ThorSwap),
    OptimalSwap(OptimalSwap),
    LimitOrder(LimitOrder),
    Distribute(Distribution),
    Schedule(Schedule),
    Conditional(Conditional),
    Many(Vec<Action>),
}

impl Action {
    pub fn size(&self) -> usize {
        match self {
            Action::Schedule(action) => action.action.size() + 1,
            Action::Conditional(action) => action.action.size() + 1,
            Action::Many(actions) => actions.iter().map(|a| a.size()).sum(),
            _ => 1,
        }
    }
}

impl StatelessOperation for Action {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        match self {
            Action::FinSwap(action) => action.init(deps, env),
            Action::ThorSwap(action) => action.init(deps, env),
            Action::OptimalSwap(action) => action.init(deps, env),
            Action::LimitOrder(action) => action.init(deps, env),
            Action::Distribute(action) => action.init(deps, env),
            Action::Schedule(action) => action.init(deps, env),
            Action::Conditional(action) => action.init(deps, env),
            Action::Many(action) => action.init(deps, env),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        match self {
            Action::FinSwap(action) => action.execute(deps, env),
            Action::ThorSwap(action) => action.execute(deps, env),
            Action::OptimalSwap(action) => action.execute(deps, env),
            Action::LimitOrder(action) => action.execute(deps, env),
            Action::Distribute(action) => action.execute(deps, env),
            Action::Schedule(action) => action.execute(deps, env),
            Action::Conditional(action) => action.execute(deps, env),
            Action::Many(action) => action.execute(deps, env),
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Action::FinSwap(action) => action.escrowed(deps, env),
            Action::ThorSwap(action) => action.escrowed(deps, env),
            Action::OptimalSwap(action) => action.escrowed(deps, env),
            Action::LimitOrder(action) => action.escrowed(deps, env),
            Action::Distribute(action) => action.escrowed(deps, env),
            Action::Schedule(action) => action.escrowed(deps, env),
            Action::Conditional(action) => action.escrowed(deps, env),
            Action::Many(action) => action.escrowed(deps, env),
        }
    }
}

impl StatefulOperation for Action {
    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        match self {
            Action::LimitOrder(action) => action.balances(deps, env, denoms),
            Action::Conditional(conditional) => conditional.action.balances(deps, env, denoms),
            Action::Many(actions) => actions.balances(deps, env, denoms),
            Action::Schedule(schedule) => schedule.action.balances(deps, env, denoms),
            _ => Ok(Coins::default()),
        }
    }

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        match self {
            Action::LimitOrder(action) => action.withdraw(deps, env, desired),
            Action::Conditional(conditional) => conditional.action.withdraw(deps, env, desired),
            Action::Many(actions) => actions.withdraw(deps, env, desired),
            Action::Schedule(schedule) => schedule.action.withdraw(deps, env, desired),
            _ => Ok((vec![], vec![], self)),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        match self {
            Action::LimitOrder(action) => action.cancel(deps, env),
            Action::Conditional(conditional) => conditional.action.cancel(deps, env),
            Action::Many(actions) => actions.cancel(deps, env),
            Action::Schedule(schedule) => schedule.action.cancel(deps, env),
            _ => Ok((vec![], vec![], self)),
        }
    }

    fn commit(self, deps: Deps, env: &Env) -> Action {
        match self {
            Action::LimitOrder(limit_order) => limit_order.commit(deps, env),
            Action::Conditional(conditional) => Action::Conditional(Conditional {
                action: Box::new(conditional.action.commit(deps, env)),
                ..conditional
            }),
            Action::Schedule(schedule) => Action::Schedule(Schedule {
                action: Box::new(schedule.action.commit(deps, env)),
                ..schedule
            }),
            Action::Many(actions) => actions.commit(deps, env),
            _ => self,
        }
    }
}
