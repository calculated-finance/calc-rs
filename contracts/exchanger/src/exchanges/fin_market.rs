use std::{cmp::max, str::FromStr};

use calc_rs::{
    core::{Callback, Contract, ContractError, ContractResult},
    exchanger::{ExpectedReceiveAmount, Route},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, BankMsg, Coin, Decimal, Deps, Env, MessageInfo, QueryRequest, Response,
    StdError, StdResult, Uint128, WasmQuery,
};
use rujira_rs::fin::{
    BookResponse, ConfigResponse, ExecuteMsg, QueryMsg, SimulationResponse, SwapRequest,
};

use crate::types::{Exchange, Pair, PositionType};

pub fn get_pair(
    deps: Deps,
    swap_denom: &str,
    target_denom: &str,
    route: &Option<Route>,
) -> StdResult<Pair> {
    match route {
        Some(route) => match route {
            Route::FinMarket { address } => {
                let config = deps
                    .querier
                    .query_wasm_smart::<ConfigResponse>(address, &QueryMsg::Config {})?;

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
                    "Route not supported for Fin market exchange",
                ));
            }
        },
        None => {
            return Err(StdError::generic_err(
                "Must provide a Fin market route to get a pair",
            ));
        }
    }
}

fn spot_price(
    deps: Deps,
    swap_denom: &str,
    target_denom: &str,
    route: &Option<Route>,
) -> StdResult<Decimal> {
    let pair = get_pair(deps, swap_denom, target_denom, route)?;

    let position_type = match swap_denom == pair.quote_denom {
        true => PositionType::Enter,
        false => PositionType::Exit,
    };

    let book_response = deps.querier.query_wasm_smart::<BookResponse>(
        pair.address.clone(),
        &QueryMsg::Book {
            limit: Some(1),
            offset: None,
        },
    )?;

    if book_response.base.is_empty() || book_response.quote.is_empty() {
        return Err(StdError::generic_err(format!(
            "Not enough orders found for {} at fin pair {}",
            swap_denom, pair.address
        )));
    }

    let quote_price = (book_response.base[0].price + book_response.quote[0].price)
        / Decimal::from_str("2").unwrap();

    Ok(match position_type {
        PositionType::Enter => quote_price,
        PositionType::Exit => Decimal::one()
            .checked_div(quote_price)
            .expect("should return a valid inverted price for fin sell"),
    })
}

#[cw_serde]
pub struct FinMarketExchange {}

impl FinMarketExchange {
    pub fn new() -> Self {
        FinMarketExchange {}
    }
}

impl Exchange for FinMarketExchange {
    fn expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &str,
        route: &Option<Route>,
    ) -> StdResult<ExpectedReceiveAmount> {
        let pair = get_pair(deps, &swap_amount.denom, &target_denom, route)?;

        let simulation = deps
            .querier
            .query::<SimulationResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: pair.address.into_string(),
                msg: to_json_binary(&QueryMsg::Simulate(swap_amount.clone()))?,
            }))?;

        let spot_price = spot_price(deps, &swap_amount.denom, &target_denom, route)?;

        let optimal_return_amount = max(
            simulation.returned,
            swap_amount.amount.mul_floor(Decimal::one() / spot_price),
        );

        let slippage = Uint128::new(10_000).mul_ceil(
            Decimal::one()
                .checked_sub(Decimal::from_ratio(
                    simulation.returned,
                    optimal_return_amount,
                ))
                .unwrap_or(Decimal::one()),
        );

        Ok(ExpectedReceiveAmount {
            receive_amount: Coin::new(simulation.returned, target_denom),
            slippage_bps: slippage.into(),
        })
    }

    fn swap(
        &self,
        deps: Deps,
        _env: &Env,
        info: &MessageInfo,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        maximum_slippage_bps: u128,
        route: &Option<Route>,
        recipient: Addr,
        on_complete: Option<Callback>,
    ) -> ContractResult {
        let pair = get_pair(
            deps,
            &swap_amount.denom,
            &minimum_receive_amount.denom,
            route,
        )?;

        let expected_receive_amount =
            self.expected_receive_amount(deps, swap_amount, &minimum_receive_amount.denom, route)?;

        if expected_receive_amount.receive_amount.amount < minimum_receive_amount.amount {
            return Err(ContractError::generic_err(format!(
                "Expected amount out {} is less than minimum receive amount {}",
                expected_receive_amount.receive_amount.amount, minimum_receive_amount.amount
            )));
        }

        if expected_receive_amount.slippage_bps > maximum_slippage_bps {
            return Err(ContractError::generic_err(format!(
                "Slippage of {} bps exceeds maximum allowed {} bps",
                expected_receive_amount.slippage_bps, maximum_slippage_bps
            )));
        }

        let swap_msg = Contract(pair.address).call(
            to_json_binary(&ExecuteMsg::Swap(SwapRequest {
                min_return: Some(minimum_receive_amount.amount),
                to: Some(recipient.to_string()),
                callback: None,
            }))?,
            vec![swap_amount.clone()],
        );

        let mut messages = vec![swap_msg];

        if let Some(on_complete) = on_complete {
            // refund the on complete trigger rebate to the sender as we don't need it
            // to schedule a separate trigger given order book swaps are atomic
            let rebate_msg = BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: on_complete.execution_rebate,
            };

            messages.push(rebate_msg.into());

            let after_swap_msg = Contract(on_complete.contract).call(on_complete.msg, vec![]);

            messages.push(after_swap_msg);
        }

        Ok(Response::new().add_messages(messages))
    }
}

#[cfg(test)]
mod expected_receive_amount_tests {
    use super::*;

    use cosmwasm_std::{
        from_json, testing::mock_dependencies, to_json_binary, Addr, Coin, ContractResult, Decimal,
        StdError, SystemResult, Uint128, WasmQuery,
    };
    use rujira_rs::fin::{BookItemResponse, Denoms, Tick};

    #[test]
    fn fails_to_get_expected_receive_amount_from_non_existing_pair() {
        let deps = mock_dependencies();

        let swap_amount = Coin::new(100u128, "uruju");
        let target_denom = "usdc";
        let pair_address = Addr::unchecked("pair-address");

        let result = FinMarketExchange::new()
            .expected_receive_amount(
                deps.as_ref(),
                &swap_amount,
                target_denom,
                &Some(Route::FinMarket {
                    address: pair_address.clone(),
                }),
            )
            .unwrap_err();

        assert_eq!(
            result,
            StdError::generic_err(format!(
                "Querier system error: No such contract: {}",
                pair_address
            ))
        );
    }

    #[test]
    fn gets_expected_receive_amount() {
        let mut deps = mock_dependencies();

        let pair = Pair {
            base_denom: "uruji".to_string(),
            quote_denom: "usdc".to_string(),
            address: Addr::unchecked("pair-address"),
        };

        let swap_amount = Coin::new(100u128, "uruji");

        let target_denom = "usdc";

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json::<QueryMsg>(msg).unwrap() {
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new(&pair.base_denom, &pair.quote_denom),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(10),
                        fee_taker: Decimal::from_str("0.01").unwrap(),
                        fee_maker: Decimal::from_str("0.01").unwrap(),
                        fee_address: "fee-address".to_string(),
                    })
                    .unwrap(),
                    QueryMsg::Simulate(_) => to_json_binary(&SimulationResponse {
                        returned: Uint128::new(130),
                        fee: Uint128::new(10),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.5").unwrap(),
                            total: Uint128::new(1000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.5").unwrap(),
                            total: Uint128::new(1000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!("Unexpected query type"),
                },
                _ => panic!("Unexpected query type"),
            }))
        });

        let expected_amount = FinMarketExchange::new()
            .expected_receive_amount(
                deps.as_ref(),
                &swap_amount,
                target_denom,
                &Some(Route::FinMarket {
                    address: Addr::unchecked("pair-address"),
                }),
            )
            .unwrap();

        assert_eq!(
            expected_amount.receive_amount,
            Coin::new(130u128, target_denom)
        );

        assert_eq!(
            expected_amount.slippage_bps,
            Uint128::new(10_000)
                .mul_ceil(
                    Decimal::one()
                        - Decimal::from_ratio(
                            expected_amount.receive_amount.amount,
                            Uint128::new(150)
                        )
                )
                .into()
        );
    }
}

#[cfg(test)]
mod swap_tests {
    use super::*;

    use calc_rs::core::ContractError;
    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::{
        from_json, ContractResult, Decimal, MessageInfo, StdError, SubMsg, SystemResult, Uint128,
        WasmMsg,
    };
    use cosmwasm_std::{testing::mock_dependencies, to_json_binary, Addr, Coin};
    use rujira_rs::fin::{BookItemResponse, ConfigResponse, Denoms, ExecuteMsg, SwapRequest, Tick};

    #[test]
    fn fails_to_swap_with_non_existing_pair() {
        let deps = mock_dependencies();

        let swap_amount = Coin::new(100u128, "uruji");
        let minimum_receive_amount = Coin::new(50u128, "rune");
        let pair_address = Addr::unchecked("non-existing-pair-address");

        let result = FinMarketExchange::new()
            .swap(
                deps.as_ref(),
                &mock_env(),
                &MessageInfo {
                    sender: Addr::unchecked("sender-address"),
                    funds: vec![swap_amount.clone()],
                },
                &swap_amount,
                &minimum_receive_amount,
                0,
                &Some(Route::FinMarket {
                    address: pair_address.clone(),
                }),
                Addr::unchecked("recipient-address"),
                None,
            )
            .unwrap_err();

        assert_eq!(
            result,
            ContractError::Std(StdError::generic_err(format!(
                "Querier system error: No such contract: {}",
                pair_address
            )))
        );
    }

    #[test]
    fn fails_to_swap_with_no_route() {
        let deps = mock_dependencies();

        let swap_amount = Coin::new(100u128, "uruji");
        let minimum_receive_amount = Coin::new(50u128, "rune");

        let result = FinMarketExchange::new()
            .swap(
                deps.as_ref(),
                &mock_env(),
                &MessageInfo {
                    sender: Addr::unchecked("sender-address"),
                    funds: vec![swap_amount.clone()],
                },
                &swap_amount,
                &minimum_receive_amount,
                0,
                &None,
                Addr::unchecked("recipient-address"),
                None,
            )
            .unwrap_err();

        assert_eq!(
            result,
            ContractError::Std(StdError::generic_err(
                "Must provide a Fin market route to get a pair"
            ))
        );
    }

    #[test]
    fn fails_to_swap_with_non_fin_route() {
        let deps = mock_dependencies();

        let swap_amount = Coin::new(100u128, "uruji");
        let minimum_receive_amount = Coin::new(50u128, "rune");

        let result = FinMarketExchange::new()
            .swap(
                deps.as_ref(),
                &mock_env(),
                &MessageInfo {
                    sender: Addr::unchecked("sender-address"),
                    funds: vec![swap_amount.clone()],
                },
                &swap_amount,
                &minimum_receive_amount,
                0,
                &Some(Route::Thorchain {}),
                Addr::unchecked("recipient-address"),
                None,
            )
            .unwrap_err();

        assert_eq!(
            result,
            ContractError::Std(StdError::generic_err(
                "Route not supported for Fin market exchange"
            ))
        );
    }

    #[test]
    fn fails_to_swap_with_incorrect_route() {
        let mut deps = mock_dependencies();

        let base_denom = "uruji".to_string();
        let quote_denom = "usdc".to_string();

        let pair = Pair {
            base_denom: base_denom.clone(),
            quote_denom: quote_denom.clone(),
            address: Addr::unchecked("pair-address"),
        };

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&ConfigResponse {
                    denoms: Denoms::new(&"not-uruji", &quote_denom.clone()),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(10),
                    fee_taker: Decimal::from_str("0.01").unwrap(),
                    fee_maker: Decimal::from_str("0.01").unwrap(),
                    fee_address: "fee-address".to_string(),
                })
                .unwrap(),
            ))
        });

        let swap_amount = Coin::new(100u128, pair.base_denom.clone());
        let minimum_receive_amount = Coin::new(50u128, pair.quote_denom.clone());

        assert_eq!(
            FinMarketExchange::new()
                .swap(
                    deps.as_ref(),
                    &mock_env(),
                    &MessageInfo {
                        sender: Addr::unchecked("sender-address"),
                        funds: vec![swap_amount.clone()],
                    },
                    &swap_amount,
                    &minimum_receive_amount,
                    0,
                    &Some(Route::FinMarket {
                        address: pair.address.clone(),
                    }),
                    Addr::unchecked("recipient-address"),
                    None,
                )
                .unwrap_err(),
            ContractError::Std(StdError::generic_err(format!(
                "Pair at {} does not support swapping {}",
                pair.address, pair.base_denom
            )))
        );
    }

    #[test]
    fn fails_to_swap_with_insufficient_expected_receive_amount() {
        let mut deps = mock_dependencies();

        let base_denom = "uruji".to_string();
        let quote_denom = "usdc".to_string();

        let pair = Pair {
            base_denom: base_denom.clone(),
            quote_denom: quote_denom.clone(),
            address: Addr::unchecked("pair-address"),
        };

        let expected_receive_amount = Uint128::new(100);

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json::<QueryMsg>(msg).unwrap() {
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new(&pair.base_denom, &pair.quote_denom),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(10),
                        fee_taker: Decimal::from_str("0.01").unwrap(),
                        fee_maker: Decimal::from_str("0.01").unwrap(),
                        fee_address: "fee-address".to_string(),
                    })
                    .unwrap(),
                    QueryMsg::Simulate(_) => to_json_binary(&SimulationResponse {
                        returned: expected_receive_amount,
                        fee: Uint128::new(10),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.0").unwrap(),
                            total: Uint128::new(1000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.0").unwrap(),
                            total: Uint128::new(1000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!(
                        "Unexpected query type {:#?}",
                        from_json::<QueryMsg>(msg).unwrap()
                    ),
                },
                _ => panic!("Unexpected query type {:#?}", query),
            }))
        });

        let swap_amount = Coin::new(100u128, base_denom.clone());
        let minimum_receive_amount = Coin::new(
            expected_receive_amount + Uint128::one(),
            quote_denom.clone(),
        );

        assert_eq!(
            FinMarketExchange::new()
                .swap(
                    deps.as_ref(),
                    &mock_env(),
                    &MessageInfo {
                        sender: Addr::unchecked("sender-address"),
                        funds: vec![swap_amount.clone()],
                    },
                    &swap_amount,
                    &minimum_receive_amount,
                    0,
                    &Some(Route::FinMarket {
                        address: pair.address.clone(),
                    }),
                    Addr::unchecked("recipient-address"),
                    None,
                )
                .unwrap_err(),
            ContractError::Std(StdError::generic_err(format!(
                "Expected amount out {} is less than minimum receive amount {}",
                expected_receive_amount, minimum_receive_amount.amount
            )))
        );
    }

    #[test]
    fn fails_to_swap_with_high_slippage() {
        let mut deps = mock_dependencies();

        let base_denom = "uruji".to_string();
        let quote_denom = "usdc".to_string();

        let pair = Pair {
            base_denom: base_denom.clone(),
            quote_denom: quote_denom.clone(),
            address: Addr::unchecked("pair-address"),
        };

        let expected_receive_amount = Uint128::new(50);

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json::<QueryMsg>(msg).unwrap() {
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new(&pair.base_denom, &pair.quote_denom),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(10),
                        fee_taker: Decimal::from_str("0.01").unwrap(),
                        fee_maker: Decimal::from_str("0.01").unwrap(),
                        fee_address: "fee-address".to_string(),
                    })
                    .unwrap(),
                    QueryMsg::Simulate(_) => to_json_binary(&SimulationResponse {
                        returned: expected_receive_amount,
                        fee: Uint128::new(10),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.0").unwrap(),
                            total: Uint128::new(1000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.0").unwrap(),
                            total: Uint128::new(1000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!(
                        "Unexpected query type {:#?}",
                        from_json::<QueryMsg>(msg).unwrap()
                    ),
                },
                _ => panic!("Unexpected query type {:#?}", query),
            }))
        });

        let swap_amount = Coin::new(100u128, base_denom.clone());
        let minimum_receive_amount = Coin::new(
            expected_receive_amount - Uint128::one(),
            quote_denom.clone(),
        );

        assert_eq!(
            FinMarketExchange::new()
                .swap(
                    deps.as_ref(),
                    &mock_env(),
                    &MessageInfo {
                        sender: Addr::unchecked("sender-address"),
                        funds: vec![swap_amount.clone()],
                    },
                    &swap_amount,
                    &minimum_receive_amount,
                    100,
                    &Some(Route::FinMarket {
                        address: pair.address.clone(),
                    }),
                    Addr::unchecked("recipient-address"),
                    None,
                )
                .unwrap_err(),
            ContractError::Std(StdError::generic_err(format!(
                "Slippage of {} bps exceeds maximum allowed {} bps",
                5000, 100
            )))
        );
    }

    #[test]
    fn swaps_with_existing_pair() {
        let mut deps = mock_dependencies();

        let base_denom = "uruji".to_string();
        let quote_denom = "usdc".to_string();

        let pair = Pair {
            base_denom: base_denom.clone(),
            quote_denom: quote_denom.clone(),
            address: Addr::unchecked("pair-address"),
        };

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json::<QueryMsg>(msg).unwrap() {
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new(&pair.base_denom, &pair.quote_denom),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(10),
                        fee_taker: Decimal::from_str("0.01").unwrap(),
                        fee_maker: Decimal::from_str("0.01").unwrap(),
                        fee_address: "fee-address".to_string(),
                    })
                    .unwrap(),
                    QueryMsg::Simulate(_) => to_json_binary(&SimulationResponse {
                        returned: Uint128::new(130),
                        fee: Uint128::new(10),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.0").unwrap(),
                            total: Uint128::new(1000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.0").unwrap(),
                            total: Uint128::new(1000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!(
                        "Unexpected query type {:#?}",
                        from_json::<QueryMsg>(msg).unwrap()
                    ),
                },
                _ => panic!("Unexpected query type {:#?}", query),
            }))
        });

        let swap_amount = Coin::new(100u128, base_denom.clone());
        let minimum_receive_amount = Coin::new(50u128, quote_denom.clone());
        let recipient = Addr::unchecked("recipient-address");

        let response = FinMarketExchange::new()
            .swap(
                deps.as_ref(),
                &mock_env(),
                &MessageInfo {
                    sender: Addr::unchecked("sender-address"),
                    funds: vec![swap_amount.clone()],
                },
                &swap_amount,
                &minimum_receive_amount,
                100,
                &Some(Route::FinMarket {
                    address: pair.address.clone(),
                }),
                recipient.clone(),
                None,
            )
            .unwrap();

        assert_eq!(response.messages.len(), 1);
        assert_eq!(
            response.messages[0].msg,
            Contract(pair.address).call(
                to_json_binary(&ExecuteMsg::Swap(SwapRequest {
                    min_return: Some(minimum_receive_amount.amount),
                    to: Some(recipient.to_string()),
                    callback: None,
                }))
                .unwrap(),
                vec![swap_amount.clone()]
            )
        );
    }

    #[test]
    fn refunds_rebate_if_on_complete_provided() {
        let mut deps = mock_dependencies();

        let base_denom = "uruji".to_string();
        let quote_denom = "usdc".to_string();

        let pair = Pair {
            base_denom: base_denom.clone(),
            quote_denom: quote_denom.clone(),
            address: Addr::unchecked("pair-address"),
        };

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json::<QueryMsg>(msg).unwrap() {
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new(&pair.base_denom, &pair.quote_denom),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(10),
                        fee_taker: Decimal::from_str("0.01").unwrap(),
                        fee_maker: Decimal::from_str("0.01").unwrap(),
                        fee_address: "fee-address".to_string(),
                    })
                    .unwrap(),
                    QueryMsg::Simulate(_) => to_json_binary(&SimulationResponse {
                        returned: Uint128::new(130),
                        fee: Uint128::new(10),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.0").unwrap(),
                            total: Uint128::new(1000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.0").unwrap(),
                            total: Uint128::new(1000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!(
                        "Unexpected query type {:#?}",
                        from_json::<QueryMsg>(msg).unwrap()
                    ),
                },
                _ => panic!("Unexpected query type {:#?}", query),
            }))
        });

        let swap_amount = Coin::new(100u128, base_denom.clone());
        let minimum_receive_amount = Coin::new(100u128, quote_denom.clone());
        let recipient = Addr::unchecked("recipient-address");
        let execution_rebate = vec![Coin::new(1u128, "rune")];

        let response = FinMarketExchange::new()
            .swap(
                deps.as_ref(),
                &mock_env(),
                &MessageInfo {
                    sender: Addr::unchecked("sender-address"),
                    funds: vec![swap_amount.clone()],
                },
                &swap_amount,
                &minimum_receive_amount,
                100,
                &Some(Route::FinMarket {
                    address: pair.address.clone(),
                }),
                recipient.clone(),
                Some(Callback {
                    contract: Addr::unchecked("callback-contract"),
                    msg: to_json_binary(&ExecuteMsg::Swap(SwapRequest {
                        min_return: Some(minimum_receive_amount.amount),
                        to: Some(recipient.to_string()),
                        callback: None,
                    }))
                    .unwrap(),
                    execution_rebate: execution_rebate.clone(),
                }),
            )
            .unwrap();

        assert_eq!(
            response.messages[1].msg,
            BankMsg::Send {
                to_address: "sender-address".to_string(),
                amount: execution_rebate,
            }
            .into()
        );
    }

    #[test]
    fn invokes_on_complete_callback() {
        let mut deps = mock_dependencies();

        let base_denom = "uruji".to_string();
        let quote_denom = "usdc".to_string();

        let pair = Pair {
            base_denom: base_denom.clone(),
            quote_denom: quote_denom.clone(),
            address: Addr::unchecked("pair-address"),
        };

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json::<QueryMsg>(msg).unwrap() {
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new(&pair.base_denom, &pair.quote_denom),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(10),
                        fee_taker: Decimal::from_str("0.01").unwrap(),
                        fee_maker: Decimal::from_str("0.01").unwrap(),
                        fee_address: "fee-address".to_string(),
                    })
                    .unwrap(),
                    QueryMsg::Simulate(_) => to_json_binary(&SimulationResponse {
                        returned: Uint128::new(130),
                        fee: Uint128::new(10),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.0").unwrap(),
                            total: Uint128::new(1000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.0").unwrap(),
                            total: Uint128::new(1000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!(
                        "Unexpected query type {:#?}",
                        from_json::<QueryMsg>(msg).unwrap()
                    ),
                },
                _ => panic!("Unexpected query type {:#?}", query),
            }))
        });

        let swap_amount = Coin::new(100u128, base_denom.clone());
        let minimum_receive_amount = Coin::new(100u128, quote_denom.clone());
        let recipient = Addr::unchecked("recipient-address");

        let callback = Callback {
            contract: Addr::unchecked("callback-contract"),
            msg: to_json_binary(&ExecuteMsg::Swap(SwapRequest {
                min_return: Some(minimum_receive_amount.amount),
                to: Some(recipient.to_string()),
                callback: None,
            }))
            .unwrap(),
            execution_rebate: vec![Coin::new(1u128, "rune")],
        };

        let response = FinMarketExchange::new()
            .swap(
                deps.as_ref(),
                &mock_env(),
                &MessageInfo {
                    sender: Addr::unchecked("sender-address"),
                    funds: vec![swap_amount.clone()],
                },
                &swap_amount,
                &minimum_receive_amount,
                100,
                &Some(Route::FinMarket {
                    address: pair.address.clone(),
                }),
                recipient.clone(),
                Some(callback.clone()),
            )
            .unwrap();

        assert_eq!(
            response.messages[2],
            SubMsg::new(WasmMsg::Execute {
                contract_addr: callback.contract.into_string(),
                msg: callback.msg,
                funds: vec![],
            })
        );
    }
}
