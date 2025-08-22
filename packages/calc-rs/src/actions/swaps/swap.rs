use std::{
    cmp::{max, min},
    mem::discriminant,
};

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

impl SwapRoute {
    pub fn validate(&self, deps: Deps, quote: &SwapQuote<New>) -> StdResult<()> {
        match self {
            SwapRoute::Fin(route) => route.validate(deps, quote),
            SwapRoute::Thorchain(route) => route.validate(deps, quote),
        }
    }

    pub fn get_expected_amount_out(
        &self,
        deps: Deps,
        quote: &SwapQuote<New>,
    ) -> StdResult<Uint128> {
        match self {
            SwapRoute::Fin(route) => route.get_expected_amount_out(deps, quote),
            SwapRoute::Thorchain(route) => route.get_expected_amount_out(deps, quote),
        }
    }

    pub fn validate_adjusted(
        self,
        deps: Deps,
        env: &Env,
        quote: SwapQuote<Adjusted>,
    ) -> StdResult<SwapQuote<Executable>> {
        match self {
            SwapRoute::Fin(pair_address) => pair_address.validate_adjusted(deps, env, quote),
            SwapRoute::Thorchain(route) => route.validate_adjusted(deps, env, quote),
        }
    }

    pub fn execute(
        &self,
        deps: Deps,
        env: &Env,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
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
pub struct New;

#[cw_serde]
pub struct Adjusted;

#[cw_serde]
pub struct Executable {
    pub expected_amount_out: Coin,
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
        let swap_balance = deps
            .querier
            .query_balance(&env.contract.address, &self.swap_amount.denom)?;

        let swap_amount = Coin::new(
            min(swap_balance.amount, self.swap_amount.amount),
            self.swap_amount.denom.clone(),
        );

        if swap_amount.amount.is_zero() {
            return Err(StdError::generic_err(
                "Available swap amount is zero".to_string(),
            ));
        }

        let (new_swap_amount, new_minimum_receive_amount) = match &self.adjustment {
            SwapAmountAdjustment::Fixed => {
                let minimum_receive_amount =
                    self.minimum_receive_amount
                        .amount
                        .mul_floor(Decimal::from_ratio(
                            swap_amount.amount,
                            self.swap_amount.amount,
                        ));

                (swap_amount, minimum_receive_amount)
            }
            SwapAmountAdjustment::LinearScalar {
                base_receive_amount,
                minimum_swap_amount,
                scalar,
            } => {
                let expected_amount_out = self.route.get_expected_amount_out(deps, &self)?;

                if expected_amount_out.is_zero() {
                    return Err(StdError::generic_err(
                        "Expected amount out is zero".to_string(),
                    ));
                }

                let base_price =
                    Decimal::from_ratio(self.swap_amount.amount, base_receive_amount.amount);

                let current_price = Decimal::from_ratio(swap_amount.amount, expected_amount_out);

                let price_delta = base_price.abs_diff(current_price) / base_price;

                let scaled_price_delta = price_delta * scalar;

                let scaled_swap_amount = if current_price < base_price {
                    swap_amount
                        .amount
                        .mul_floor(Decimal::one().saturating_add(scaled_price_delta))
                } else {
                    swap_amount
                        .amount
                        .mul_floor(Decimal::one().saturating_sub(scaled_price_delta))
                };

                let new_swap_amount = Coin::new(
                    max(
                        scaled_swap_amount,
                        minimum_swap_amount
                            .clone()
                            .unwrap_or(Coin::new(0u128, self.swap_amount.denom.clone()))
                            .amount,
                    ),
                    self.swap_amount.denom,
                );

                if new_swap_amount.amount.is_zero() {
                    return Err(StdError::generic_err(
                        "Swap amount after adjustment is zero".to_string(),
                    ));
                }

                let new_minimum_receive_amount =
                    self.minimum_receive_amount
                        .amount
                        .mul_ceil(Decimal::from_ratio(
                            new_swap_amount.amount,
                            self.swap_amount.amount,
                        ));

                (new_swap_amount, new_minimum_receive_amount)
            }
        };

        Ok(SwapQuote {
            swap_amount: new_swap_amount,
            minimum_receive_amount: Coin::new(
                new_minimum_receive_amount,
                self.minimum_receive_amount.denom,
            ),
            maximum_slippage_bps: self.maximum_slippage_bps,
            adjustment: self.adjustment,
            route: self.route,
            state: Adjusted,
        })
    }
}

impl SwapQuote<Adjusted> {
    pub fn validate(self, deps: Deps, env: &Env) -> StdResult<SwapQuote<Executable>> {
        self.route.clone().validate_adjusted(deps, env, self)
    }
}

impl SwapQuote<Executable> {
    pub fn execute(&self, deps: Deps, env: &Env) -> StdResult<CosmosMsg> {
        self.route
            .execute(deps, env, &self.swap_amount, &self.minimum_receive_amount)
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

        if let SwapAmountAdjustment::LinearScalar {
            base_receive_amount,
            minimum_swap_amount,
            ..
        } = &self.adjustment
        {
            if base_receive_amount.amount.is_zero() {
                return Err(StdError::generic_err("Base receive amount cannot be zero"));
            }

            if base_receive_amount.denom != self.minimum_receive_amount.denom {
                return Err(StdError::generic_err(
                    "Base receive amount denom must match minimum receive amount denom",
                ));
            }

            if let Some(minimum_swap_amount) = minimum_swap_amount {
                if minimum_swap_amount.denom != self.swap_amount.denom {
                    return Err(StdError::generic_err(
                        "Minimum swap amount denom must match swap amount denom",
                    ));
                }
            }
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

    fn denoms(self, _deps: Deps) -> StdResult<Vec<String>> {
        Ok(vec![
            self.swap_amount.denom,
            self.minimum_receive_amount.denom,
        ])
    }
}

#[cfg(test)]
mod tests {
    use calc_rs_test::mocks::mock_dependencies_with_custom_grpc_querier;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        to_json_binary, Addr, Binary, Coin, ContractResult, Decimal, SystemResult, Uint128,
    };
    use prost::Message;
    use rujira_rs::{fin::SimulationResponse, proto::types::QueryQuoteSwapResponse};

    use crate::actions::swaps::{
        fin::FinRoute,
        swap::{New, SwapAmountAdjustment, SwapQuote, SwapRoute},
        thor::ThorchainRoute,
    };

    #[test]
    fn test_linear_scalar_swap_adjustment_with_fin_route() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier
            .bank
            .update_balance(&env.contract.address, vec![Coin::new(1000u128, "rune")]);

        let quote = SwapQuote {
            swap_amount: Coin::new(1000u128, "rune"),
            minimum_receive_amount: Coin::new(1u128, "x/ruji"),
            maximum_slippage_bps: 300,
            adjustment: SwapAmountAdjustment::LinearScalar {
                base_receive_amount: Coin::new(500u128, "x/ruji"),
                minimum_swap_amount: None,
                scalar: Decimal::one(),
            },
            route: SwapRoute::Fin(FinRoute {
                pair_address: Addr::unchecked("pair_address"),
            }),
            state: New,
        };

        // Price at $2.00

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&SimulationResponse {
                    returned: Uint128::new(500),
                    fee: Uint128::zero(),
                })
                .unwrap(),
            ))
        });

        let adjusted_quote = quote.clone().adjust(deps.as_ref(), &env).unwrap();

        assert_eq!(adjusted_quote.swap_amount, quote.swap_amount);

        // Price at $3.00

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&SimulationResponse {
                    returned: Uint128::new(334),
                    fee: Uint128::zero(),
                })
                .unwrap(),
            ))
        });

        let adjusted_quote = quote.clone().adjust(deps.as_ref(), &env).unwrap();

        assert_eq!(
            adjusted_quote.swap_amount.amount,
            quote.swap_amount.amount.mul_floor(Decimal::permille(502))
        );

        // Price at $1.00

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&SimulationResponse {
                    returned: Uint128::new(1000),
                    fee: Uint128::zero(),
                })
                .unwrap(),
            ))
        });

        let adjusted_quote = quote.clone().adjust(deps.as_ref(), &env).unwrap();

        assert_eq!(
            adjusted_quote.swap_amount.amount,
            quote.swap_amount.amount.mul_floor(Decimal::percent(150))
        );
    }

    #[test]
    fn test_linear_scalar_swap_adjustment_with_thorchain_route() {
        let mut deps = mock_dependencies_with_custom_grpc_querier();
        let env = mock_env();

        deps.querier
            .default
            .bank
            .update_balance(&env.contract.address, vec![Coin::new(1000u128, "rune")]);

        let quote = SwapQuote {
            swap_amount: Coin::new(1000u128, "rune"),
            minimum_receive_amount: Coin::new(1u128, "x/ruji"),
            maximum_slippage_bps: 300,
            adjustment: SwapAmountAdjustment::LinearScalar {
                base_receive_amount: Coin::new(500u128, "x/ruji"),
                minimum_swap_amount: None,
                scalar: Decimal::one(),
            },
            route: SwapRoute::Thorchain(ThorchainRoute {
                streaming_interval: None,
                max_streaming_quantity: None,
                affiliate_code: None,
                affiliate_bps: None,
                latest_swap: None,
            }),
            state: New,
        };

        // Price at $2.00

        deps.querier.with_grpc_handler(|_| {
            let quote = QueryQuoteSwapResponse {
                fees: None,
                expiry: i64::MAX,
                warning: String::new(),
                notes: String::new(),
                dust_threshold: "0".to_string(),
                recommended_min_amount_in: "1".to_string(),
                gas_rate_units: "rune".to_string(),
                memo: "swap".to_string(),
                expected_amount_out: "500".to_string(),
                max_streaming_quantity: 5,
                streaming_swap_blocks: 5,
                inbound_address: "destination".to_string(),
                inbound_confirmation_blocks: 10,
                inbound_confirmation_seconds: 10,
                outbound_delay_blocks: 10,
                outbound_delay_seconds: 10,
                router: String::new(),
                recommended_gas_rate: String::new(),
                streaming_swap_seconds: 10,
                total_swap_seconds: 10,
            };

            let mut buf = Vec::new();
            quote.encode(&mut buf).unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let adjusted_quote = quote.clone().adjust(deps.as_ref(), &env).unwrap();

        assert_eq!(adjusted_quote.swap_amount, quote.swap_amount);

        // Price at $3.00

        deps.querier.with_grpc_handler(|_| {
            let quote = QueryQuoteSwapResponse {
                fees: None,
                expiry: i64::MAX,
                warning: String::new(),
                notes: String::new(),
                dust_threshold: "0".to_string(),
                recommended_min_amount_in: "1".to_string(),
                gas_rate_units: "rune".to_string(),
                memo: "swap".to_string(),
                expected_amount_out: "334".to_string(),
                max_streaming_quantity: 5,
                streaming_swap_blocks: 5,
                inbound_address: "destination".to_string(),
                inbound_confirmation_blocks: 10,
                inbound_confirmation_seconds: 10,
                outbound_delay_blocks: 10,
                outbound_delay_seconds: 10,
                router: String::new(),
                recommended_gas_rate: String::new(),
                streaming_swap_seconds: 10,
                total_swap_seconds: 10,
            };

            let mut buf = Vec::new();
            quote.encode(&mut buf).unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let adjusted_quote = quote.clone().adjust(deps.as_ref(), &env).unwrap();

        assert_eq!(
            adjusted_quote.swap_amount.amount,
            quote.swap_amount.amount.mul_floor(Decimal::permille(502))
        );

        // // Price at $1.00

        deps.querier.with_grpc_handler(|_| {
            let quote = QueryQuoteSwapResponse {
                fees: None,
                expiry: i64::MAX,
                warning: String::new(),
                notes: String::new(),
                dust_threshold: "0".to_string(),
                recommended_min_amount_in: "1".to_string(),
                gas_rate_units: "rune".to_string(),
                memo: "swap".to_string(),
                expected_amount_out: "1000".to_string(),
                max_streaming_quantity: 5,
                streaming_swap_blocks: 5,
                inbound_address: "destination".to_string(),
                inbound_confirmation_blocks: 10,
                inbound_confirmation_seconds: 10,
                outbound_delay_blocks: 10,
                outbound_delay_seconds: 10,
                router: String::new(),
                recommended_gas_rate: String::new(),
                streaming_swap_seconds: 10,
                total_swap_seconds: 10,
            };

            let mut buf = Vec::new();
            quote.encode(&mut buf).unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let adjusted_quote = quote.clone().adjust(deps.as_ref(), &env).unwrap();

        assert_eq!(
            adjusted_quote.swap_amount.amount,
            quote.swap_amount.amount.mul_floor(Decimal::percent(150))
        );
    }
}
