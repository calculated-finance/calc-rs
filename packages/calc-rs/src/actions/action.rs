use std::vec;

use calc_node_interface::ExternalNode;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, CosmosMsg, Deps, Env, StdError, StdResult};

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
    External(ExternalNode),
}

impl Action {
    pub fn size(&self) -> usize {
        match self {
            Action::Swap(action) => action.routes.len() * 4 + 1,
            Action::Distribute(action) => action.destinations.len() + 1,
            Action::LimitOrder(_) => 4,
            Action::External(_) => 0,
        }
    }
}

impl Operation<Action> for Action {
    fn init(self, deps: Deps, env: &Env, affiliates: &[Affiliate]) -> StdResult<Action> {
        Ok(match self {
            Action::Swap(swap) => Action::Swap(swap.init(deps, env, affiliates)?),
            Action::LimitOrder(limit_order) => {
                Action::LimitOrder(limit_order.init(deps, env, affiliates)?)
            }
            Action::Distribute(distribution) => {
                Action::Distribute(distribution.init(deps, env, affiliates)?)
            }
            Action::External(external) => Action::External(external),
        })
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Action)> {
        Ok(match self {
            Action::Swap(swap) => {
                let (messages, swap) = swap.execute(deps, env)?;
                (messages, Action::Swap(swap))
            }
            Action::LimitOrder(limit_order) => {
                let (messages, limit_order) = limit_order.execute(deps, env)?;
                (messages, Action::LimitOrder(limit_order))
            }
            Action::Distribute(distribution) => {
                let (messages, distribution) = distribution.execute(deps, env)?;
                (messages, Action::Distribute(distribution))
            }
            Action::External(_) => {
                return Err(StdError::generic_err(
                    "External actions must be executed by the strategy adapter",
                ));
            }
        })
    }
}

impl StatefulOperation<Action> for Action {
    fn balances(&self, deps: Deps, env: &Env) -> StdResult<Coins> {
        match self {
            Action::LimitOrder(limit_order) => limit_order.balances(deps, env),
            Action::External(_) => Err(StdError::generic_err(
                "External action balances must be queried by the strategy adapter",
            )),
            _ => Ok(Coins::default()),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Action)> {
        match self {
            Action::LimitOrder(limit_order) => {
                let (messages, limit_order) = limit_order.cancel(deps, env)?;
                Ok((messages, Action::LimitOrder(limit_order)))
            }
            Action::External(_) => Err(StdError::generic_err(
                "External actions must be cancelled by the strategy adapter",
            )),
            _ => Ok((vec![], self)),
        }
    }

    fn commit(self, deps: Deps, env: &Env) -> StdResult<Action> {
        match self {
            Action::LimitOrder(limit_order) => {
                let limit_order = limit_order.commit(deps, env)?;
                Ok(Action::LimitOrder(limit_order))
            }
            Action::External(_) => Err(StdError::generic_err(
                "External actions must be committed by the strategy adapter",
            )),
            _ => Ok(self),
        }
    }
}
