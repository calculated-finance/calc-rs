use std::vec;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, CosmosMsg, Deps, Env, StdResult};

use crate::{
    actions::{
        distribution::Distribution, increment::Increment,
        limit_orders::fin_limit_order::FinLimitOrder, swaps::swap::Swap,
        track_account::TrackAccount,
    },
    manager::Affiliate,
    operation::{Operation, StatefulOperation},
};

#[cw_serde]
pub enum Action {
    Swap(Swap),
    LimitOrder(FinLimitOrder),
    Distribute(Distribution),
    Increment(Increment),
    Track(TrackAccount),
}

impl Action {
    pub fn size(&self) -> usize {
        match self {
            Action::Swap(action) => action.routes.len() * 4 + 1,
            Action::Distribute(action) => action.destinations.len() + 1,
            Action::LimitOrder(_) => 4,
            Action::Increment(_) => 0,
            Action::Track(_) => 1,
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
            Action::Increment(counter) => Action::Increment(counter.init(deps, env, affiliates)?),
            Action::Track(track) => Action::Track(track.init(deps, env, affiliates)?),
        })
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Action)> {
        match self {
            Action::Swap(swap) => {
                let (messages, swap) = swap.execute(deps, env)?;
                Ok((messages, Action::Swap(swap)))
            }
            Action::LimitOrder(limit_order) => {
                let (messages, limit_order) = limit_order.execute(deps, env)?;
                Ok((messages, Action::LimitOrder(limit_order)))
            }
            Action::Distribute(distribution) => {
                let (messages, distribution) = distribution.execute(deps, env)?;
                Ok((messages, Action::Distribute(distribution)))
            }
            Action::Increment(counter) => {
                let (messages, counter) = counter.execute(deps, env)?;
                Ok((messages, Action::Increment(counter)))
            }
            Action::Track(track) => {
                let (messages, track) = track.execute(deps, env)?;
                Ok((messages, Action::Track(track)))
            }
        }
    }
}

impl StatefulOperation<Action> for Action {
    fn balances(&self, deps: Deps, env: &Env) -> StdResult<Coins> {
        match self {
            Action::LimitOrder(limit_order) => limit_order.balances(deps, env),
            _ => Ok(Coins::default()),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Action)> {
        match self {
            Action::LimitOrder(limit_order) => {
                let (messages, limit_order) = limit_order.cancel(deps, env)?;
                Ok((messages, Action::LimitOrder(limit_order)))
            }
            _ => Ok((vec![], self)),
        }
    }

    fn commit(self, deps: Deps, env: &Env) -> StdResult<Action> {
        match self {
            Action::LimitOrder(limit_order) => {
                let limit_order = limit_order.commit(deps, env)?;
                Ok(Action::LimitOrder(limit_order))
            }
            _ => Ok(self),
        }
    }
}
