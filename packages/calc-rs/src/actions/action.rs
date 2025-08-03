use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdResult};

use crate::{
    actions::{
        conditional::Conditional,
        distribution::Distribution,
        limit_order::LimitOrder,
        operation::{StatefulOperation, StatelessOperation},
        swaps::swap::Swap,
    },
    manager::Affiliate,
    strategy::{ActionNode, StrategyMsg},
};

#[cw_serde]
pub enum Action {
    Swap(Swap),
    LimitOrder(LimitOrder),
    Distribute(Distribution),
    // Schedule(Schedule),
    Conditional(Conditional),
    // Many(Vec<Action>),
}

impl Action {
    pub fn to_executable_array(self, start_index: u16) -> Vec<ActionNode> {
        match self {
            Action::Conditional(ref action) => {
                let mut current_index = start_index;

                let mut root_node = ActionNode {
                    action: self.clone(),
                    index: current_index,
                    next: None,
                };

                let mut nodes = vec![];

                for action in action.actions.clone().into_iter() {
                    let action_nodes = action.to_executable_array(current_index + 1);
                    current_index += (action_nodes.len() + 1) as u16;
                    nodes.extend(action_nodes);
                }

                root_node.next = Some(start_index + ((nodes.len() + 1) as u16));
                return vec![root_node].into_iter().chain(nodes).collect();
            }
            _ => vec![ActionNode {
                action: self,
                index: start_index,
                next: Some(start_index + 1),
            }],
        }
    }

    pub fn size(&self) -> usize {
        match self {
            Action::Swap(action) => action.routes.len() * 4 + 1,
            Action::Distribute(action) => action.destinations.len() + 1,
            Action::LimitOrder(_) => 4,
            // Action::Schedule(schedule) => {
            //     schedule.actions.iter().map(|a| a.size()).sum::<usize>() + 1
            // }
            Action::Conditional(conditional) => {
                conditional.actions.iter().map(|a| a.size()).sum::<usize>() + 1
            } // Action::Many(actions) => actions.iter().map(|a| a.size()).sum::<usize>() + 1,
        }
    }

    pub fn add_affiliates(self, affiliates: &Vec<Affiliate>) -> StdResult<Action> {
        Ok(match self {
            Action::Distribute(distribution) => {
                Action::Distribute(distribution.with_affiliates(affiliates)?)
            }
            Action::Swap(swap) => Action::Swap(swap.with_affiliates()),
            // Action::Schedule(schedule) => {
            //     let mut initialised_actions = vec![];

            //     for action in schedule.actions {
            //         initialised_actions.push(Self::add_affiliates(action, affiliates)?);
            //     }

            //     Action::Schedule(Schedule {
            //         actions: initialised_actions,
            //         ..schedule
            //     })
            // }
            Action::Conditional(conditional) => {
                let mut initialised_actions = vec![];

                for action in conditional.actions {
                    initialised_actions.push(Self::add_affiliates(action, affiliates)?);
                }

                Action::Conditional(Conditional {
                    actions: initialised_actions,
                    ..conditional
                })
            }
            // Action::Many(actions) => {
            //     let mut initialised_actions = vec![];

            //     for action in actions {
            //         initialised_actions.push(Self::add_affiliates(action, affiliates)?);
            //     }

            //     Action::Many(initialised_actions)
            // }
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
            // Action::Schedule(action) => action.init(deps, env),
            Action::Conditional(action) => action.init(deps, env),
            // Action::Many(action) => action.init(deps, env),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        match self {
            Action::Swap(action) => action.execute(deps, env),
            Action::LimitOrder(action) => action.execute(deps, env),
            Action::Distribute(action) => action.execute(deps, env),
            // Action::Schedule(action) => action.execute(deps, env),
            Action::Conditional(action) => action.execute(deps, env),
            // Action::Many(action) => action.execute(deps, env),
        }
    }

    fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Action::Swap(action) => action.denoms(deps, env),
            Action::LimitOrder(action) => action.denoms(deps, env),
            Action::Distribute(action) => action.denoms(deps, env),
            // Action::Schedule(action) => action.denoms(deps, env),
            Action::Conditional(action) => action.denoms(deps, env),
            // Action::Many(actions) => actions.denoms(deps, env),
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Action::Swap(action) => action.escrowed(deps, env),
            Action::LimitOrder(action) => action.escrowed(deps, env),
            Action::Distribute(action) => action.escrowed(deps, env),
            // Action::Schedule(action) => action.escrowed(deps, env),
            Action::Conditional(action) => action.escrowed(deps, env),
            // Action::Many(action) => action.escrowed(deps, env),
        }
    }
}

impl StatefulOperation for Action {
    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        match self {
            Action::LimitOrder(action) => action.balances(deps, env, denoms),
            Action::Conditional(conditional) => conditional.balances(deps, env, denoms),
            // Action::Many(actions) => actions.balances(deps, env, denoms),
            // Action::Schedule(schedule) => schedule.balances(deps, env, denoms),
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
            // Action::Many(actions) => actions.withdraw(deps, env, desired),
            // Action::Schedule(schedule) => schedule.withdraw(deps, env, desired),
            _ => Ok((vec![], vec![], self)),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        match self {
            Action::LimitOrder(action) => action.cancel(deps, env),
            Action::Conditional(conditional) => conditional.cancel(deps, env),
            // Action::Many(actions) => actions.cancel(deps, env),
            // Action::Schedule(schedule) => schedule.cancel(deps, env),
            _ => Ok((vec![], vec![], self)),
        }
    }

    fn commit(self, deps: Deps, env: &Env) -> StdResult<Action> {
        match self {
            Action::LimitOrder(limit_order) => limit_order.commit(deps, env),
            Action::Conditional(conditional) => conditional.commit(deps, env),
            // Action::Schedule(scheduled) => scheduled.commit(deps, env),
            // Action::Many(actions) => actions.commit(deps, env),
            _ => Ok(self),
        }
    }
}
