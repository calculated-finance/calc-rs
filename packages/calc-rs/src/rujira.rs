use cosmwasm_std::{Addr, Decimal, Deps, StdError, StdResult, Uint128};
use rujira_rs::fin::{BookItemResponse, BookResponse, ConfigResponse, QueryMsg, Side};

const ORDERS_THRESHOLD: usize = 12;
const MIN_ORDERS: usize = 4;
const DEPTH_THRESHOLD: Uint128 = Uint128::new(10u128.pow(8)); // ~$1 USDC
const MIN_DEPTH: Uint128 = Uint128::new(10u128.pow(3)); // ~$1 BTC
const MAX_ITERATIONS: u8 = 2;

pub fn get_side_price(deps: Deps, pair_address: &Addr, side: &Side) -> StdResult<Decimal> {
    let mut depth = Uint128::zero();
    let mut book: Vec<BookItemResponse> = Vec::with_capacity(12);
    let mut i: u8 = 1;

    while i <= MAX_ITERATIONS && book.len() < ORDERS_THRESHOLD && depth < DEPTH_THRESHOLD {
        let book_response = deps.querier.query_wasm_smart::<BookResponse>(
            pair_address,
            &QueryMsg::Book {
                limit: Some(i * 4),
                offset: Some(book.len() as u8),
            },
        )?;

        let mut orders = if side == &Side::Base {
            book_response.base
        } else {
            book_response.quote
        };

        if orders.is_empty() && book.len() < MIN_ORDERS {
            return Err(StdError::generic_err(
                "Order book is too thin to avoid price manipulation",
            ));
        }

        depth += orders.iter().map(|order| order.total).sum::<Uint128>();

        book.append(&mut orders);
        i += 1;
    }

    if book.len() < MIN_ORDERS || depth < MIN_DEPTH {
        return Err(StdError::generic_err(
            "Order book is too thin to avoid price manipulation",
        ));
    }

    let value = book.iter().fold(Uint128::zero(), |acc, order| {
        acc + order.total.mul_floor(order.price)
    });

    let pair = deps
        .querier
        .query_wasm_smart::<ConfigResponse>(pair_address, &QueryMsg::Config {})?;

    let price = if side == &Side::Base {
        pair.tick.truncate_ceil(&Decimal::from_ratio(value, depth))
    } else {
        pair.tick.truncate_floor(&Decimal::from_ratio(value, depth))
    };

    Ok(price)
}

pub fn get_mid_price(deps: Deps, address: &Addr) -> StdResult<Decimal> {
    let quote_price = get_side_price(deps, address, &Side::Quote)?;
    let base_price = get_side_price(deps, address, &Side::Base)?;

    if quote_price.is_zero() || base_price.is_zero() {
        return Err(StdError::generic_err(
            "Order book is too thin to avoid price manipulation",
        ));
    }

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
    use rujira_rs::fin::{BookResponse, Denoms, Tick};

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
    fn get_price_with_insufficient_depth_fails() {
        let mut deps = mock_dependencies();

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&BookResponse {
                    base: vec![
                        BookItemResponse {
                            price: Decimal::one(),
                            total: Uint128::new(49),
                        };
                        10
                    ],
                    quote: vec![
                        BookItemResponse {
                            price: Decimal::one(),
                            total: Uint128::new(49),
                        };
                        10
                    ],
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
    fn get_price_with_sufficient_immediate_depth_succeeds() {
        let mut deps = mock_dependencies();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Book { limit, .. } => to_json_binary(&BookResponse {
                        base: vec![
                            BookItemResponse {
                                price: Decimal::one(),
                                total: Uint128::new(500_000_000),
                            };
                            limit.unwrap() as usize
                        ],
                        quote: vec![
                            BookItemResponse {
                                price: Decimal::one(),
                                total: Uint128::new(500_000_000),
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
    fn get_price_with_sufficient_eventual_depth_succeeds() {
        let mut deps = mock_dependencies();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Book { limit, .. } => to_json_binary(&BookResponse {
                        base: vec![
                            BookItemResponse {
                                price: Decimal::one(),
                                total: Uint128::new(60_000),
                            };
                            limit.unwrap() as usize
                        ],
                        quote: vec![
                            BookItemResponse {
                                price: Decimal::one(),
                                total: Uint128::new(60_000),
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
    fn test_correctly_calculates_vwap() {
        let mut deps = mock_dependencies();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![
                            BookItemResponse {
                                price: Decimal::from_str("4.0").unwrap(),
                                total: Uint128::new(799_999_999),
                            },
                            BookItemResponse {
                                price: Decimal::from_str("4.0").unwrap(),
                                total: Uint128::new(799_999_999),
                            },
                            BookItemResponse {
                                price: Decimal::from_str("3.0").unwrap(),
                                total: Uint128::new(200_000_001),
                            },
                            BookItemResponse {
                                price: Decimal::from_str("3.0").unwrap(),
                                total: Uint128::new(200_000_001),
                            },
                        ],
                        quote: vec![
                            BookItemResponse {
                                price: Decimal::from_str("2.0").unwrap(),
                                total: Uint128::new(200_000_001),
                            },
                            BookItemResponse {
                                price: Decimal::from_str("2.0").unwrap(),
                                total: Uint128::new(200_000_001),
                            },
                            BookItemResponse {
                                price: Decimal::from_str("1.0").unwrap(),
                                total: Uint128::new(799_999_999),
                            },
                            BookItemResponse {
                                price: Decimal::from_str("1.0").unwrap(),
                                total: Uint128::new(799_999_999),
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
            Decimal::from_str("3.799999999").unwrap()
        );

        assert_eq!(
            get_side_price(
                deps.as_ref(),
                &Addr::unchecked("rujira-fin:pair"),
                &Side::Quote,
            )
            .unwrap(),
            Decimal::from_str("1.200000001").unwrap()
        );
    }
}
