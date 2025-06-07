use calc_rs::{math::checked_mul, types::ContractResult};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, Decimal, Deps, MessageInfo, StdError, StdResult, Uint128};
use rujira_rs::{query::Pool, Asset, Layer1Asset};

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

fn load_pool(deps: Deps, asset: Layer1Asset) -> StdResult<Pool> {
    Ok(Pool::load(deps.querier, &asset)
        .map_err(|e| StdError::generic_err(format!("Failed to load pool: {}", e)))?)
}

const THOR_RUNE: &str = "thor.rune";

fn get_expected_receive_amount(
    pool: &Pool,
    swap_asset: Layer1Asset,
    swap_amount: Uint128,
) -> StdResult<(Layer1Asset, Uint128)> {
    let receive_asset = match swap_asset.denom_string().as_str() {
        THOR_RUNE => match pool.asset.clone() {
            Asset::Layer1(asset) => asset,
            _ => return Err(StdError::generic_err("Pool asset is not a Layer1 asset")),
        },
        _ => Layer1Asset::new("THOR", "rune"),
    };

    let receive_amount = swap_amount
        .checked_mul(pool.balance_asset)?
        .checked_mul(pool.balance_rune)?
        .checked_div(
            swap_amount
                .checked_add(match swap_asset.denom_string().as_str() {
                    THOR_RUNE => pool.balance_rune,
                    _ => pool.balance_asset,
                })?
                .pow(2),
        )?;

    Ok((receive_asset, receive_amount))
}

impl Exchange for PoolExchange {
    fn can_swap(&self, deps: Deps, swap_denom: &str, target_denom: &str) -> StdResult<bool> {
        let route = [swap_denom, target_denom]
            .iter()
            .filter(|&&denom| denom != "rune")
            .map(|&denom| {
                let asset = Layer1Asset::from_native(denom.to_string())
                    .map_err(|e| StdError::generic_err(format!("Invalid secured asset: {}", e)))?;
                load_pool(deps, asset)
            })
            .collect::<StdResult<Vec<Pool>>>()?;

        Ok(!route.is_empty())
    }

    fn get_expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: Coin,
        target_denom: &str,
    ) -> StdResult<Coin> {
        let route = [swap_amount.denom.as_str(), target_denom]
            .iter()
            .filter(|&&denom| denom != "rune")
            .map(|&denom| {
                let asset = Layer1Asset::from_native(denom.to_string())
                    .map_err(|e| StdError::generic_err(format!("Invalid secured asset: {}", e)))?;
                load_pool(deps, asset)
            })
            .collect::<StdResult<Vec<Pool>>>()?;

        if route.is_empty() {
            return Err(StdError::generic_err("No valid route found"));
        }

        let (out_asset, out_amount) = route.iter().fold(
            (
                Layer1Asset::from_native(swap_amount.denom)
                    .map_err(|e| StdError::generic_err(format!("Invalid secured asset: {}", e)))?,
                swap_amount.amount,
            ),
            |(in_asset, in_amount), pool| {
                get_expected_receive_amount(pool, in_asset, in_amount)
                    .expect("Failed to get expected receive amount")
            },
        );

        Ok(Coin {
            denom: out_asset.denom_string(),
            amount: out_amount,
        })
    }

    fn get_spot_price(
        &self,
        deps: Deps,
        swap_denom: &str,
        target_denom: &str,
    ) -> StdResult<Decimal> {
        let route = [swap_denom, target_denom]
            .iter()
            .filter(|&&denom| denom != "rune")
            .map(|&denom| {
                let asset = Layer1Asset::from_native(denom.to_string())
                    .map_err(|e| StdError::generic_err(format!("Invalid secured asset: {}", e)))?;
                load_pool(deps, asset)
            })
            .collect::<StdResult<Vec<Pool>>>()?;

        if route.is_empty() {
            return Err(StdError::generic_err("No valid route found"));
        }

        let in_amount = checked_mul(
            Uint128::new(100_000_000),
            route.first().unwrap().asset_tor_price,
        )
        .map_err(|e| {
            StdError::generic_err(format!(
                "Unable to calculate $1 USD worth of {}: {}",
                swap_denom, e
            ))
        })?;

        let (_, out_amount) = route.iter().fold(
            (
                Layer1Asset::from_native(swap_denom.to_string())
                    .map_err(|e| StdError::generic_err(format!("Invalid secured asset: {}", e)))?,
                in_amount,
            ),
            |(in_asset, in_amount), pool| {
                get_expected_receive_amount(pool, in_asset, in_amount)
                    .expect("Failed to get expected receive amount")
            },
        );

        Ok(Decimal::from_ratio(in_amount, out_amount))
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
