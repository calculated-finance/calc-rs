use std::{collections::HashSet, mem::discriminant};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, CosmosMsg, Decimal, Deps, Env, StdError, StdResult, Uint128};

use crate::{
    actions::swaps::{fin::FinRoute, thor::ThorchainRoute},
    manager::Affiliate,
    operation::Operation,
};

#[cw_serde]
pub enum SwapAmountAdjustment {
    Fixed,
    LinearScalar {
        base_receive_amount: Coin,
        minimum_swap_amount: Option<Coin>,
        scalar: Decimal,
    },
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
pub struct Executable {
    pub expected_amount_out: Coin,
}

pub trait Quotable {
    fn validate(&self, deps: Deps, route: &SwapQuote<New>) -> StdResult<()>;
    fn adjust(
        &self,
        deps: Deps,
        env: &Env,
        route: SwapQuote<New>,
    ) -> StdResult<SwapQuote<Adjusted>>;
    fn validate_adjusted(
        &self,
        deps: Deps,
        env: &Env,
        route: SwapQuote<Adjusted>,
    ) -> StdResult<SwapQuote<Executable>>;
    fn execute(
        &self,
        deps: Deps,
        env: &Env,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
    ) -> StdResult<CosmosMsg>;
}

impl Quotable for SwapRoute {
    fn validate(&self, deps: Deps, quote: &SwapQuote<New>) -> StdResult<()> {
        match self {
            SwapRoute::Fin(route) => route.validate(deps, quote),
            SwapRoute::Thorchain(route) => route.validate(deps, quote),
        }
    }

    fn adjust(
        &self,
        deps: Deps,
        env: &Env,
        quote: SwapQuote<New>,
    ) -> StdResult<SwapQuote<Adjusted>> {
        match self {
            SwapRoute::Fin(pair_address) => pair_address.adjust(deps, env, quote),
            SwapRoute::Thorchain(route) => route.adjust(deps, env, quote),
        }
    }

    fn validate_adjusted(
        &self,
        deps: Deps,
        env: &Env,
        quote: SwapQuote<Adjusted>,
    ) -> StdResult<SwapQuote<Executable>> {
        match self {
            SwapRoute::Fin(pair_address) => pair_address.validate_adjusted(deps, env, quote),
            SwapRoute::Thorchain(route) => route.validate_adjusted(deps, env, quote),
        }
    }

    fn execute(
        &self,
        deps: Deps,
        env: &Env,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
    ) -> StdResult<CosmosMsg> {
        match self {
            SwapRoute::Fin(route) => route.execute(deps, env, swap_amount, minimum_receive_amount),
            SwapRoute::Thorchain(route) => {
                route.execute(deps, env, swap_amount, minimum_receive_amount)
            }
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
    pub fn validate(&self, deps: Deps) -> StdResult<()> {
        self.route.validate(deps, self)
    }

    pub fn adjust(self, deps: Deps, env: &Env) -> StdResult<SwapQuote<Adjusted>> {
        self.route.clone().adjust(deps, env, self)
    }
}

impl SwapQuote<Adjusted> {
    pub fn validate(self, deps: Deps, env: &Env) -> StdResult<SwapQuote<Executable>> {
        self.route.clone().validate_adjusted(deps, env, self)
    }
}

impl SwapQuote<Executable> {
    pub fn execute(&self, deps: Deps, env: &Env) -> StdResult<CosmosMsg> {
        self.route.execute(
            deps,
            env,
            self.swap_amount.clone(),
            self.minimum_receive_amount.clone(),
        )
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
    pub fn validate(&self, deps: Deps) -> StdResult<()> {
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
            SwapQuote {
                swap_amount: self.swap_amount.clone(),
                minimum_receive_amount: self.minimum_receive_amount.clone(),
                maximum_slippage_bps: self.maximum_slippage_bps,
                adjustment: self.adjustment.clone(),
                route: route.clone(),
                state: New,
            }
            .validate(deps)?;
        }

        Ok(())
    }

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

    pub fn best_quote(&self, deps: Deps, env: &Env) -> StdResult<Option<SwapQuote<Executable>>> {
        let mut best_quote = None;
        let mut best_amount = Uint128::zero();

        for route in &self.routes {
            let quote = SwapQuote {
                swap_amount: self.swap_amount.clone(),
                minimum_receive_amount: self.minimum_receive_amount.clone(),
                maximum_slippage_bps: self.maximum_slippage_bps,
                adjustment: self.adjustment.clone(),
                route: route.clone(),
                state: New,
            }
            .adjust(deps, env)
            .and_then(|adjusted_quote| adjusted_quote.validate(deps, env));

            if let Ok(validated_quote) = quote {
                if validated_quote.state.expected_amount_out.amount > best_amount {
                    best_amount = validated_quote.state.expected_amount_out.amount;
                    best_quote = Some(validated_quote);
                }
            }
        }

        Ok(best_quote)
    }

    pub fn execute_unsafe(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Swap)> {
        let best_quote = self.best_quote(deps, env)?;

        if let Some(quote) = best_quote {
            let swap_message = quote.execute(deps, env)?;

            let updated_routes = self
                .routes
                .iter()
                .map(|r| {
                    if discriminant(r) == discriminant(&quote.route) {
                        quote.route.clone()
                    } else {
                        r.clone()
                    }
                })
                .collect::<Vec<_>>();

            Ok((
                vec![swap_message],
                Swap {
                    // Some routes (i.e. Thorchain) may have relevant state that cannot be
                    // verifiably committed or recreated, so we cache it here.
                    routes: updated_routes,
                    ..self
                },
            ))
        } else {
            Ok((vec![], self))
        }
    }
}

impl Operation<Swap> for Swap {
    fn init(self, deps: Deps, _env: &Env, _affiliates: &[Affiliate]) -> StdResult<Swap> {
        self.validate(deps)?;
        Ok(self.with_affiliates())
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<CosmosMsg>, Swap) {
        match self.clone().execute_unsafe(deps, env) {
            Ok((messages, swap)) => (messages, swap),
            Err(_) => (vec![], self),
        }
    }

    fn denoms(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::from([
            self.swap_amount.denom.clone(),
            self.minimum_receive_amount.denom.clone(),
        ]))
    }
}
