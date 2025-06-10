use calc_rs::{
    math::checked_mul,
    types::{ContractResult, ExpectedReturnAmount},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, Decimal, Deps, MessageInfo, StdError, StdResult, Uint128};
use rujira_rs::{query::Pool, Asset, Layer1Asset, SecuredAsset};

use crate::types::Exchange;

#[cw_serde]
pub struct PoolExchange {
    pub name: String,
}

impl PoolExchange {
    pub fn new() -> Self {
        PoolExchange {
            name: "Pool".to_string(),
        }
    }
}

pub fn layer_1_asset(denom: &str) -> StdResult<Layer1Asset> {
    if denom.contains("rune") {
        return Ok(Layer1Asset::new("THOR", "RUNE"));
    }

    let (chain, symbol) = denom
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
    Ok(Pool::load(deps.querier, asset).map_err(|e| {
        StdError::generic_err(format!(
            "Failed to load pool for asset {}: {}",
            asset.denom_string(),
            e
        ))
    })?)
}

fn get_pools(deps: Deps, swap_denom: &str, target_denom: &str) -> Result<Vec<Pool>, StdError> {
    Ok([swap_denom, target_denom]
        .iter()
        .filter(|&&denom| !denom.to_lowercase().contains("rune"))
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

impl Exchange for PoolExchange {
    fn can_swap(&self, deps: Deps, swap_denom: &str, target_denom: &str) -> StdResult<bool> {
        Ok(!get_pools(deps, swap_denom, target_denom)?.is_empty())
    }

    fn route(&self, deps: Deps, swap_amount: Coin, target_denom: &str) -> StdResult<Vec<Coin>> {
        let pools = get_pools(deps, swap_amount.denom.as_str(), target_denom)?;

        if pools.is_empty() {
            return Err(StdError::generic_err("No valid route found"));
        }

        let mut route = vec![swap_amount.clone()];

        for (i, pool) in pools.iter().enumerate() {
            let (out_asset, out_amount) = get_expected_receive_amount(
                pool,
                &layer_1_asset(&route[i].denom)?,
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

    fn get_expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: Coin,
        target_denom: &str,
    ) -> StdResult<ExpectedReturnAmount> {
        let pools = get_pools(deps, swap_amount.denom.as_str(), target_denom)?;

        if pools.is_empty() {
            return Err(StdError::generic_err("No valid route found"));
        }

        let (_, out_amount) = pools.iter().fold(
            (layer_1_asset(&swap_amount.denom)?, swap_amount.amount),
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

        let spot_price = self.get_spot_price(deps, &swap_amount.denom, &target_denom)?;

        let optimal_return_amount = checked_mul(swap_amount.amount, Decimal::one() / spot_price)
            .map_err(|e| {
                StdError::generic_err(format!("Failed to calculate optimal return amount: {}", e))
            })?;

        let slippage =
            Decimal::one().checked_sub(Decimal::from_ratio(out_amount, optimal_return_amount))?;

        Ok(ExpectedReturnAmount {
            amount: Coin {
                denom: target_denom.to_string(),
                amount: out_amount,
            },
            slippage,
        })
    }

    fn get_spot_price(
        &self,
        deps: Deps,
        swap_denom: &str,
        target_denom: &str,
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
        _deps: Deps,
        _info: MessageInfo,
        _swap_amount: Coin,
        _minimum_receive_amount: Coin,
    ) -> ContractResult {
        unimplemented!("PoolExchange::swap is not implemented yet")
        // let target_asset = Layer1Asset::from_native(minimum_receive_amount.denom).map_err(|e| {
        //     ContractError::Std(StdError::generic_err(
        //         format!(
        //             "Unable to map {} to Layer 1 asset: {}",
        //             minimum_receive_amount.denom, e
        //         )
        //         .as_str(),
        //     ))
        // })?;

        // MsgSwap {
        //     tx: todo!(),
        //     target_asset: todo!(),
        //     destination: todo!(),
        //     trade_target: todo!(),
        //     affiliate_address: todo!(),
        //     affiliate_basis_points: todo!(),
        //     signer: todo!(),
        //     aggregator: todo!(),
        //     aggregator_target_address: todo!(),
        //     aggregator_target_limit: todo!(),
        //     order_type: todo!(),
        //     stream_quantity: todo!(),
        //     stream_interval: todo!(),
        // };
    }
}
