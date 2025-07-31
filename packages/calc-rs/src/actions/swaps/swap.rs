use std::{collections::HashSet, mem::discriminant};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, Decimal, Deps, Env, Event, StdError, StdResult};

use crate::{
    actions::{
        action::Action,
        operation::StatelessOperation,
        swaps::{fin::FinRoute, thor::ThorchainRoute},
    },
    strategy::StrategyMsg,
};

pub enum SwapEvent {
    SkipSwap { reason: String },
}

impl From<SwapEvent> for Event {
    fn from(val: SwapEvent) -> Self {
        match val {
            SwapEvent::SkipSwap { reason } => {
                Event::new("skip_swap").add_attribute("reason", reason)
            }
        }
    }
}

#[cw_serde]
pub enum SwapAmountAdjustment {
    Fixed,
    LinearScalar {
        base_receive_amount: Coin,
        minimum_swap_amount: Option<Coin>,
        scalar: Decimal,
    },
}

pub trait Routable {
    fn get_expected_amount_out(&self, swap_amount: Coin) -> StdResult<Coin>;
}

#[cw_serde]
pub enum SwapRoute {
    Fin(FinRoute),
    Thorchain(ThorchainRoute),
}

#[cw_serde]
pub struct New;

#[cw_serde]
pub struct Adjusted;

#[cw_serde]
pub struct Validated {
    pub expected_amount_out: Coin,
}

#[cw_serde]
pub struct Executable {
    pub messages: Vec<StrategyMsg>,
}

pub trait Quotable {
    fn verify(&self, deps: Deps, route: &SwapQuote<New>) -> StdResult<()>;
    fn adjust(
        &self,
        deps: Deps,
        env: &Env,
        route: &SwapQuote<New>,
    ) -> StdResult<SwapQuote<Adjusted>>;
    fn validate(
        &self,
        deps: Deps,
        env: &Env,
        route: &SwapQuote<Adjusted>,
    ) -> StdResult<SwapQuote<Validated>>;
    fn execute(
        &self,
        deps: Deps,
        env: &Env,
        route: &SwapQuote<Validated>,
    ) -> StdResult<SwapQuote<Executable>>;
}

impl Quotable for SwapRoute {
    fn verify(&self, deps: Deps, quote: &SwapQuote<New>) -> StdResult<()> {
        match self {
            SwapRoute::Fin(route) => route.verify(deps, quote),
            SwapRoute::Thorchain(route) => route.verify(deps, quote),
        }
    }

    fn adjust(
        &self,
        deps: Deps,
        env: &Env,
        quote: &SwapQuote<New>,
    ) -> StdResult<SwapQuote<Adjusted>> {
        match self {
            SwapRoute::Fin(pair_address) => pair_address.adjust(deps, env, quote),
            SwapRoute::Thorchain(route) => route.adjust(deps, env, quote),
        }
    }

    fn validate(
        &self,
        deps: Deps,
        env: &Env,
        quote: &SwapQuote<Adjusted>,
    ) -> StdResult<SwapQuote<Validated>> {
        match self {
            SwapRoute::Fin(pair_address) => pair_address.validate(deps, env, quote),
            SwapRoute::Thorchain(route) => route.validate(deps, env, quote),
        }
    }

    fn execute(
        &self,
        deps: Deps,
        env: &Env,
        quote: &SwapQuote<Validated>,
    ) -> StdResult<SwapQuote<Executable>> {
        match self {
            SwapRoute::Fin(pair_address) => pair_address.execute(deps, env, quote),
            SwapRoute::Thorchain(route) => route.execute(deps, env, quote),
        }
    }
}

#[cw_serde]
pub struct SwapQuote<S> {
    pub swap_amount: Coin,
    pub minimum_receive_amount: Coin,
    pub maximum_slippage_bps: u64,
    pub adjustment: SwapAmountAdjustment,
    pub route: SwapRoute,
    pub state: S,
}

impl SwapQuote<New> {
    pub fn adjust(self, deps: Deps, env: &Env) -> StdResult<SwapQuote<Adjusted>> {
        self.route.adjust(deps, env, &self)
    }
}

impl SwapQuote<Adjusted> {
    pub fn validate(self, deps: Deps, env: &Env) -> StdResult<SwapQuote<Validated>> {
        self.route.validate(deps, env, &self)
    }
}

impl SwapQuote<Validated> {
    pub fn execute(self, deps: Deps, env: &Env) -> StdResult<SwapQuote<Executable>> {
        self.route.execute(deps, env, &self)
    }
}

impl SwapQuote<Executable> {
    pub fn swap_messages(self) -> Vec<StrategyMsg> {
        self.state.messages
    }
}

#[cw_serde]
pub struct Swap {
    pub swap_amount: Coin,
    pub minimum_receive_amount: Coin,
    pub maximum_slippage_bps: u64,
    pub adjustment: SwapAmountAdjustment,
    pub routes: Vec<SwapRoute>,
}

impl Swap {
    pub fn with_affiliates(self) -> Self {
        Swap {
            routes: self
                .routes
                .into_iter()
                .map(|route| match route {
                    SwapRoute::Thorchain(thor_route) => SwapRoute::Thorchain(ThorchainRoute {
                        // As per agreement with Rujira
                        affiliate_code: Some("rj".to_string()),
                        affiliate_bps: Some(10),
                        ..thor_route
                    }),
                    _ => route,
                })
                .collect(),
            ..self
        }
    }

    pub fn best_route(&self, deps: Deps, env: &Env) -> StdResult<Option<SwapQuote<Validated>>> {
        Ok(self
            .routes
            .clone()
            .into_iter()
            .filter_map(|route| {
                route
                    .adjust(
                        deps,
                        env,
                        &SwapQuote {
                            swap_amount: self.swap_amount.clone(),
                            minimum_receive_amount: self.minimum_receive_amount.clone(),
                            maximum_slippage_bps: self.maximum_slippage_bps,
                            adjustment: self.adjustment.clone(),
                            route: route.clone(),
                            state: New,
                        },
                    )
                    .ok()?
                    .validate(deps, env)
                    .ok()
            })
            .max_by(|a, b| {
                a.state
                    .expected_amount_out
                    .amount
                    .cmp(&b.state.expected_amount_out.amount)
            }))
    }

    pub fn execute_unsafe(
        self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        let best_route = self.best_route(deps, env)?;

        if let Some(route) = best_route {
            let messages = route.clone().execute(deps, env)?.swap_messages();

            let updated_routes = self
                .routes
                .iter()
                .map(|r| {
                    if discriminant(r) == discriminant(&route.route) {
                        route.route.clone()
                    } else {
                        r.clone()
                    }
                })
                .collect::<Vec<_>>();

            Ok((
                messages,
                vec![],
                Action::Swap(Swap {
                    swap_amount: self.swap_amount,
                    minimum_receive_amount: self.minimum_receive_amount,
                    maximum_slippage_bps: self.maximum_slippage_bps,
                    adjustment: self.adjustment,
                    // Some routes (i.e. Thorchain) may have relevant state that cannot be
                    // verifiably committed or recreated, so we cache it here.
                    routes: updated_routes,
                }),
            ))
        } else {
            Ok((
                vec![],
                vec![SwapEvent::SkipSwap {
                    reason: "No viable swap route found".to_string(),
                }
                .into()],
                Action::Swap(self),
            ))
        }
    }
}

impl StatelessOperation for Swap {
    fn init(self, deps: Deps, _env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if self.swap_amount.amount.is_zero() {
            return Err(StdError::generic_err("Swap amount cannot be zero"));
        }

        if self.maximum_slippage_bps > 10_000 {
            return Err(StdError::generic_err(
                "Maximum slippage basis points cannot exceed 10,000",
            ));
        }

        if self.routes.is_empty() {
            return Err(StdError::generic_err("No swap routes provided"));
        }

        for route in &self.routes {
            route.verify(
                deps,
                &SwapQuote {
                    swap_amount: self.swap_amount.clone(),
                    minimum_receive_amount: self.minimum_receive_amount.clone(),
                    maximum_slippage_bps: self.maximum_slippage_bps,
                    adjustment: self.adjustment.clone(),
                    route: route.clone(),
                    state: New,
                },
            )?;
        }

        Ok((vec![], vec![], Action::Swap(self)))
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        match self.clone().execute_unsafe(deps, env) {
            Ok((action, messages, events)) => (action, messages, events),
            Err(err) => (
                vec![],
                vec![SwapEvent::SkipSwap {
                    reason: format!("Swap execution failed: {err}"),
                }
                .into()],
                Action::Swap(self),
            ),
        }
    }

    fn denoms(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::from([
            self.swap_amount.denom.clone(),
            self.minimum_receive_amount.denom.clone(),
        ]))
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::from([self.minimum_receive_amount.denom.clone()]))
    }
}
