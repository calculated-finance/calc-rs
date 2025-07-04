use std::{collections::HashSet, u8, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Decimal, Deps, Env, StdResult, SubMsg, Uint128};

use crate::{
    actions::{
        composite::CompositeAction,
        distribute::{Destination, DistributeAction, Recipient},
        operation::Operation,
        order::OrderAction,
        swap::SwapAction,
    },
    conditions::Condition,
    events::DomainEvent,
    manager::Affiliate,
};

#[cw_serde]
pub enum Action {
    Swap(SwapAction),
    Order(OrderAction),
    Distribute(DistributeAction),
    Composite(CompositeAction),
}

impl Action {
    pub fn with_affiliates(&self, affiliates: &Vec<Affiliate>) -> Self {
        match self {
            Action::Distribute(DistributeAction {
                denoms,
                mutable_destinations,
                immutable_destinations,
            }) => {
                let total_affiliate_bps = affiliates
                    .iter()
                    .fold(0, |acc, affiliate| acc + affiliate.bps);

                let total_shares = mutable_destinations
                    .iter()
                    .chain(immutable_destinations.iter())
                    .fold(Uint128::zero(), |acc, d| acc + d.shares);

                let total_shares_with_fees =
                    total_shares.mul_ceil(Decimal::bps(10_000 + total_affiliate_bps));

                Action::Distribute(DistributeAction {
                    denoms: denoms.clone(),
                    mutable_destinations: mutable_destinations.clone(),
                    immutable_destinations: [
                        immutable_destinations.clone(),
                        affiliates
                            .iter()
                            .map(|affiliate| Destination {
                                recipient: Recipient::Bank {
                                    address: affiliate.address.clone(),
                                },
                                shares: total_shares_with_fees
                                    .mul_floor(Decimal::bps(affiliate.bps)),
                                label: Some(affiliate.code.clone()),
                            })
                            .collect::<Vec<_>>(),
                    ]
                    .concat(),
                })
            }
            Action::Composite(CompositeAction {
                actions,
                conditions,
            }) => Action::Composite(CompositeAction {
                actions: actions
                    .iter()
                    .map(|action| action.with_affiliates(affiliates))
                    .collect(),
                conditions: conditions.clone(),
            }),
            _ => self.clone(),
        }
    }
}

impl Operation for Action {
    fn init(self, deps: Deps, env: &Env) -> StdResult<Action> {
        match self {
            Action::Swap(action) => action.init(deps, env),
            Action::Order(action) => action.init(deps, env),
            Action::Distribute(action) => action.init(deps, env),
            Action::Composite(action) => action.init(deps, env),
        }
    }

    fn condition(&self, env: &Env) -> Option<Condition> {
        match self {
            Action::Swap(action) => action.condition(env),
            Action::Order(action) => action.condition(env),
            Action::Distribute(action) => action.condition(env),
            Action::Composite(action) => action.condition(env),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<DomainEvent>)> {
        match self {
            Action::Swap(action) => action.execute(deps, env),
            Action::Order(action) => action.execute(deps, env),
            Action::Distribute(action) => action.execute(deps, env),
            Action::Composite(action) => action.execute(deps, env),
        }
    }

    fn update(
        self,
        deps: Deps,
        env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<DomainEvent>)> {
        match self {
            Action::Swap(action) => action.update(deps, env, update),
            Action::Order(action) => action.update(deps, env, update),
            Action::Distribute(action) => action.update(deps, env, update),
            Action::Composite(action) => action.update(deps, env, update),
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Action::Swap(action) => action.escrowed(deps, env),
            Action::Order(action) => action.escrowed(deps, env),
            Action::Distribute(action) => action.escrowed(deps, env),
            Action::Composite(action) => action.escrowed(deps, env),
        }
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &[String]) -> StdResult<Coins> {
        match self {
            Action::Swap(action) => action.balances(deps, env, denoms),
            Action::Order(action) => action.balances(deps, env, denoms),
            Action::Distribute(action) => action.balances(deps, env, denoms),
            Action::Composite(action) => action.balances(deps, env, denoms),
        }
    }

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &Coins,
    ) -> StdResult<(Action, Vec<SubMsg>, Coins)> {
        match self {
            Action::Swap(action) => action.withdraw(deps, env, desired),
            Action::Order(action) => action.withdraw(deps, env, desired),
            Action::Distribute(action) => action.withdraw(deps, env, desired),
            Action::Composite(action) => action.withdraw(deps, env, desired),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<DomainEvent>)> {
        match self {
            Action::Swap(action) => action.cancel(deps, env),
            Action::Order(action) => action.cancel(deps, env),
            Action::Distribute(action) => action.cancel(deps, env),
            Action::Composite(action) => action.cancel(deps, env),
        }
    }
}
