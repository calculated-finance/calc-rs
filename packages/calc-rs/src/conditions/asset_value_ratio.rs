use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Deps, Env, StdError, StdResult};
use rujira_rs::{
    fin::{BookResponse, ConfigResponse, QueryMsg},
    query::Pool,
    Layer1Asset,
};

#[cw_serde]
pub enum PriceSource {
    Fin { address: Addr },
    Thorchain,
}

#[cw_serde]
pub struct AssetValueRatio {
    pub numerator: String,
    pub denominator: String,
    pub ratio: Decimal,
    pub tolerance: Decimal,
    pub oracle: PriceSource,
}

impl AssetValueRatio {
    pub fn validate(&self, deps: Deps) -> StdResult<()> {
        match self.oracle {
            PriceSource::Fin { ref address } => {
                let pair = deps
                    .querier
                    .query_wasm_smart::<ConfigResponse>(address.clone(), &QueryMsg::Config {})?;

                let denoms = [pair.denoms.base(), pair.denoms.quote()];

                if !denoms.contains(&self.numerator.as_str()) {
                    return Err(StdError::generic_err(format!(
                        "Pair at {} does not include asset {}",
                        address, self.numerator
                    )));
                }

                if !denoms.contains(&self.denominator.as_str()) {
                    return Err(StdError::generic_err(format!(
                        "Pair at {} does not include asset {}",
                        address, self.denominator
                    )));
                }
            }
            PriceSource::Thorchain => {
                fetch_l1_asset_price(deps, &self.numerator)?;
                fetch_l1_asset_price(deps, &self.denominator)?;
            }
        }

        Ok(())
    }

    pub fn is_satisfied(&self, deps: Deps, env: &Env) -> StdResult<bool> {
        let numerator_balance = deps
            .querier
            .query_balance(&env.contract.address, &self.numerator)?;

        if numerator_balance.amount.is_zero() {
            return Ok(false);
        }

        let denominator_balance = deps
            .querier
            .query_balance(&env.contract.address, &self.denominator)?;

        if denominator_balance.amount.is_zero() {
            return Ok(false);
        }

        let price = match self.oracle.clone() {
            PriceSource::Fin { address } => {
                let book_response = deps.querier.query_wasm_smart::<BookResponse>(
                    &address,
                    &QueryMsg::Book {
                        limit: Some(1),
                        offset: None,
                    },
                )?;

                if book_response.base.is_empty() || book_response.quote.is_empty() {
                    return Err(StdError::generic_err("Order book is empty".to_string()));
                }

                let pair = deps
                    .querier
                    .query_wasm_smart::<ConfigResponse>(address, &QueryMsg::Config {})?;

                let mid_price = (book_response.base[0].price + book_response.quote[0].price)
                    / Decimal::from_ratio(2u128, 1u128);

                if mid_price.is_zero() {
                    return Err(StdError::generic_err("Mid price is zero".to_string()));
                }

                if pair.denoms.base() == self.numerator {
                    Decimal::one() / mid_price
                } else {
                    mid_price
                }
            }
            PriceSource::Thorchain => {
                let numerator_price = fetch_l1_asset_price(deps, &self.numerator)?;
                let denominator_price = fetch_l1_asset_price(deps, &self.denominator)?;

                numerator_price
                    .checked_div(denominator_price)
                    .map_err(|_| {
                        StdError::generic_err(format!(
                        "Failed to calculate asset value ratio: L1 oracle price for '{}' is zero",
                        self.denominator
                    ))
                    })?
            }
        };

        let balance_ratio =
            Decimal::from_ratio(numerator_balance.amount, denominator_balance.amount);

        let value_ratio = balance_ratio * price;

        Ok(value_ratio.abs_diff(self.ratio) < self.tolerance)
    }
}

fn fetch_l1_asset_price(deps: Deps, asset: &str) -> StdResult<Decimal> {
    let layer_1_asset = Layer1Asset::from_native(asset.to_string())
        .map_err(|e| StdError::generic_err(format!("'{}' is not a secured asset: {e}", asset)))?;

    Pool::load(deps.querier, &layer_1_asset)
        .map_err(|e| {
            StdError::generic_err(format!(
                "Failed to load oracle price for {layer_1_asset}, error: {e}"
            ))
        })
        .map(|pool| pool.asset_tor_price)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use calc_rs_test::{fixtures::mock_pool, mocks::mock_dependencies_with_custom_grpc_querier};
    use cosmwasm_std::{
        from_json,
        testing::{mock_dependencies, mock_env},
        to_json_binary, Addr, Coin, ContractResult, Decimal, SystemResult, Uint128, WasmQuery,
    };
    use rujira_rs::fin::{BookItemResponse, BookResponse, ConfigResponse, Denoms, QueryMsg, Tick};

    use crate::conditions::asset_value_ratio::{AssetValueRatio, PriceSource};

    #[test]
    fn test_asset_ratio_with_fin_price_source() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new(
                            "btc",  // base
                            "usdc", // quote
                        ),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(1),
                        fee_taker: Decimal::percent(1),
                        fee_maker: Decimal::percent(1),
                        fee_address: Addr::unchecked("ruji1feeaddress").to_string(),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("2.1").unwrap(),
                            total: Uint128::new(1000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.9").unwrap(),
                            total: Uint128::new(1000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!("Unexpected query type"),
                },
                _ => panic!("Unexpected query type"),
            }))
        });

        deps.querier.bank.update_balance(
            &env.contract.address,
            vec![Coin::new(0_u128, "usdc"), Coin::new(0_u128, "btc")],
        );

        assert!(!AssetValueRatio {
            numerator: "usdc".to_string(),
            denominator: "btc".to_string(),
            ratio: Decimal::from_str("2").unwrap(),
            tolerance: Decimal::percent(10),
            oracle: PriceSource::Fin {
                address: Addr::unchecked("ruji1finaddress"),
            },
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        deps.querier.bank.update_balance(
            &env.contract.address,
            vec![Coin::new(0_u128, "usdc"), Coin::new(1000_u128, "btc")],
        );

        assert!(!AssetValueRatio {
            numerator: "usdc".to_string(),
            denominator: "btc".to_string(),
            ratio: Decimal::from_str("2").unwrap(),
            tolerance: Decimal::percent(10),
            oracle: PriceSource::Fin {
                address: Addr::unchecked("ruji1finaddress"),
            },
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        deps.querier.bank.update_balance(
            &env.contract.address,
            vec![Coin::new(1000_u128, "usdc"), Coin::new(0_u128, "btc")],
        );

        assert!(!AssetValueRatio {
            numerator: "usdc".to_string(),
            denominator: "btc".to_string(),
            ratio: Decimal::from_str("2").unwrap(),
            tolerance: Decimal::percent(10),
            oracle: PriceSource::Fin {
                address: Addr::unchecked("ruji1finaddress"),
            },
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        deps.querier.bank.update_balance(
            &env.contract.address,
            vec![Coin::new(1000_u128, "usdc"), Coin::new(1000_u128, "btc")],
        );

        assert!(AssetValueRatio {
            numerator: "usdc".to_string(),
            denominator: "btc".to_string(),
            ratio: Decimal::from_str("2").unwrap(),
            tolerance: Decimal::percent(10),
            oracle: PriceSource::Fin {
                address: Addr::unchecked("ruji1finaddress"),
            },
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(!AssetValueRatio {
            numerator: "usdc".to_string(),
            denominator: "btc".to_string(),
            ratio: Decimal::from_str("2.25").unwrap(),
            tolerance: Decimal::percent(10),
            oracle: PriceSource::Fin {
                address: Addr::unchecked("ruji1finaddress"),
            },
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(AssetValueRatio {
            numerator: "btc".to_string(),
            denominator: "usdc".to_string(),
            ratio: Decimal::from_str("0.5").unwrap(),
            tolerance: Decimal::percent(10),
            oracle: PriceSource::Fin {
                address: Addr::unchecked("ruji1finaddress"),
            },
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(!AssetValueRatio {
            numerator: "btc".to_string(),
            denominator: "usdc".to_string(),
            ratio: Decimal::from_str("0.3").unwrap(),
            tolerance: Decimal::percent(10),
            oracle: PriceSource::Fin {
                address: Addr::unchecked("ruji1finaddress"),
            },
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());
    }

    #[test]
    fn test_asset_ratio_with_thorchain_price_source() {
        let mut deps = mock_dependencies_with_custom_grpc_querier();
        let env = mock_env();

        deps.querier.with_grpc_handler(|query| {
            SystemResult::Ok(ContractResult::Ok(mock_pool(query.data.clone()).unwrap()))
        });

        deps.querier.default.bank.update_balance(
            &env.contract.address,
            vec![
                Coin::new(200_000_u128, "ETH-USDC"),
                Coin::new(1_u128, "BTC-BTC"),
            ],
        );

        assert!(AssetValueRatio {
            numerator: "ETH-USDC".to_string(),
            denominator: "BTC-BTC".to_string(),
            ratio: Decimal::from_str("2").unwrap(),
            tolerance: Decimal::percent(10),
            oracle: PriceSource::Thorchain,
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(!AssetValueRatio {
            numerator: "ETH-USDC".to_string(),
            denominator: "BTC-BTC".to_string(),
            ratio: Decimal::from_str("2.25").unwrap(),
            tolerance: Decimal::percent(10),
            oracle: PriceSource::Thorchain,
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(AssetValueRatio {
            numerator: "BTC-BTC".to_string(),
            denominator: "ETH-USDC".to_string(),
            ratio: Decimal::from_str("0.5").unwrap(),
            tolerance: Decimal::percent(10),
            oracle: PriceSource::Thorchain,
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(!AssetValueRatio {
            numerator: "BTC-BTC".to_string(),
            denominator: "ETH-USDC".to_string(),
            ratio: Decimal::from_str("0.4").unwrap(),
            tolerance: Decimal::percent(10),
            oracle: PriceSource::Thorchain,
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());
    }
}
