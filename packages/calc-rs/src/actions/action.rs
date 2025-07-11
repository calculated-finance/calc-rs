use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdResult};

use crate::{
    actions::{
        conditional::Conditional,
        distribution::Distribution,
        limit_order::LimitOrder,
        operation::{StatefulOperation, StatelessOperation},
        schedule::Schedule,
        swaps::swap::Swap,
    },
    manager::Affiliate,
    strategy::StrategyMsg,
};

#[cw_serde]
pub enum Action {
    Swap(Swap),
    LimitOrder(LimitOrder),
    Distribute(Distribution),
    Schedule(Schedule),
    Conditional(Conditional),
    Many(Vec<Action>),
}

impl Action {
    pub fn size(&self) -> usize {
        match self {
            Action::Swap(action) => action.routes.len() * 4 + 1,
            Action::Distribute(action) => action.destinations.len() + 1,
            Action::LimitOrder(_) => 4,
            Action::Schedule(action) => action.action.size() + 1,
            Action::Conditional(action) => action.action.size() + action.condition.size() + 1,
            Action::Many(actions) => actions.iter().map(|a| a.size()).sum::<usize>() + 1,
        }
    }

    pub fn add_affiliates(self, affiliates: &Vec<Affiliate>) -> StdResult<Action> {
        Ok(match self {
            Action::Distribute(distribution) => {
                Action::Distribute(distribution.with_affiliates(affiliates)?)
            }
            Action::Swap(swap) => Action::Swap(swap.with_affiliates()),
            Action::Schedule(schedule) => Action::Schedule(Schedule {
                action: Box::new(Self::add_affiliates(*schedule.action, affiliates)?),
                ..schedule
            }),
            Action::Conditional(conditional) => Action::Conditional(Conditional {
                action: Box::new(Self::add_affiliates(*conditional.action, affiliates)?),
                ..conditional
            }),
            Action::Many(actions) => {
                let mut initialised_actions = vec![];

                for action in actions {
                    initialised_actions.push(Self::add_affiliates(action, affiliates)?);
                }

                Action::Many(initialised_actions)
            }
            _ => self,
        })
    }
}

impl StatelessOperation for Action {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        match self {
            Action::Swap(action) => action.init(deps, env),
            Action::LimitOrder(action) => action.init(deps, env),
            Action::Distribute(action) => action.init(deps, env),
            Action::Schedule(action) => action.init(deps, env),
            Action::Conditional(action) => action.init(deps, env),
            Action::Many(action) => action.init(deps, env),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        match self {
            Action::Swap(action) => action.execute(deps, env),
            Action::LimitOrder(action) => action.execute(deps, env),
            Action::Distribute(action) => action.execute(deps, env),
            Action::Schedule(action) => action.execute(deps, env),
            Action::Conditional(action) => action.execute(deps, env),
            Action::Many(action) => action.execute(deps, env),
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Action::Swap(action) => action.escrowed(deps, env),
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
            Action::Conditional(conditional) => conditional.balances(deps, env, denoms),
            Action::Many(actions) => actions.balances(deps, env, denoms),
            Action::Schedule(schedule) => schedule.balances(deps, env, denoms),
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
            Action::Conditional(conditional) => conditional.withdraw(deps, env, desired),
            Action::Many(actions) => actions.withdraw(deps, env, desired),
            Action::Schedule(schedule) => schedule.withdraw(deps, env, desired),
            _ => Ok((vec![], vec![], self)),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        match self {
            Action::LimitOrder(action) => action.cancel(deps, env),
            Action::Conditional(conditional) => conditional.cancel(deps, env),
            Action::Many(actions) => actions.cancel(deps, env),
            Action::Schedule(schedule) => schedule.cancel(deps, env),
            _ => Ok((vec![], vec![], self)),
        }
    }

    fn commit(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        match self {
            Action::LimitOrder(limit_order) => limit_order.commit(deps, env),
            Action::Conditional(conditional) => conditional.commit(deps, env),
            Action::Schedule(scheduled) => scheduled.commit(deps, env),
            Action::Many(actions) => actions.commit(deps, env),
            _ => Ok((vec![], vec![], self)),
        }
    }
}
