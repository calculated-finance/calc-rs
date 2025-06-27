use calc_rs::{
    exchanger::{ExpectedReceiveAmount, Route},
    scheduler::{CreateTrigger, SchedulerExecuteMsg, TriggerConditionsThreshold},
    types::{
        layer_1_asset, secured_asset, Callback, Condition, Contract, ContractError, ContractResult,
        MsgDeposit,
    },
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, CosmosMsg, Decimal, Deps, Env, MessageInfo, Response, StdError,
    StdResult, Uint128,
};
use cw_storage_plus::Item;
#[cfg(test)]
use rujira_rs::proto::types::QueryPoolResponse;
use rujira_rs::{query::Pool, Asset, Layer1Asset, NativeAsset};

use crate::types::Exchange;

pub const SCHEDULER: Item<Addr> = Item::new("scheduler");

#[cw_serde]
pub struct ThorchainExchange {
    scheduler: Addr,
}

impl ThorchainExchange {
    pub fn new(deps: Deps) -> Self {
        ThorchainExchange {
            scheduler: SCHEDULER.load(deps.storage).unwrap(),
        }
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

impl Exchange for ThorchainExchange {
    fn can_swap(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        route: &Option<Route>,
    ) -> StdResult<bool> {
        let expected_receive_amount = self.expected_receive_amount(
            deps,
            swap_amount,
            &NativeAsset::new(&minimum_receive_amount.denom),
            route,
        )?;

        Ok(expected_receive_amount.receive_amount.amount >= minimum_receive_amount.amount)
    }

    fn path(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
        route: &Option<Route>,
    ) -> StdResult<Vec<Coin>> {
        if let Some(route) = route {
            match route {
                Route::Thorchain {} => {}
                _ => {
                    return Err(StdError::generic_err(
                        "Route type not supported for Thorchain exchange",
                    ));
                }
            }
        }

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
        route: &Option<Route>,
    ) -> StdResult<ExpectedReceiveAmount> {
        if let Some(route) = route {
            match route {
                Route::Thorchain {} => {}
                _ => {
                    return Err(StdError::generic_err(
                        "Route not supported for Thorchain exchange",
                    ));
                }
            }
        }

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

        let spot_price = self.spot_price(deps, &swap_asset, target_denom, route)?;

        let optimal_return_amount = swap_amount.amount.mul_floor(Decimal::one() / spot_price);

        let slippage_bps = Uint128::new(10_000).mul_ceil(
            Decimal::one().checked_sub(Decimal::from_ratio(out_amount, optimal_return_amount))?,
        );

        Ok(ExpectedReceiveAmount {
            receive_amount: Coin {
                denom: target_denom.denom_string(),
                amount: out_amount,
            },
            slippage_bps: slippage_bps.into(),
        })
    }

    fn spot_price(
        &self,
        deps: Deps,
        swap_denom: &NativeAsset,
        target_denom: &NativeAsset,
        route: &Option<Route>,
    ) -> StdResult<Decimal> {
        if let Some(route) = route {
            match route {
                Route::Thorchain {} => {}
                _ => {
                    return Err(StdError::generic_err(
                        "Route type not supported for Thorchain exchange",
                    ));
                }
            }
        }

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
        env: &Env,
        _info: &MessageInfo,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        route: &Option<Route>,
        recipient: Addr,
        on_complete: Option<Callback>,
    ) -> ContractResult {
        if !self.can_swap(deps, swap_amount, minimum_receive_amount, route)? {
            return Err(ContractError::Std(StdError::generic_err(format!(
                "Unable to swap {} {} into at least {} {}",
                swap_amount.amount,
                swap_amount.denom,
                minimum_receive_amount.amount,
                minimum_receive_amount.denom
            ))));
        }

        let swap_asset = if swap_amount.denom.to_ascii_lowercase().contains("rune") {
            "THOR.RUNE".to_string()
        } else {
            swap_amount.denom.to_ascii_uppercase()
        };

        let receive_asset = if minimum_receive_amount
            .denom
            .to_ascii_lowercase()
            .contains("rune")
        {
            "THOR.RUNE".to_string()
        } else {
            minimum_receive_amount.denom.to_ascii_uppercase()
        };

        let memo = format!(
            "=:{}:{}:{}:rj:10:calc-swap",
            receive_asset, recipient, minimum_receive_amount.amount
        );

        let swap_msg = CosmosMsg::from(MsgDeposit {
            memo,
            coins: vec![Coin {
                denom: swap_asset,
                amount: swap_amount.amount,
            }],
            signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
        });

        let mut messages = vec![swap_msg];

        if let Some(callback) = on_complete {
            messages.push(Contract(self.scheduler.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::CreateTrigger(CreateTrigger {
                    conditions: vec![Condition::BlocksCompleted(env.block.height + 5)],
                    threshold: TriggerConditionsThreshold::All,
                    to: callback.contract,
                    msg: callback.msg,
                }))?,
                callback.execution_rebate,
            ));
        }

        Ok(Response::default().add_messages(messages))
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
    use super::*;

    use calc_rs_test::test::mock_dependencies_with_custom_querier;
    use cosmwasm_std::{Binary, ContractResult, SystemResult};
    use prost::Message;
    use rujira_rs::{
        proto::types::{QueryPoolRequest, QueryPoolResponse},
        NativeAsset,
    };

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
    use super::*;

    use calc_rs_test::test::mock_dependencies_with_custom_querier;
    use cosmwasm_std::{Addr, Binary, Coin, ContractResult, StdError, SystemResult, Uint128};
    use prost::Message;
    use rujira_rs::{
        proto::types::{QueryPoolRequest, QueryPoolResponse},
        NativeAsset,
    };

    use crate::{
        exchanges::thorchain::{default_pool_response, ThorchainExchange, SCHEDULER},
        types::Exchange,
    };

    #[test]
    fn cannot_swap_with_no_pools() {
        let mut deps = mock_dependencies_with_custom_querier();

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let minimum_receive_amount = Coin {
            denom: "eth-usdc".to_string().clone(),
            amount: Uint128::new(50),
        };

        assert_eq!(
            ThorchainExchange::new(deps.as_ref())
                .can_swap(deps.as_ref(), &swap_amount, &minimum_receive_amount, &None)
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
    fn cannot_swap_with_non_thor_route() {
        let mut deps = mock_dependencies_with_custom_querier();

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

        let swap_amount = Coin {
            denom: "arb-eth".to_string().clone(),
            amount: Uint128::new(100),
        };

        let minimum_receive_amount = Coin {
            denom: "eth-usdc".to_string().clone(),
            amount: Uint128::new(50),
        };

        assert_eq!(
            ThorchainExchange::new(deps.as_ref())
                .can_swap(
                    deps.as_ref(),
                    &swap_amount,
                    &minimum_receive_amount,
                    &Some(Route::Fin {
                        address: Addr::unchecked("pair")
                    })
                )
                .unwrap_err(),
            StdError::generic_err("Route not supported for Thorchain exchange")
        );
    }

    #[test]
    fn can_swap_with_single_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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

        assert!(ThorchainExchange::new(deps.as_ref())
            .can_swap(deps.as_ref(), &swap_amount, &minimum_receive_amount, &None)
            .unwrap());
    }

    #[test]
    fn can_swap_with_multiple_pools() {
        let mut deps = mock_dependencies_with_custom_querier();

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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

        assert!(ThorchainExchange::new(deps.as_ref())
            .can_swap(deps.as_ref(), &swap_amount, &minimum_receive_amount, &None)
            .unwrap());
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;

    use calc_rs_test::test::mock_dependencies_with_custom_querier;
    use cosmwasm_std::{
        Addr, Binary, Coin, ContractResult, StdError, SystemError, SystemResult, Uint128,
    };
    use prost::Message;
    use rujira_rs::{
        proto::types::{QueryPoolRequest, QueryPoolResponse},
        NativeAsset,
    };

    #[test]
    fn fails_to_get_route_with_no_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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
            ThorchainExchange::new(deps.as_ref())
                .path(deps.as_ref(), &swap_amount, &target_denom, &None)
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

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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
            ThorchainExchange::new(deps.as_ref())
                .path(deps.as_ref(), &swap_amount, &target_denom, &None)
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
            ThorchainExchange::new(deps.as_ref())
                .path(deps.as_ref(), &swap_amount, &target_denom, &None)
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

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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
            ThorchainExchange::new(deps.as_ref())
                .path(deps.as_ref(), &swap_amount, &target_denom, &None)
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
            ThorchainExchange::new(deps.as_ref())
                .path(deps.as_ref(), &swap_amount, &target_denom, &None)
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

    use calc_rs_test::test::mock_dependencies_with_custom_querier;
    use cosmwasm_std::{Binary, ContractResult, SystemError, SystemResult};
    use prost::Message;
    use rujira_rs::proto::types::QueryPoolRequest;

    use super::*;

    #[test]
    fn fails_to_get_expected_receive_amount_with_no_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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
            ThorchainExchange::new(deps.as_ref())
                .path(deps.as_ref(), &swap_amount, &target_denom, &None)
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

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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
            ThorchainExchange::new(deps.as_ref())
                .expected_receive_amount(deps.as_ref(), &swap_amount, &target_denom, &None)
                .unwrap(),
            ExpectedReceiveAmount {
                receive_amount: Coin {
                    denom: target_denom.denom_string(),
                    amount: Uint128::new(99)
                },
                slippage_bps: 100u128
            }
        );

        let swap_amount = Coin {
            denom: "rune".to_string().clone(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("arb-eth");

        assert_eq!(
            ThorchainExchange::new(deps.as_ref())
                .expected_receive_amount(deps.as_ref(), &swap_amount, &target_denom, &None)
                .unwrap(),
            ExpectedReceiveAmount {
                receive_amount: Coin {
                    denom: target_denom.denom_string(),
                    amount: Uint128::new(99)
                },
                slippage_bps: 100u128
            }
        );
    }

    #[test]
    fn gets_expected_receive_amount_with_multiple_pools() {
        let mut deps = mock_dependencies_with_custom_querier();

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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
            ThorchainExchange::new(deps.as_ref())
                .expected_receive_amount(deps.as_ref(), &swap_amount, &target_denom, &None)
                .unwrap(),
            ExpectedReceiveAmount {
                receive_amount: Coin {
                    denom: target_denom.denom_string(),
                    amount: Uint128::new(98)
                },
                slippage_bps: 200u128
            }
        );

        let swap_amount = Coin {
            denom: "eth-usdc".to_string().clone(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("arb-eth");

        assert_eq!(
            ThorchainExchange::new(deps.as_ref())
                .expected_receive_amount(deps.as_ref(), &swap_amount, &target_denom, &None)
                .unwrap(),
            ExpectedReceiveAmount {
                receive_amount: Coin {
                    denom: target_denom.denom_string(),
                    amount: Uint128::new(98)
                },
                slippage_bps: 200u128
            }
        );
    }
}

#[cfg(test)]
mod spot_price_tests {

    use std::str::FromStr;

    use super::*;

    use calc_rs_test::test::mock_dependencies_with_custom_querier;
    use cosmwasm_std::{
        Addr, Binary, ContractResult, Decimal, StdError, SystemError, SystemResult,
    };
    use prost::Message;
    use rujira_rs::{
        proto::types::{QueryPoolRequest, QueryPoolResponse},
        NativeAsset,
    };

    #[test]
    fn fails_to_get_spot_price_with_no_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

        deps.querier.with_grpc_handler(|query| {
            SystemResult::Err(SystemError::InvalidRequest {
                error: "No such pool".to_string(),
                request: query.data.clone(),
            })
        });

        let swap_asset = NativeAsset::new("arb-eth");
        let target_denom = NativeAsset::new("eth-usdc");

        assert_eq!(
            ThorchainExchange::new(deps.as_ref())
                .spot_price(deps.as_ref(), &swap_asset, &target_denom, &None)
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

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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
            ThorchainExchange::new(deps.as_ref())
                .spot_price(deps.as_ref(), &swap_asset, &target_denom, &None)
                .unwrap(),
            Decimal::from_str("0.2").unwrap()
        );

        let swap_asset = NativeAsset::new("rune");
        let target_denom = NativeAsset::new("arb-eth");

        assert_eq!(
            ThorchainExchange::new(deps.as_ref())
                .spot_price(deps.as_ref(), &swap_asset, &target_denom, &None)
                .unwrap(),
            Decimal::from_str("5").unwrap()
        );
    }

    #[test]
    fn gets_spot_price_with_multiple_pools() {
        let mut deps = mock_dependencies_with_custom_querier();

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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
            ThorchainExchange::new(deps.as_ref())
                .spot_price(deps.as_ref(), &swap_asset, &target_denom, &None)
                .unwrap(),
            Decimal::from_str("1").unwrap()
        );

        let swap_asset = NativeAsset::new("arb-eth");
        let target_denom = NativeAsset::new("eth-usdc");

        assert_eq!(
            ThorchainExchange::new(deps.as_ref())
                .spot_price(deps.as_ref(), &swap_asset, &target_denom, &None)
                .unwrap(),
            Decimal::from_str("1").unwrap()
        );
    }
}

#[cfg(test)]
mod swap_tests {
    use super::*;

    use calc_rs_test::test::mock_dependencies_with_custom_querier;
    use cosmwasm_std::{
        testing::mock_env, Addr, Api, Binary, Coin, ContractResult, MessageInfo, Response,
        StdError, SystemError, SystemResult, Uint128,
    };
    use prost::Message;
    use rujira_rs::{
        proto::types::{QueryPoolRequest, QueryPoolResponse},
        NativeAsset,
    };

    #[test]
    fn fails_to_swap_with_no_pool() {
        let mut deps = mock_dependencies_with_custom_querier();

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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
            ThorchainExchange::new(deps.as_ref())
                .swap(
                    deps.as_ref(),
                    &mock_env(),
                    &MessageInfo {
                        sender: Addr::unchecked("sender"),
                        funds: vec![],
                    },
                    &swap_amount,
                    &minimum_receive_amount,
                    &None,
                    Addr::unchecked("recipient"),
                    None
                )
                .unwrap_err(),
            ContractError::Std(StdError::generic_err(format!(
                "Failed to load pool for asset {}",
                layer_1_asset(&NativeAsset::new(&swap_amount.denom))
                    .unwrap()
                    .denom_string()
            )))
        );
    }

    #[test]
    fn swaps_with_single_pool() {
        let mut deps = mock_dependencies_with_custom_querier();
        let env = mock_env();

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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
            ThorchainExchange::new(deps.as_ref())
                .swap(
                    deps.as_ref(),
                    &mock_env(),
                    &MessageInfo {
                        sender: Addr::unchecked("sender"),
                        funds: vec![],
                    },
                    &swap_amount,
                    &minimum_receive_amount,
                    &None,
                    recipient.clone(),
                    None
                )
                .unwrap(),
            Response::default().add_message(MsgDeposit {
                memo: format!(
                    "=:{}:{}:{}:rj:10:calc-swap",
                    "THOR.RUNE".to_string(),
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

        SCHEDULER
            .save(deps.as_mut().storage, &Addr::unchecked("scheduler"))
            .unwrap();

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
            ThorchainExchange::new(deps.as_ref())
                .swap(
                    deps.as_ref(),
                    &env,
                    &MessageInfo {
                        sender: Addr::unchecked("sender"),
                        funds: vec![],
                    },
                    &swap_amount,
                    &minimum_receive_amount,
                    &None,
                    recipient.clone(),
                    None
                )
                .unwrap(),
            Response::default().add_message(MsgDeposit {
                memo: format!(
                    "=:{}:{}:{}:rj:10:calc-swap",
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
