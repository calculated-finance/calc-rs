use calc_rs::msg::{ExchangeExecuteMsg, ExchangeQueryMsg};
use calc_rs::types::{ContractError, ContractResult};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdError,
    StdResult,
};
use rujira_rs::query::Pool;
use rujira_rs::{Asset, Layer1Asset};

use crate::exchanges::fin::FinExchange;
use crate::state::{delete_pair, save_pair, ADMIN};
use crate::types::{Exchange, Pair};

#[cw_serde]
pub struct InstantiateMsg {}

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    _msg: InstantiateMsg,
) -> ContractResult {
    ADMIN.save(deps.storage, &info.sender)?;
    Ok(Response::default())
}

#[cw_serde]
enum CustomMsg {
    CreatePairs { pairs: Vec<Pair> },
    DeletePairs { pairs: Vec<Pair> },
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExchangeExecuteMsg,
) -> ContractResult {
    let exchanges = vec![FinExchange::new()];

    match msg {
        ExchangeExecuteMsg::Swap {
            minimum_receive_amount,
            ..
        } => {
            if info.funds.len() != 1 {
                return Err(StdError::generic_err("Must provide exactly one coin to swap").into());
            }

            if info.funds[0].amount.is_zero() {
                return Err(StdError::generic_err("Must provide a non-zero amount to swap").into());
            }

            let swap_amount = info.funds[0].clone();
            let target_denom = minimum_receive_amount.denom.clone();

            let best_exchange = exchanges
                .iter()
                .filter(|e| e.can_swap(deps.as_ref(), &swap_amount.denom, &target_denom))
                .max_by(|a, b| {
                    a.get_expected_receive_amount(deps.as_ref(), swap_amount.clone(), &target_denom)
                        .expect(
                            format!(
                                "Failed to get expected receive amount for {} to {}",
                                swap_amount.denom, target_denom
                            )
                            .as_str(),
                        )
                        .amount
                        .cmp(
                            &b.get_expected_receive_amount(
                                deps.as_ref(),
                                swap_amount.clone(),
                                &target_denom,
                            )
                            .expect(
                                format!(
                                    "Failed to get expected receive amount for {} to {}",
                                    swap_amount.denom, target_denom
                                )
                                .as_str(),
                            )
                            .amount,
                        )
                });

            match best_exchange {
                Some(exchange) => exchange.swap(
                    deps.as_ref(),
                    info,
                    swap_amount.clone(),
                    minimum_receive_amount,
                ),
                None => Err(StdError::generic_err(format!(
                    "Unable to find an exchange for swapping {} to {}",
                    swap_amount.denom, target_denom
                ))
                .into()),
            }
        }
        ExchangeExecuteMsg::Custom(custom_msg) => {
            if info.sender != ADMIN.load(deps.storage)? {
                return Err(ContractError::Unauthorized {});
            }

            match from_json::<CustomMsg>(&custom_msg)? {
                CustomMsg::CreatePairs { pairs } => {
                    for pair in pairs {
                        save_pair(deps.storage, &pair)?;
                    }
                    Ok(Response::default())
                }
                CustomMsg::DeletePairs { pairs } => {
                    for pair in pairs {
                        delete_pair(deps.storage, &pair);
                    }
                    Ok(Response::default())
                }
            }
        }
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: ExchangeQueryMsg) -> StdResult<Binary> {
    let exchanges = vec![FinExchange::new()];

    match msg {
        ExchangeQueryMsg::GetExpectedReceiveAmount {
            swap_amount,
            target_denom,
            ..
        } => exchanges
            .iter()
            .filter(|e| e.can_swap(deps, &swap_amount.denom, &target_denom))
            .map(|e| e.get_expected_receive_amount(deps, swap_amount.clone(), &target_denom))
            .collect::<StdResult<Vec<_>>>()?
            .into_iter()
            .max_by(|a, b| a.amount.cmp(&b.amount))
            .map_or_else(
                || {
                    Err(StdError::generic_err(format!(
                        "Unable to find an exchange for swapping {} to {}",
                        swap_amount.denom, target_denom
                    )))
                },
                |amount| to_json_binary(&amount),
            ),
        ExchangeQueryMsg::GetSpotPrice {
            swap_denom,
            target_denom,
            ..
        } => exchanges
            .iter()
            .filter(|e| e.can_swap(deps, &swap_denom, &target_denom))
            .map(|e| e.get_spot_price(deps, &swap_denom, &target_denom))
            .collect::<StdResult<Vec<_>>>()?
            .into_iter()
            .max_by(|a, b| a.cmp(b))
            .map_or_else(
                || {
                    Err(StdError::generic_err(format!(
                        "Unable to find an exchange for spot price of {} to {}",
                        swap_denom, target_denom
                    )))
                },
                |price| to_json_binary(&price),
            ),
        ExchangeQueryMsg::GetUsdPrice { asset } => match asset {
            Asset::Native(asset) => {
                let oracle = Layer1Asset::from_native(asset.denom_string().to_ascii_uppercase())
                    .map_err(|e| {
                        StdError::generic_err(format!(
                            "Unable to build layer 1 asset from native asset {:?}: {:?}",
                            asset, e
                        ))
                    })?;

                let pool = Pool::load(deps.querier, &oracle).map_err(|e| {
                    StdError::generic_err(format!(
                        "Unable to load pool from layer 1 asset {:?}: {:?}",
                        oracle, e
                    ))
                })?;

                to_json_binary(&pool.asset_tor_price)
            }
            Asset::Layer1(asset) => {
                let pool = Pool::load(deps.querier, &asset).map_err(|e| {
                    StdError::generic_err(format!(
                        "Unable to load pool from layer 1 asset {:?}: {:?}",
                        asset, e
                    ))
                })?;

                to_json_binary(&pool.asset_tor_price)
            }
            Asset::Secured(asset) => {
                let oracle = Layer1Asset::from_native(asset.denom_string().to_ascii_uppercase())
                    .map_err(|e| {
                        StdError::generic_err(format!(
                            "Unable to build layer 1 asset from secured asset {:?}: {:?}",
                            asset, e
                        ))
                    })?;

                let pool = Pool::load(deps.querier, &oracle).map_err(|e| {
                    StdError::generic_err(format!(
                        "Unable to load pool from layer 1 asset {:?}: {:?}",
                        oracle, e
                    ))
                })?;

                to_json_binary(&pool.asset_tor_price)
            }
        },
    }
}

#[cfg(test)]
mod tests {}
