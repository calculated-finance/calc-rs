use anybuf::Anybuf;
use calc_rs::types::{ContractError, ContractResult, ExpectedReturnAmount};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, AnyMsg, CanonicalAddr, Coin, CosmosMsg, Decimal, Deps, Env, Response, StdError,
    StdResult, Uint128,
};
#[cfg(test)]
use rujira_rs::proto::types::QueryPoolResponse;
use rujira_rs::{query::Pool, Asset, Layer1Asset, NativeAsset, SecuredAsset};

use crate::types::Exchange;

#[cw_serde]
pub struct ThorExchange {}

impl ThorExchange {
    pub fn new() -> Self {
        ThorExchange {}
    }
}

struct MsgDeposit {
    pub memo: String,
    pub coins: Vec<Coin>,
    pub signer: CanonicalAddr,
}

impl From<MsgDeposit> for CosmosMsg {
    fn from(value: MsgDeposit) -> Self {
        let coins: Vec<Anybuf> = value
            .coins
            .iter()
            .map(|c| {
                let asset = layer_1_asset(&NativeAsset::new(&c.denom))
                    .unwrap()
                    .denom_string()
                    .to_ascii_uppercase();
                let (chain, symbol) = asset.split_once('.').unwrap();

                Anybuf::new()
                    .append_message(
                        1,
                        &Anybuf::new()
                            .append_string(1, chain)
                            .append_string(2, symbol)
                            .append_string(3, symbol)
                            .append_bool(4, false)
                            .append_bool(5, false)
                            .append_bool(6, c.denom.to_lowercase() != "rune"),
                    )
                    .append_string(2, c.amount.to_string())
            })
            .collect();

        let value = Anybuf::new()
            .append_repeated_message(1, &coins)
            .append_string(2, value.memo)
            .append_bytes(3, value.signer.to_vec());

        CosmosMsg::Any(AnyMsg {
            type_url: "/types.MsgDeposit".to_string(),
            value: value.as_bytes().into(),
        })
    }
}

pub fn layer_1_asset(denom: &NativeAsset) -> StdResult<Layer1Asset> {
    let denom_string = denom.denom_string();

    if denom_string.contains("rune") {
        return Ok(Layer1Asset::new("THOR", "RUNE"));
    }

    let (chain, symbol) = denom_string
        .split_once('-')
        .ok_or_else(|| StdError::generic_err(format!("Invalid layer 1 asset: {}", denom)))?;

    Ok(Layer1Asset::new(
        &chain.to_ascii_uppercase(),
        &symbol.to_ascii_uppercase(),
    ))
}

fn secured_asset(asset: &Layer1Asset) -> StdResult<SecuredAsset> {
    match asset.denom_string().to_uppercase().split_once(".") {
        Some((chain, symbol)) => Ok(SecuredAsset::new(chain, symbol)),
        None => Err(StdError::generic_err(format!(
            "Invalid layer 1 asset: {}",
            asset.denom_string()
        ))),
    }
}

fn load_pool(deps: Deps, asset: &Layer1Asset) -> StdResult<Pool> {
    Ok(Pool::load(deps.querier, asset).map_err(|_| {
        StdError::generic_err(format!(
            "Failed to load pool for asset {}",
            asset.denom_string(),
        ))
    })?)
}

fn get_pools(
    deps: Deps,
    swap_denom: &NativeAsset,
    target_denom: &NativeAsset,
) -> Result<Vec<Pool>, StdError> {
    Ok([swap_denom, target_denom]
        .iter()
        .filter(|&&denom| !denom.denom_string().contains("rune"))
        .map(|&denom| load_pool(deps, &layer_1_asset(denom)?))
        .collect::<StdResult<Vec<Pool>>>()?)
}

fn get_expected_receive_amount(
    pool: &Pool,
    swap_asset: &Layer1Asset,
    swap_amount: &Uint128,
) -> StdResult<(Layer1Asset, Uint128)> {
    let receive_asset = match swap_asset.denom_string().as_str() {
        "thor.rune" => match pool.asset.clone() {
            Asset::Layer1(asset) => asset,
            _ => return Err(StdError::generic_err("Pool asset is not a Layer 1 asset")),
        },
        _ => Layer1Asset::new("THOR", "RUNE"),
    };

    let receive_amount = swap_amount
        .checked_mul(pool.balance_asset)?
        .checked_mul(pool.balance_rune)?
        .checked_div(
            swap_amount
                .checked_add(match swap_asset.denom_string().as_str() {
                    "thor.rune" => pool.balance_rune,
                    _ => pool.balance_asset,
                })?
                .pow(2),
        )?;

    Ok((receive_asset, receive_amount))
}

fn get_spot_price(pool: &Pool, swap_asset: &Layer1Asset) -> StdResult<(Layer1Asset, Decimal)> {
    let pool_asset = match pool.asset.clone() {
        Asset::Layer1(asset) => asset,
        _ => return Err(StdError::generic_err("Pool asset is not a Layer 1 asset")),
    };

    let pool_asset_price = Decimal::from_ratio(pool.balance_rune, pool.balance_asset);

    match swap_asset.denom_string().as_str() {
        "thor.rune" => Ok((pool_asset, pool_asset_price)),
        _ => Ok((
            Layer1Asset::new("THOR", "RUNE"),
            Decimal::one() / (pool_asset_price),
        )),
    }
}

impl Exchange for ThorExchange {
    fn can_swap(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
    ) -> StdResult<bool> {
        let expected_return_amount = self
            .expected_receive_amount(
                deps,
                swap_amount,
                &NativeAsset::new(&minimum_receive_amount.denom),
            )
            .unwrap_or(ExpectedReturnAmount {
                return_amount: Coin {
                    denom: minimum_receive_amount.denom.clone(),
                    amount: Uint128::zero(),
                },
                slippage: Decimal::zero(),
            });

        Ok(expected_return_amount.return_amount.amount >= minimum_receive_amount.amount)
    }

    fn route(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
    ) -> StdResult<Vec<Coin>> {
        let pools = get_pools(deps, &NativeAsset::new(&swap_amount.denom), target_denom)?;

        if pools.is_empty() {
            return Err(StdError::generic_err("No valid route found"));
        }

        let mut route = vec![swap_amount.clone()];

        for (i, pool) in pools.iter().enumerate() {
            let (out_asset, out_amount) = get_expected_receive_amount(
                pool,
                &layer_1_asset(&NativeAsset::new(&route[i].denom))?,
                &route[i].amount,
            )?;

            if out_amount.is_zero() {
                return Err(StdError::generic_err("Received zero amount from pool"));
            }

            route.push(Coin {
                denom: if out_asset.is_rune() {
                    "rune".to_string()
                } else {
                    secured_asset(&out_asset)?.denom_string()
                },
                amount: out_amount,
            });
        }

        Ok(route)
    }

    fn expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
    ) -> StdResult<ExpectedReturnAmount> {
        let swap_asset = NativeAsset::new(&swap_amount.denom);

        let pools = get_pools(deps, &swap_asset, target_denom)?;

        if pools.is_empty() {
            return Err(StdError::generic_err("No valid route found"));
        }

        let (_, out_amount) = pools.iter().fold(
            (layer_1_asset(&swap_asset)?, swap_amount.amount),
            |(in_asset, in_amount), pool| {
                get_expected_receive_amount(pool, &in_asset, &in_amount).expect(
                    format!(
                        "Failed to get expected receive amount for swapping {} {} in {} pool",
                        in_amount,
                        in_asset.denom_string(),
                        pool.asset
                    )
                    .as_str(),
                )
            },
        );

        let spot_price = self.spot_price(deps, &swap_asset, target_denom)?;

        let optimal_return_amount = swap_amount.amount.mul_floor(Decimal::one() / spot_price);

        let slippage =
            Decimal::one().checked_sub(Decimal::from_ratio(out_amount, optimal_return_amount))?;

        Ok(ExpectedReturnAmount {
            return_amount: Coin {
                denom: target_denom.denom_string(),
                amount: out_amount,
            },
            slippage,
        })
    }

    fn spot_price(
        &self,
        deps: Deps,
        swap_denom: &NativeAsset,
        target_denom: &NativeAsset,
    ) -> StdResult<Decimal> {
        let pools = get_pools(deps, swap_denom, target_denom)?;

        if pools.is_empty() {
            return Err(StdError::generic_err("No valid route found"));
        }

        let (_, price) = pools.iter().fold(
            (layer_1_asset(swap_denom)?, Decimal::one()),
            |(asset, out_price), pool| {
                get_spot_price(pool, &asset)
                    .map(|(asset, price)| (asset, out_price * price))
                    .expect(&format!(
                        "Failed to get spot price for swapping {} in {} pool",
                        asset.denom_string(),
                        pool.asset
                    ))
            },
        );

        Ok(price)
    }

    fn swap(
        &self,
        deps: Deps,
        env: Env,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        recipient: Addr,
    ) -> ContractResult {
        if !self.can_swap(deps, swap_amount, minimum_receive_amount)? {
            return Err(ContractError::Std(StdError::generic_err(format!(
                "Unable to swap {} {} into at least {} {}",
                swap_amount.amount,
                swap_amount.denom,
                minimum_receive_amount.amount,
                minimum_receive_amount.denom
            ))));
        }

        let swap_asset = secured_asset(&layer_1_asset(&NativeAsset::new(&swap_amount.denom))?)?;
        let receive_asset = secured_asset(&layer_1_asset(&NativeAsset::new(
            &minimum_receive_amount.denom,
        ))?)?;

        let memo = format!(
            "=:{}:{}:{}",
            receive_asset.denom_string().to_ascii_uppercase(),
            recipient,
            minimum_receive_amount.amount
        );

        let swap_msg = MsgDeposit {
            memo,
            coins: vec![Coin {
                denom: swap_asset.denom_string(),
                amount: swap_amount.amount,
            }],
            signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
        };

        Ok(Response::new().add_message(swap_msg))
    }
}

#[cfg(test)]
mod asset_tests {
    use super::*;

    #[test]
    fn test_layer_1_asset() {
        let denom_string = layer_1_asset(&NativeAsset::new("rune"))
            .unwrap()
            .denom_string();
        let (chain, symbol) = denom_string.split_once('.').unwrap();

        assert_eq!(chain, "thor");
        assert_eq!(symbol, "rune");

        let denom_string = layer_1_asset(&NativeAsset::new("eth-usd"))
            .unwrap()
            .denom_string();
        let (chain, symbol) = denom_string.split_once('.').unwrap();

        assert_eq!(chain, "eth");
        assert_eq!(symbol, "usd");

        let denom_string = layer_1_asset(&NativeAsset::new(
            "eth-usd-0xdac17f958d2ee523a2206206994597c13d831ec7",
        ))
        .unwrap()
        .denom_string();
        let (chain, symbol) = denom_string.split_once('.').unwrap();

        assert_eq!(chain, "eth");
        assert_eq!(symbol, "usd-0xdac17f958d2ee523a2206206994597c13d831ec7");

        let err = layer_1_asset(&NativeAsset::new("uruji")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Generic error: Invalid layer 1 asset: RUNE.uruji"
        );
    }

    #[test]
    fn test_secured_asset() {
        let secured = secured_asset(&layer_1_asset(&NativeAsset::new("eth-eth")).unwrap())
            .unwrap()
            .denom_string();
        let (chain, symbol) = secured.split_once('-').unwrap();

        assert_eq!(chain, "eth");
        assert_eq!(symbol, "eth");

        let secured = secured_asset(&layer_1_asset(&NativeAsset::new("rune")).unwrap())
            .unwrap()
            .denom_string();
        let (chain, symbol) = secured.split_once('-').unwrap();

        assert_eq!(chain, "thor");
        assert_eq!(symbol, "rune");
    }
}

#[cfg(test)]
fn default_pool_response() -> QueryPoolResponse {
    QueryPoolResponse {
        asset: "ETH.ETH".to_string(),
        status: rujira_rs::proto::types::PoolStatus::Available
            .as_str_name()
            .to_string(),
        short_code: "eth".to_string(),
        decimals: 8,
        pending_inbound_asset: "0".to_string(),
        pending_inbound_rune: "0".to_string(),
        balance_asset: "10000000000".to_string(),
        balance_rune: "10000000000".to_string(),
        asset_tor_price: "1.0".to_string(),
        pool_units: "10000000000".to_string(),
        lp_units: "10000000000".to_string(),
        synth_units: "10000000000".to_string(),
        synth_supply: "10000000000".to_string(),
        savers_depth: "10000000000".to_string(),
        savers_units: "10000000000".to_string(),
        savers_fill_bps: "0".to_string(),
        savers_capacity_remaining: "0".to_string(),
        synth_mint_paused: false,
        synth_supply_remaining: "0".to_string(),
        loan_collateral: "0".to_string(),
        loan_collateral_remaining: "0".to_string(),
        loan_cr: "0".to_string(),
        derived_depth_bps: "0".to_string(),
    }
}

#[cfg(test)]
mod pools_tests {
    use calc_rs::mock::mock_dependencies_with_custom_querier;
    use cosmwasm_std::{Binary, ContractResult, SystemResult};
    use prost::Message;
    use rujira_rs::{
        proto::types::{QueryPoolRequest, QueryPoolResponse},
        NativeAsset,
    };

    use super::*;

    #[test]
    fn fails_to_fetch_pools_for_non_l1_asset() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_asset = NativeAsset::new("uruji");
        let target_asset = NativeAsset::new("eth-usdc");

        let error = get_pools(deps.as_ref(), &swap_asset, &target_asset).unwrap_err();

        assert_eq!(
            error,
            StdError::generic_err(format!(
                "Invalid layer 1 asset: RUNE.{}",
                swap_asset.denom_string(),
            ))
        );
    }

    #[test]
    fn gets_single_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_asset = NativeAsset::new("arb-eth");
        let target_asset = NativeAsset::new("rune");

        let result = vec![Pool::try_from(QueryPoolResponse {
            asset: layer_1_asset(&swap_asset).unwrap().to_string(),
            ..default_pool_response()
        })
        .unwrap()];

        let pools = get_pools(deps.as_ref(), &swap_asset, &target_asset).unwrap();

        assert_eq!(pools, result);
    }

    #[test]
    fn gets_multiple_pools() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_asset = NativeAsset::new("arb-eth");
        let target_asset = NativeAsset::new("eth-usdc");

        let result = vec![
            Pool::try_from(QueryPoolResponse {
                asset: layer_1_asset(&swap_asset).unwrap().to_string(),
                ..default_pool_response()
            })
            .unwrap(),
            Pool::try_from(QueryPoolResponse {
                asset: layer_1_asset(&target_asset).unwrap().to_string(),
                ..default_pool_response()
            })
            .unwrap(),
        ];

        let pools = get_pools(deps.as_ref(), &swap_asset, &target_asset).unwrap();

        assert_eq!(pools, result);
    }
}

#[cfg(test)]
mod can_swap_tests {

    use calc_rs::mock::mock_dependencies_with_custom_querier;
    use cosmwasm_std::{Binary, Coin, ContractResult, SystemResult, Uint128};
    use prost::Message;
    use rujira_rs::proto::types::{QueryPoolRequest, QueryPoolResponse};

    use crate::{
        exchanges::thor::{default_pool_response, ThorExchange},
        types::Exchange,
    };

    #[test]
    fn cannot_swap_with_no_pools() {
        let deps = mock_dependencies_with_custom_querier();

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let minimum_receive_amount = Coin {
            denom: "eth-usdc".to_string().clone(),
            amount: Uint128::new(50),
        };

        assert!(!ThorExchange::new()
            .can_swap(deps.as_ref(), &swap_amount, &minimum_receive_amount)
            .unwrap());
    }

    #[test]
    fn can_swap_with_single_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let minimum_receive_amount = Coin {
            denom: "rune".to_string().clone(),
            amount: Uint128::new(50),
        };

        assert!(ThorExchange::new()
            .can_swap(deps.as_ref(), &swap_amount, &minimum_receive_amount)
            .unwrap());
    }

    #[test]
    fn can_swap_with_multiple_pools() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let minimum_receive_amount = Coin {
            denom: "eth-usdc".to_string().clone(),
            amount: Uint128::new(50),
        };

        assert!(ThorExchange::new()
            .can_swap(deps.as_ref(), &swap_amount, &minimum_receive_amount)
            .unwrap());
    }
}

#[cfg(test)]
mod route_tests {
    use calc_rs::mock::mock_dependencies_with_custom_querier;
    use cosmwasm_std::{
        Binary, Coin, ContractResult, StdError, SystemError, SystemResult, Uint128,
    };
    use prost::Message;
    use rujira_rs::{
        proto::types::{QueryPoolRequest, QueryPoolResponse},
        NativeAsset,
    };

    use crate::{
        exchanges::thor::{default_pool_response, layer_1_asset, ThorExchange},
        types::Exchange,
    };

    #[test]
    fn fails_to_get_route_with_no_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(|query| {
            SystemResult::Err(SystemError::InvalidRequest {
                error: "No such pool".to_string(),
                request: query.data.clone(),
            })
        });

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("eth-usdc");

        assert_eq!(
            ThorExchange::new()
                .route(deps.as_ref(), &swap_amount, &target_denom)
                .unwrap_err(),
            StdError::generic_err(format!(
                "Failed to load pool for asset {}",
                layer_1_asset(&NativeAsset::new(&swap_amount.denom))
                    .unwrap()
                    .denom_string()
            ))
        );
    }

    #[test]
    fn gets_route_with_single_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("rune");

        assert_eq!(
            ThorExchange::new()
                .route(deps.as_ref(), &swap_amount, &target_denom)
                .unwrap(),
            vec![
                swap_amount,
                Coin {
                    denom: target_denom.denom_string(),
                    amount: Uint128::new(99)
                }
            ]
        );

        let swap_amount = Coin {
            denom: "rune".to_string().clone(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("arb-eth");

        assert_eq!(
            ThorExchange::new()
                .route(deps.as_ref(), &swap_amount, &target_denom)
                .unwrap(),
            vec![
                swap_amount,
                Coin {
                    denom: target_denom.denom_string(),
                    amount: Uint128::new(99)
                }
            ]
        );
    }

    #[test]
    fn gets_route_with_multiple_pools() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("eth-usdc");

        assert_eq!(
            ThorExchange::new()
                .route(deps.as_ref(), &swap_amount, &target_denom)
                .unwrap(),
            vec![
                swap_amount,
                Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(99)
                },
                Coin {
                    denom: target_denom.denom_string(),
                    amount: Uint128::new(98)
                }
            ]
        );

        let swap_amount = Coin {
            denom: "eth-usdc".to_string().clone(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("arb-eth");

        assert_eq!(
            ThorExchange::new()
                .route(deps.as_ref(), &swap_amount, &target_denom)
                .unwrap(),
            vec![
                swap_amount,
                Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(99)
                },
                Coin {
                    denom: target_denom.denom_string(),
                    amount: Uint128::new(98)
                }
            ]
        );
    }
}

#[cfg(test)]
mod expected_receive_amount_tests {
    use std::str::FromStr;

    use calc_rs::{mock::mock_dependencies_with_custom_querier, types::ExpectedReturnAmount};
    use cosmwasm_std::{
        Binary, Coin, ContractResult, Decimal, StdError, SystemError, SystemResult, Uint128,
    };
    use prost::Message;
    use rujira_rs::{
        proto::types::{QueryPoolRequest, QueryPoolResponse},
        NativeAsset,
    };

    use crate::{
        exchanges::thor::{default_pool_response, layer_1_asset, ThorExchange},
        types::Exchange,
    };

    #[test]
    fn fails_to_get_expected_receive_amount_with_no_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(|query| {
            SystemResult::Err(SystemError::InvalidRequest {
                error: "No such pool".to_string(),
                request: query.data.clone(),
            })
        });

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("eth-usdc");

        assert_eq!(
            ThorExchange::new()
                .route(deps.as_ref(), &swap_amount, &target_denom)
                .unwrap_err(),
            StdError::generic_err(format!(
                "Failed to load pool for asset {}",
                layer_1_asset(&NativeAsset::new(&swap_amount.denom))
                    .unwrap()
                    .denom_string()
            ))
        );
    }

    #[test]
    fn gets_expected_receive_amount_with_single_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("rune");

        assert_eq!(
            ThorExchange::new()
                .expected_receive_amount(deps.as_ref(), &swap_amount, &target_denom)
                .unwrap(),
            ExpectedReturnAmount {
                return_amount: Coin {
                    denom: target_denom.denom_string(),
                    amount: Uint128::new(99)
                },
                slippage: Decimal::from_str("0.01").unwrap()
            }
        );

        let swap_amount = Coin {
            denom: "rune".to_string().clone(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("arb-eth");

        assert_eq!(
            ThorExchange::new()
                .expected_receive_amount(deps.as_ref(), &swap_amount, &target_denom)
                .unwrap(),
            ExpectedReturnAmount {
                return_amount: Coin {
                    denom: target_denom.denom_string(),
                    amount: Uint128::new(99)
                },
                slippage: Decimal::from_str("0.01").unwrap()
            }
        );
    }

    #[test]
    fn gets_expected_receive_amount_with_multiple_pools() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("eth-usdc");

        assert_eq!(
            ThorExchange::new()
                .expected_receive_amount(deps.as_ref(), &swap_amount, &target_denom)
                .unwrap(),
            ExpectedReturnAmount {
                return_amount: Coin {
                    denom: target_denom.denom_string(),
                    amount: Uint128::new(98)
                },
                slippage: Decimal::from_str("0.02").unwrap()
            }
        );

        let swap_amount = Coin {
            denom: "eth-usdc".to_string().clone(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("arb-eth");

        assert_eq!(
            ThorExchange::new()
                .expected_receive_amount(deps.as_ref(), &swap_amount, &target_denom)
                .unwrap(),
            ExpectedReturnAmount {
                return_amount: Coin {
                    denom: target_denom.denom_string(),
                    amount: Uint128::new(98)
                },
                slippage: Decimal::from_str("0.02").unwrap()
            }
        );
    }
}

#[cfg(test)]
mod spot_price_tests {

    use std::str::FromStr;

    use calc_rs::mock::mock_dependencies_with_custom_querier;
    use cosmwasm_std::{Binary, ContractResult, Decimal, StdError, SystemError, SystemResult};
    use prost::Message;
    use rujira_rs::{
        proto::types::{QueryPoolRequest, QueryPoolResponse},
        NativeAsset,
    };

    use crate::{
        exchanges::thor::{default_pool_response, layer_1_asset, ThorExchange},
        types::Exchange,
    };

    #[test]
    fn fails_to_get_spot_price_with_no_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(|query| {
            SystemResult::Err(SystemError::InvalidRequest {
                error: "No such pool".to_string(),
                request: query.data.clone(),
            })
        });

        let swap_asset = NativeAsset::new("arb-eth");
        let target_denom = NativeAsset::new("eth-usdc");

        assert_eq!(
            ThorExchange::new()
                .spot_price(deps.as_ref(), &swap_asset, &target_denom)
                .unwrap_err(),
            StdError::generic_err(format!(
                "Failed to load pool for asset {}",
                layer_1_asset(&swap_asset).unwrap().denom_string()
            ))
        );
    }

    #[test]
    fn gets_spot_price_with_single_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                balance_asset: "100".to_string(),
                balance_rune: "500".to_string(),
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_asset = NativeAsset::new("arb-eth");
        let target_denom = NativeAsset::new("rune");

        assert_eq!(
            ThorExchange::new()
                .spot_price(deps.as_ref(), &swap_asset, &target_denom)
                .unwrap(),
            Decimal::from_str("0.2").unwrap()
        );

        let swap_asset = NativeAsset::new("rune");
        let target_denom = NativeAsset::new("arb-eth");

        assert_eq!(
            ThorExchange::new()
                .spot_price(deps.as_ref(), &swap_asset, &target_denom)
                .unwrap(),
            Decimal::from_str("5").unwrap()
        );
    }

    #[test]
    fn gets_spot_price_with_multiple_pools() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                balance_asset: "100".to_string(),
                balance_rune: "500".to_string(),
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_asset = NativeAsset::new("arb-eth");
        let target_denom = NativeAsset::new("eth-usdc");

        assert_eq!(
            ThorExchange::new()
                .spot_price(deps.as_ref(), &swap_asset, &target_denom)
                .unwrap(),
            Decimal::from_str("1").unwrap()
        );

        let swap_asset = NativeAsset::new("arb-eth");
        let target_denom = NativeAsset::new("eth-usdc");

        assert_eq!(
            ThorExchange::new()
                .spot_price(deps.as_ref(), &swap_asset, &target_denom)
                .unwrap(),
            Decimal::from_str("1").unwrap()
        );
    }
}

#[cfg(test)]
mod swap_tests {

    use calc_rs::{mock::mock_dependencies_with_custom_querier, types::ContractError};
    use cosmwasm_std::{
        testing::mock_env, Addr, Api, Binary, Coin, ContractResult, Response, StdError,
        SystemError, SystemResult, Uint128,
    };
    use prost::Message;
    use rujira_rs::{
        proto::types::{QueryPoolRequest, QueryPoolResponse},
        NativeAsset,
    };

    use crate::{
        exchanges::thor::{
            default_pool_response, layer_1_asset, secured_asset, MsgDeposit, ThorExchange,
        },
        types::Exchange,
    };

    #[test]
    fn fails_to_swap_with_no_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        deps.querier.with_grpc_handler(|query| {
            SystemResult::Err(SystemError::InvalidRequest {
                error: "No such pool".to_string(),
                request: query.data.clone(),
            })
        });

        let swap_amount = Coin {
            denom: "arb-eth".to_string(),
            amount: Uint128::new(100),
        };

        let minimum_receive_amount = Coin {
            denom: "eth-usdc".to_string(),
            amount: Uint128::new(50),
        };

        assert_eq!(
            ThorExchange::new()
                .swap(
                    deps.as_ref(),
                    mock_env(),
                    &swap_amount,
                    &minimum_receive_amount,
                    Addr::unchecked("recipient")
                )
                .unwrap_err(),
            ContractError::Std(StdError::generic_err(format!(
                "Unable to swap {} {} into at least {} {}",
                swap_amount.amount,
                swap_amount.denom,
                minimum_receive_amount.amount,
                minimum_receive_amount.denom
            )))
        );
    }

    #[test]
    fn swaps_with_single_pool() {
        let mut deps = mock_dependencies_with_custom_querier();
        let env = mock_env();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let minimum_receive_amount = Coin {
            denom: "rune".to_string(),
            amount: Uint128::new(50),
        };

        let recipient = Addr::unchecked("recipient");

        assert_eq!(
            ThorExchange::new()
                .swap(
                    deps.as_ref(),
                    env.clone(),
                    &swap_amount,
                    &minimum_receive_amount,
                    recipient.clone()
                )
                .unwrap(),
            Response::default().add_message(MsgDeposit {
                memo: format!(
                    "=:{}:{}:{}",
                    secured_asset(
                        &layer_1_asset(&NativeAsset::new(&minimum_receive_amount.denom)).unwrap()
                    )
                    .unwrap()
                    .denom_string()
                    .to_ascii_uppercase()
                    .to_string(),
                    recipient.to_string(),
                    minimum_receive_amount.amount
                )
                .to_string(),
                coins: vec![Coin {
                    denom: swap_amount.denom.to_string(),
                    amount: swap_amount.amount,
                }],
                signer: deps
                    .api
                    .addr_canonicalize(env.contract.address.as_str())
                    .unwrap(),
            })
        );
    }

    #[test]
    fn swaps_with_multiple_pools() {
        let mut deps = mock_dependencies_with_custom_querier();
        let env = mock_env();

        deps.querier.with_grpc_handler(move |query| {
            let pool_query = QueryPoolRequest::decode(query.data.as_slice()).unwrap();

            let mut buf = Vec::new();

            QueryPoolResponse {
                asset: pool_query.asset,
                ..default_pool_response()
            }
            .encode(&mut buf)
            .unwrap();

            SystemResult::Ok(ContractResult::Ok(Binary::from(buf)))
        });

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let minimum_receive_amount = Coin {
            denom: "eth-usdc".to_string(),
            amount: Uint128::new(50),
        };

        let recipient = Addr::unchecked("recipient");

        assert_eq!(
            ThorExchange::new()
                .swap(
                    deps.as_ref(),
                    env.clone(),
                    &swap_amount,
                    &minimum_receive_amount,
                    recipient.clone()
                )
                .unwrap(),
            Response::default().add_message(MsgDeposit {
                memo: format!(
                    "=:{}:{}:{}",
                    secured_asset(
                        &layer_1_asset(&NativeAsset::new(&minimum_receive_amount.denom)).unwrap()
                    )
                    .unwrap()
                    .denom_string()
                    .to_ascii_uppercase()
                    .to_string(),
                    recipient.to_string(),
                    minimum_receive_amount.amount
                )
                .to_string(),
                coins: vec![Coin {
                    denom: swap_amount.denom.to_string(),
                    amount: swap_amount.amount,
                }],
                signer: deps
                    .api
                    .addr_canonicalize(env.contract.address.as_str())
                    .unwrap(),
            })
        );
    }
}
