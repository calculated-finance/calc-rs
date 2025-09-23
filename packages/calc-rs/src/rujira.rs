use cosmwasm_std::{Addr, Decimal, Deps, StdError, StdResult, Uint128};
use rujira_rs::fin::{BookResponse, ConfigResponse, QueryMsg, Side};

const MIN_ORDERS: u8 = 4;
const MIN_VALUE: Uint128 = Uint128::new(10u128.pow(4));
const LIMIT: u8 = 4;
const MAX_ITERATIONS: u8 = 2;

pub fn get_side_price(deps: Deps, pair_address: &Addr, side: &Side) -> StdResult<Decimal> {
    let mut orders: u8 = 0;
    let mut quote_value = Uint128::zero();
    let mut base_depth = Uint128::zero();

    let mut i: u8 = 0;

    while i < MAX_ITERATIONS && (quote_value < MIN_VALUE || orders < MIN_ORDERS) {
        let book_response = deps.querier.query_wasm_smart::<BookResponse>(
            pair_address,
            &QueryMsg::Book {
                limit: Some(LIMIT),
                offset: Some(orders),
            },
        )?;

        let book = if side == &Side::Base {
            book_response.base
        } else {
            book_response.quote
        };

        if book.is_empty() {
            return Err(StdError::generic_err(
                "Order book is too thin to avoid price manipulation",
            ));
        }

        for order in &book {
            if order.price.is_zero() {
                return Err(StdError::generic_err(
                    "Order book contains a zero price order",
                ));
            }

            if side == &Side::Base {
                quote_value = quote_value.checked_add(order.total.mul_floor(order.price))?;
                base_depth = base_depth.checked_add(order.total)?;
            } else {
                quote_value = quote_value.checked_add(order.total)?;
                base_depth = base_depth.checked_add(order.total.div_ceil(order.price))?;
            }

            orders += 1;

            if orders >= MIN_ORDERS && quote_value >= MIN_VALUE {
                break;
            }
        }

        if book.len() < LIMIT as usize {
            break;
        }

        i += 1;
    }

    if orders < MIN_ORDERS || quote_value < MIN_VALUE || base_depth.is_zero() {
        return Err(StdError::generic_err(
            "Order book is too thin to avoid price manipulation",
        ));
    }

    let pair = deps
        .querier
        .query_wasm_smart::<ConfigResponse>(pair_address, &QueryMsg::Config {})?;

    let vwap = Decimal::from_ratio(quote_value, base_depth);

    let price = if side == &Side::Base {
        pair.tick.truncate_ceil(&vwap)
    } else {
        pair.tick.truncate_floor(&vwap)
    };

    Ok(price)
}

pub fn get_mid_price(deps: Deps, address: &Addr) -> StdResult<Decimal> {
    let quote_price = get_side_price(deps, address, &Side::Quote)?;
    let base_price = get_side_price(deps, address, &Side::Base)?;

    Ok((quote_price + base_price) / Decimal::from_ratio(2u128, 1u128))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    use cosmwasm_std::{
        from_json, testing::mock_dependencies, to_json_binary, ContractResult, SystemResult,
        WasmQuery,
    };
    use rujira_rs::fin::{BookItemResponse, BookResponse, Denoms, Tick};

    #[test]
    fn test_get_price_with_empty_book_fails() {
        let mut deps = mock_dependencies();

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&BookResponse {
                    base: vec![],
                    quote: vec![],
                })
                .unwrap(),
            ))
        });

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Base
            )
            .unwrap_err()
            .to_string(),
            "Generic error: Order book is too thin to avoid price manipulation"
        );

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Quote
            )
            .unwrap_err()
            .to_string(),
            "Generic error: Order book is too thin to avoid price manipulation"
        );
    }

    #[test]
    fn test_get_price_with_insufficient_depth_fails() {
        let mut deps = mock_dependencies();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Book { limit, .. } => to_json_binary(&BookResponse {
                        base: vec![
                            BookItemResponse {
                                price: Decimal::one(),
                                total: Uint128::new(1_000),
                            };
                            limit.unwrap() as usize
                        ],
                        quote: vec![
                            BookItemResponse {
                                price: Decimal::one(),
                                total: Uint128::new(1_000),
                            };
                            limit.unwrap() as usize
                        ],
                    })
                    .unwrap(),
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            }))
        });

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Base
            )
            .unwrap_err()
            .to_string(),
            "Generic error: Order book is too thin to avoid price manipulation"
        );

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Quote
            )
            .unwrap_err()
            .to_string(),
            "Generic error: Order book is too thin to avoid price manipulation"
        );
    }

    #[test]
    fn test_get_price_with_insufficient_orders_fails() {
        let mut deps = mock_dependencies();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::one(),
                            total: Uint128::new(100_000_000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::one(),
                            total: Uint128::new(100_000_000),
                        }],
                    })
                    .unwrap(),
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            }))
        });

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Base
            )
            .unwrap_err()
            .to_string(),
            "Generic error: Order book is too thin to avoid price manipulation"
        );

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Quote
            )
            .unwrap_err()
            .to_string(),
            "Generic error: Order book is too thin to avoid price manipulation"
        );
    }

    #[test]
    fn test_get_price_with_sufficient_immediate_depth_succeeds() {
        let mut deps = mock_dependencies();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Book { limit, .. } => to_json_binary(&BookResponse {
                        base: vec![
                            BookItemResponse {
                                price: Decimal::one(),
                                total: Uint128::new(30_000_000),
                            };
                            limit.unwrap() as usize
                        ],
                        quote: vec![
                            BookItemResponse {
                                price: Decimal::one(),
                                total: Uint128::new(30_000_000),
                            };
                            limit.unwrap() as usize
                        ],
                    })
                    .unwrap(),
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new("rune", "x/ruji"),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(6),
                        fee_taker: Decimal::percent(1),
                        fee_maker: Decimal::percent(1),
                        fee_address: "feetaker".to_string(),
                    })
                    .unwrap(),
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            }))
        });

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Base,
            )
            .unwrap(),
            Decimal::one()
        );

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Quote,
            )
            .unwrap(),
            Decimal::one()
        );
    }

    #[test]
    fn test_get_price_with_sufficient_eventual_depth_succeeds() {
        let mut deps = mock_dependencies();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Book { limit, .. } => to_json_binary(&BookResponse {
                        base: vec![
                            BookItemResponse {
                                price: Decimal::one(),
                                total: Uint128::new(15_000_000),
                            };
                            limit.unwrap() as usize
                        ],
                        quote: vec![
                            BookItemResponse {
                                price: Decimal::one(),
                                total: Uint128::new(15_000_000),
                            };
                            limit.unwrap() as usize
                        ],
                    })
                    .unwrap(),
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new("rune", "x/ruji"),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(6),
                        fee_taker: Decimal::percent(1),
                        fee_maker: Decimal::percent(1),
                        fee_address: "feetaker".to_string(),
                    })
                    .unwrap(),
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            }))
        });

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Base,
            )
            .unwrap(),
            Decimal::one()
        );

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Quote,
            )
            .unwrap(),
            Decimal::one()
        );
    }

    #[test]
    fn test_get_price_with_significant_price_value_succeeds() {
        let mut deps = mock_dependencies();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Book { limit, .. } => to_json_binary(&BookResponse {
                        base: vec![
                            BookItemResponse {
                                price: Decimal::from_str("100000").unwrap(),
                                total: Uint128::new(300),
                            };
                            limit.unwrap() as usize
                        ],
                        quote: vec![
                            BookItemResponse {
                                price: Decimal::from_str("100000").unwrap(),
                                total: Uint128::new(30_000_000),
                            };
                            limit.unwrap() as usize
                        ],
                    })
                    .unwrap(),
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new("rune", "x/ruji"),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(6),
                        fee_taker: Decimal::percent(1),
                        fee_maker: Decimal::percent(1),
                        fee_address: "feetaker".to_string(),
                    })
                    .unwrap(),
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            }))
        });

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Base,
            )
            .unwrap(),
            Decimal::from_str("100000").unwrap()
        );

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Quote,
            )
            .unwrap(),
            Decimal::from_str("100000").unwrap()
        );
    }

    #[test]
    fn test_correctly_calculates_vwap() {
        let mut deps = mock_dependencies();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![
                            BookItemResponse {
                                price: Decimal::from_str("4.0").unwrap(),
                                total: Uint128::new(100_000_000),
                            },
                            BookItemResponse {
                                price: Decimal::from_str("4.0").unwrap(),
                                total: Uint128::new(100_000_000),
                            },
                            BookItemResponse {
                                price: Decimal::from_str("3.0").unwrap(),
                                total: Uint128::new(100_000_000),
                            },
                            BookItemResponse {
                                price: Decimal::from_str("3.0").unwrap(),
                                total: Uint128::new(100_000_000),
                            },
                        ],
                        quote: vec![
                            BookItemResponse {
                                price: Decimal::from_str("2.0").unwrap(),
                                total: Uint128::new(100_000_000),
                            },
                            BookItemResponse {
                                price: Decimal::from_str("2.0").unwrap(),
                                total: Uint128::new(100_000_000),
                            },
                            BookItemResponse {
                                price: Decimal::from_str("1.0").unwrap(),
                                total: Uint128::new(100_000_000),
                            },
                            BookItemResponse {
                                price: Decimal::from_str("1.0").unwrap(),
                                total: Uint128::new(100_000_000),
                            },
                        ],
                    })
                    .unwrap(),
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new("rune", "x/ruji"),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(10),
                        fee_taker: Decimal::percent(1),
                        fee_maker: Decimal::percent(1),
                        fee_address: "feetaker".to_string(),
                    })
                    .unwrap(),
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            }))
        });

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Base,
            )
            .unwrap(),
            Decimal::from_str("3.5").unwrap()
        );

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Quote,
            )
            .unwrap(),
            Decimal::from_str("1.333333333").unwrap()
        );
    }
}
