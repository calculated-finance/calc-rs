use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, CosmosMsg, Deps, Env, StdResult};

use crate::{
    actions::{
        distribution::Distribution, limit_orders::fin_limit_order::FinLimitOrder, swaps::swap::Swap,
    },
    manager::Affiliate,
    operation::{Operation, StatefulOperation},
};

#[cw_serde]
pub enum Action {
    Swap(Swap),
    LimitOrder(FinLimitOrder),
    Distribute(Distribution),
}

impl Action {
    pub fn size(&self) -> usize {
        match self {
            Action::Swap(action) => action.routes.len() * 4 + 1,
            Action::Distribute(action) => action.destinations.len() + 1,
            Action::LimitOrder(_) => 4,
        }
    }
}

impl Operation<Action> for Action {
    fn init(self, deps: Deps, env: &Env, affiliates: &[Affiliate]) -> StdResult<Action> {
        match self {
            Action::Swap(action) => action.init(deps, env, affiliates),
            Action::LimitOrder(action) => action.init(deps, env, affiliates),
            Action::Distribute(action) => action.init(deps, env, affiliates),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<CosmosMsg>, Action) {
        match self {
            Action::Swap(action) => action.execute(deps, env),
            Action::LimitOrder(action) => action.execute(deps, env),
            Action::Distribute(action) => action.execute(deps, env),
        }
    }

    fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Action::Swap(action) => action.denoms(deps, env),
            Action::LimitOrder(action) => action.denoms(deps, env),
            Action::Distribute(action) => action.denoms(deps, env),
        }
    }
}

impl StatefulOperation<Action> for Action {
    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        match self {
            Action::LimitOrder(action) => action.balances(deps, env, denoms),
            _ => Ok(Coins::default()),
        }
    }

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Vec<CosmosMsg>, Action)> {
        match self {
            Action::LimitOrder(action) => action.withdraw(deps, env, desired),
            _ => Ok((vec![], self)),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Action)> {
        match self {
            Action::LimitOrder(action) => action.cancel(deps, env),
            _ => Ok((vec![], self)),
        }
    }

    fn commit(self, deps: Deps, env: &Env) -> StdResult<Action> {
        match self {
            Action::LimitOrder(limit_order) => limit_order.commit(deps, env),
            _ => Ok(self),
        }
    }
}
