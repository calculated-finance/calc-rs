use calc_rs::{
    core::{Callback, ContractResult},
    exchanger::{ExpectedReceiveAmount, Route},
};
use cosmwasm_std::{Addr, Coin, Deps, Env, MessageInfo, StdError, StdResult};
use rujira_rs::fin::{ConfigResponse, QueryMsg};

use crate::types::{Exchange, Pair};

pub fn get_pair(
    deps: Deps,
    swap_denom: &str,
    target_denom: &str,
    route: &Option<Route>,
) -> StdResult<Pair> {
    match route {
        Some(route) => match route {
            Route::FinLimit { address } => {
                let config = deps
                    .querier
                    .query_wasm_smart::<ConfigResponse>(address, &QueryMsg::Config {})
                    .map_err(|_| {
                        StdError::generic_err(format!(
                            "Failed to query config for Fin pair at {}",
                            address
                        ))
                    })?;

                let denoms = [config.denoms.base(), config.denoms.quote()];

                if !denoms.contains(&swap_denom) {
                    return Err(StdError::generic_err(format!(
                        "Pair at {} does not support swapping {}",
                        address, swap_denom
                    )));
                }

                if !denoms.contains(&target_denom) {
                    return Err(StdError::generic_err(format!(
                        "Pair at {} does not support swapping {}",
                        address, target_denom
                    )));
                }

                Ok(Pair {
                    base_denom: config.denoms.base().to_string(),
                    quote_denom: config.denoms.quote().to_string(),
                    address: Addr::unchecked(address),
                })
            }
            _ => {
                return Err(StdError::generic_err(
                    "Route not supported for Fin limit exchange",
                ));
            }
        },
        None => {
            return Err(StdError::generic_err(
                "Must provide a Fin limit route to get a pair",
            ));
        }
    }
}

pub struct FinLimitExchange {}

impl FinLimitExchange {
    pub fn new() -> Self {
        FinLimitExchange {}
    }
}

impl Exchange for FinLimitExchange {
    fn expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &str,
        route: &Option<Route>,
    ) -> StdResult<ExpectedReceiveAmount> {
        let pair = get_pair(deps, swap_amount.denom.as_str(), target_denom, route)?;

        Ok(ExpectedReceiveAmount {
            receive_amount: Coin {
                denom: pair.quote_denom,
                amount: swap_amount.amount, // Placeholder logic, replace with actual calculation
            },
            slippage_bps: 0, // Placeholder logic, replace with actual calculation
        })
    }

    fn swap(
        &self,
        deps: Deps,
        env: &Env,
        info: &MessageInfo,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        maximum_slippage_bps: u128,
        route: &Option<Route>,
        recipient: Addr,
        on_complete: Option<Callback>,
    ) -> ContractResult {
        todo!()
    }
}

#[cfg(test)]
mod expected_receive_amount_tests {
    use std::str::FromStr;

    use super::*;
    use cosmwasm_std::{
        from_json, testing::mock_dependencies, to_json_binary, Coin, ContractResult, Decimal,
        StdError, SystemResult, Uint128, WasmQuery,
    };
    use rujira_rs::fin::{BookItemResponse, BookResponse, Denoms, Tick};

    #[test]
    fn fails_if_no_route_provided() {
        let deps = mock_dependencies();
        let swap_amount = Coin::new(100u128, "rune");
        let target_denom = "btc";

        let error = FinLimitExchange::new()
            .expected_receive_amount(deps.as_ref(), &swap_amount, target_denom, &None)
            .unwrap_err();

        assert_eq!(
            error,
            StdError::generic_err("Must provide a Fin limit route to get a pair")
        );
    }

    #[test]
    fn fails_if_route_not_fin_limit() {
        let deps = mock_dependencies();
        let swap_amount = Coin::new(100u128, "rune");
        let target_denom = "btc";

        let error = FinLimitExchange::new()
            .expected_receive_amount(
                deps.as_ref(),
                &swap_amount,
                target_denom,
                &Some(Route::FinMarket {
                    address: Addr::unchecked("pair_address"),
                }),
            )
            .unwrap_err();

        assert_eq!(
            error,
            StdError::generic_err("Route not supported for Fin market exchange")
        );
    }

    #[test]
    fn fails_if_pair_does_not_exist() {
        let deps = mock_dependencies();
        let swap_amount = Coin::new(100u128, "rune");
        let target_denom = "btc";

        let pair_address = Addr::unchecked("test_address");

        let error = FinLimitExchange::new()
            .expected_receive_amount(
                deps.as_ref(),
                &swap_amount,
                target_denom,
                &Some(Route::FinLimit {
                    address: pair_address.clone(),
                }),
            )
            .unwrap_err();

        assert_eq!(
            error,
            StdError::generic_err(format!(
                "Failed to query config for Fin pair at {}",
                pair_address
            ))
        );
    }

    #[test]
    fn returns_expected_receive_amount() {
        let mut deps = mock_dependencies();
        let swap_amount = Coin::new(100u128, "rune");
        let target_denom = "btc";

        let pair_address = Addr::unchecked("test_address");

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new("rune", "btc-btc"),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(1u8),
                        fee_taker: Decimal::from_str("0.01").unwrap(),
                        fee_maker: Decimal::from_str("0.01").unwrap(),
                        fee_address: "fee_address".to_string(),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("110.0").unwrap(),
                            total: Uint128::new(1000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("90.0").unwrap(),
                            total: Uint128::new(1000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!("Unexpected query message"),
                },
                _ => panic!("Unexpected query type"),
            }))
        });

        let expected = FinLimitExchange::new()
            .expected_receive_amount(
                deps.as_ref(),
                &swap_amount,
                target_denom,
                &Some(Route::FinLimit {
                    address: pair_address.clone(),
                }),
            )
            .unwrap();

        assert_eq!(
            expected,
            ExpectedReceiveAmount {
                receive_amount: Coin::new(11000u128, "btc-btc"),
                slippage_bps: 0
            }
        );
    }
}
