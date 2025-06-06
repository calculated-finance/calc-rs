use calc_rs::types::{ContractError, ContractResult};
use cosmwasm_std::{Coin, Decimal, Deps, MessageInfo, StdError, StdResult, Uint128};
use rujira_rs::{query::Pool, Asset, Layer1Asset};

use crate::types::Exchange;

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

fn get_expected_receive_amount(
    pool: &Pool,
    swap_asset: Layer1Asset,
    swap_amount: Uint128,
) -> StdResult<(Layer1Asset, Uint128)> {
    let receive_asset = match swap_asset.denom_string().as_str() {
        "THOR.RUNE" => match pool.asset.clone() {
            Asset::Layer1(asset) => asset,
            _ => return Err(StdError::generic_err("Pool asset is not a Layer1 asset")),
        },
        _ => Layer1Asset::new(&"THOR", &"RUNE"),
    };

    let receive_amount = swap_amount
        .checked_mul(pool.balance_asset)?
        .checked_mul(pool.balance_rune)?
        .checked_div(
            swap_amount
                .checked_add(match swap_asset.denom_string().as_str() {
                    "THOR.RUNE" => pool.balance_rune,
                    _ => pool.balance_asset,
                })?
                .pow(2),
        )?;

    Ok((receive_asset, receive_amount))
}

impl Exchange for PoolExchange {
    fn can_swap(&self, _deps: Deps, _swap_denom: &str, _target_denom: &str) -> bool {
        false
    }

    fn get_expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: Coin,
        target_denom: &str,
    ) -> StdResult<Coin> {
        let route: Vec<Pool> = [swap_amount.denom.as_str(), target_denom]
            .iter()
            .filter(|&&denom| denom != "rune")
            .map(|&denom| {
                let asset = Layer1Asset::from_native(denom.to_string())
                    .map_err(|e| StdError::generic_err(format!("Invalid asset: {}", e)))?;
                load_pool(deps, asset)
            })
            .collect::<StdResult<Vec<Pool>>>()?;

        if route.is_empty() {
            return Err(StdError::generic_err("No valid route found"));
        }

        let (asset, amount) = route.iter().fold(
            (
                Layer1Asset::from_native(swap_amount.denom)
                    .map_err(|e| StdError::generic_err(format!("Invalid asset: {}", e)))?,
                swap_amount.amount,
            ),
            |(current_asset, current_amount), pool| {
                get_expected_receive_amount(pool, current_asset, current_amount)
                    .expect("Failed to get expected receive amount")
            },
        );

        Ok(Coin {
            denom: asset.denom_string(),
            amount,
        })
    }

    fn get_spot_price(
        &self,
        _deps: Deps,
        _swap_denom: &str,
        _target_denom: &str,
    ) -> StdResult<Decimal> {
        Err(StdError::generic_err(
            "get_spot_price not implemented for PoolExchange",
        ))
    }

    fn swap(
        &self,
        _deps: Deps,
        _info: MessageInfo,
        _swap_amount: Coin,
        _minimum_receive_amount: Coin,
    ) -> ContractResult {
        Err(ContractError::Generic(
            "swap not implemented for PoolExchange",
        ))
    }
}
