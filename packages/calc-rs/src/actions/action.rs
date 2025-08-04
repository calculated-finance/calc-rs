use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdResult};

use crate::{
    actions::{
        conditional::Conditional, distribution::Distribution, limit_order::LimitOrder,
        operation::Operation, swaps::swap::Swap,
    },
    core::Threshold,
    manager::Affiliate,
    strategy::{OpNode, OperationImpl, StrategyMsg},
};

#[cw_serde]
pub enum Action {
    Swap(Swap),
    LimitOrder(LimitOrder),
    Distribute(Distribution),
    Conditional(Conditional),
}

impl Action {
    pub fn to_operations(self, start_index: u16) -> Vec<OpNode> {
        match self {
            Action::Conditional(ref conditional) => {
                let mut current_index = start_index;

                let conditions_size = conditional.conditions.len();
                let actions_size = conditional.actions.len();

                let mut nodes = vec![];

                for condition in conditional.conditions.clone().into_iter() {
                    let node = OpNode {
                        operation: OperationImpl::Condition(condition),
                        index: current_index,
                        next: if conditional.threshold == Threshold::All {
                            Some(start_index + (conditions_size as u16) + (actions_size as u16))
                        } else {
                            Some(current_index + 1)
                        },
                    };

                    current_index += 1;
                    nodes.push(node);
                }

                for action in conditional.actions.clone().into_iter() {
                    let action_nodes = action.to_operations(current_index + 1);

                    current_index += (action_nodes.len() + 1) as u16;
                    nodes.extend(action_nodes);
                }

                nodes
            }
            _ => vec![OpNode {
                operation: OperationImpl::Action(self),
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
            Action::Conditional(conditional) => {
                conditional.actions.iter().map(|a| a.size()).sum::<usize>() + 1
            }
        }
    }

    pub fn add_affiliates(self, affiliates: &Vec<Affiliate>) -> StdResult<Action> {
        Ok(match self {
            Action::Distribute(distribution) => {
                Action::Distribute(distribution.with_affiliates(affiliates)?)
            }
            Action::Swap(swap) => Action::Swap(swap.with_affiliates()),
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
            _ => self,
        })
    }
}

impl Operation<Action> for Action {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        match self {
            Action::Swap(action) => action.init(deps, env),
            Action::LimitOrder(action) => action.init(deps, env),
            Action::Distribute(action) => action.init(deps, env),
            Action::Conditional(action) => action.init(deps, env),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        match self {
            Action::Swap(action) => action.execute(deps, env),
            Action::LimitOrder(action) => action.execute(deps, env),
            Action::Distribute(action) => action.execute(deps, env),
            Action::Conditional(action) => action.execute(deps, env),
        }
    }

    fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Action::Swap(action) => action.denoms(deps, env),
            Action::LimitOrder(action) => action.denoms(deps, env),
            Action::Distribute(action) => action.denoms(deps, env),
            Action::Conditional(action) => action.denoms(deps, env),
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Action::Swap(action) => action.escrowed(deps, env),
            Action::LimitOrder(action) => action.escrowed(deps, env),
            Action::Distribute(action) => action.escrowed(deps, env),
            Action::Conditional(action) => action.escrowed(deps, env),
        }
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        match self {
            Action::LimitOrder(action) => action.balances(deps, env, denoms),
            Action::Conditional(conditional) => conditional.balances(deps, env, denoms),
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
            _ => Ok((vec![], vec![], self)),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        match self {
            Action::LimitOrder(action) => action.cancel(deps, env),
            Action::Conditional(conditional) => conditional.cancel(deps, env),
            _ => Ok((vec![], vec![], self)),
        }
    }

    fn commit(self, deps: Deps, env: &Env) -> StdResult<Action> {
        match self {
            Action::LimitOrder(limit_order) => limit_order.commit(deps, env),
            Action::Conditional(conditional) => conditional.commit(deps, env),
            _ => Ok(self),
        }
    }
}
