use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdResult};

use crate::{
    actions::{
        conditional::Conditional, distribution::Distribution, limit_order::LimitOrder,
        operation::Operation, swaps::swap::Swap,
    },
    manager::Affiliate,
    strategy::StrategyMsg,
};

#[cw_serde]
pub enum Action {
    Swap(Swap),
    LimitOrder(LimitOrder),
    Distribute(Distribution),
    Conditional(Conditional),
}

impl Action {
    pub fn size(&self) -> usize {
        match self {
            Action::Swap(action) => action.routes.len() * 4 + 1,
            Action::Distribute(action) => action.destinations.len() + 1,
            Action::LimitOrder(_) => 4,
            Action::Conditional(conditional) => {
                conditional
                    .conditions
                    .iter()
                    .map(|c| c.size())
                    .sum::<usize>()
                    + conditional.actions.iter().map(|a| a.size()).sum::<usize>()
                    + 1
            }
        }
    }
}

impl Operation<Action> for Action {
    fn init(self, deps: Deps, env: &Env, affiliates: &[Affiliate]) -> StdResult<Action> {
        match self {
            Action::Swap(action) => action.init(deps, env, affiliates),
            Action::LimitOrder(action) => action.init(deps, env, affiliates),
            Action::Distribute(action) => action.init(deps, env, affiliates),
            Action::Conditional(action) => action.init(deps, env, affiliates),
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
