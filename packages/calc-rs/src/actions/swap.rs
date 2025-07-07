use std::{collections::HashSet, vec};

use crate::actions::{
    action::Action, fin_swap::FinSwap, operation::Operation, thor_swap::ThorSwap,
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, Coins, Decimal, Deps, Env, Event, StdError, StdResult, SubMsg};

#[cw_serde]
pub enum SwapAmountAdjustment {
    Fixed,
    LinearScalar {
        base_receive_amount: Coin,
        minimum_swap_amount: Option<Coin>,
        scalar: Decimal,
    },
}

pub trait Exchange {
    fn get_expected_amount_out(&self, deps: Deps, env: Env) -> StdResult<Coin>;
}

#[cw_serde]
pub enum SwapRoute {
    Fin(FinSwap),
    Thorchain(ThorSwap),
}

impl SwapRoute {
    pub fn denoms(&self) -> HashSet<String> {
        match self {
            SwapRoute::Fin(swap) => HashSet::from([
                swap.swap_amount.denom.clone(),
                swap.minimum_receive_amount.denom.clone(),
            ]),
            SwapRoute::Thorchain(swap) => HashSet::from([
                swap.swap_amount.denom.clone(),
                swap.minimum_receive_amount.denom.clone(),
            ]),
        }
    }

    pub fn get_expected_amount_out(&self, deps: Deps, env: &Env) -> StdResult<Coin> {
        match self {
            SwapRoute::Fin(swap) => swap.get_expected_amount_out(deps),
            SwapRoute::Thorchain(swap) => swap.get_expected_amount_out(deps, env),
        }
    }
}

impl From<Action> for SwapRoute {
    fn from(action: Action) -> Self {
        match action {
            Action::FinSwap(fin_swap) => SwapRoute::Fin(fin_swap),
            Action::ThorSwap(thor_swap) => SwapRoute::Thorchain(thor_swap),
            _ => panic!("Cannot convert non-swap action to SwapRoute"),
        }
    }
}

impl Operation for SwapRoute {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            SwapRoute::Fin(swap) => swap.init(deps, env),
            SwapRoute::Thorchain(swap) => swap.init(deps, env),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            SwapRoute::Fin(swap) => swap.execute(deps, env),
            SwapRoute::Thorchain(swap) => swap.execute(deps, env),
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            SwapRoute::Fin(swap) => swap.escrowed(deps, env),
            SwapRoute::Thorchain(swap) => swap.escrowed(deps, env),
        }
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        match self {
            SwapRoute::Fin(swap) => swap.balances(deps, env, denoms),
            SwapRoute::Thorchain(swap) => swap.balances(deps, env, denoms),
        }
    }

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            SwapRoute::Fin(swap) => swap.withdraw(deps, env, desired),
            SwapRoute::Thorchain(swap) => swap.withdraw(deps, env, desired),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        match self {
            SwapRoute::Fin(swap) => swap.cancel(deps, env),
            SwapRoute::Thorchain(swap) => swap.cancel(deps, env),
        }
    }
}

#[cw_serde]
pub struct OptimalSwap {
    pub routes: Vec<SwapRoute>,
}

impl Operation for OptimalSwap {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let mut initialised_routes = vec![];
        let mut messages = vec![];
        let mut events = vec![];

        let first_route = self.routes.first().ok_or_else(|| {
            StdError::generic_err("An optimal swap action must contain at least one route")
        })?;

        for route in self.routes.iter() {
            if route.denoms() != first_route.denoms() {
                return Err(StdError::generic_err(
                    "All routes in an optimal swap action must have the same denoms",
                ));
            }

            let (initialised_route, route_messages, route_events) =
                route.clone().init(deps, env)?;

            messages.extend(route_messages);
            events.extend(route_events);

            match initialised_route {
                Action::FinSwap(fin_swap) => initialised_routes.push(SwapRoute::Fin(fin_swap)),
                Action::ThorSwap(thor_swap) => {
                    initialised_routes.push(SwapRoute::Thorchain(thor_swap))
                }
                _ => {
                    return Err(StdError::generic_err(
                        "OptimalSwap can only contain FinSwap or ThorSwap routes",
                    ));
                }
            }
        }

        Ok((Action::OptimalSwap(self), messages, events))
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let best_route_index = self
            .routes
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                r.get_expected_amount_out(deps, env)
                    .ok()
                    .map(|amount| (i, amount))
            })
            .max_by(|a, b| a.1.amount.cmp(&b.1.amount))
            .map(|(i, _)| i);

        if let Some(best_route_index) = best_route_index {
            let best_route = &self.routes[best_route_index];
            let (action, swap_messages, swap_events) = best_route.clone().execute(deps, env)?;

            let mut updated_routes = self.routes.clone();
            updated_routes[best_route_index] = action.into();

            return Ok((
                Action::OptimalSwap(OptimalSwap {
                    routes: updated_routes,
                    ..self
                }),
                swap_messages,
                swap_events,
            ));
        };

        Ok((Action::OptimalSwap(self), vec![], vec![]))
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        let mut escrowed = HashSet::new();

        for route in &self.routes {
            let route_escrowed = route.escrowed(_deps, _env)?;
            escrowed.extend(route_escrowed);
        }

        Ok(escrowed)
    }

    fn balances(&self, _deps: Deps, _env: &Env, _denoms: &HashSet<String>) -> StdResult<Coins> {
        Ok(Coins::default())
    }

    fn withdraw(
        self,
        _deps: Deps,
        _env: &Env,
        _desired: &HashSet<String>,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::OptimalSwap(self), vec![], vec![]))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::OptimalSwap(self), vec![], vec![]))
    }
}
