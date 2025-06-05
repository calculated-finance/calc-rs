use calc_rs::msg::{ExchangeExecuteMsg, ExchangeQueryMsg};
use calc_rs::types::{Contract, ContractError, ContractResult};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, Binary, Coin, Decimal, Deps, DepsMut, Env, MessageInfo,
    QueryRequest, Response, StdError, StdResult, WasmQuery,
};
use rujira_rs::fin::{
    BookResponse, ExecuteMsg as FinExecuteMsg, QueryMsg, SimulationResponse, SwapRequest,
};
use rujira_rs::query::Pool;
use rujira_rs::{Asset, Layer1Asset, NativeAsset, SecuredAsset};

use crate::exchanges::fin::FinExchange;
use crate::state::{delete_pair, find_pair, save_pair, ADMIN};
use crate::types::{Exchange, Pair, PositionType};

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

fn parse_asset(denom: &str) -> Asset {
    if let Some((chain, symbol)) = denom.split_once('-') {
        Asset::Secured(SecuredAsset::new(chain.into(), symbol.into()))
    } else if let Some((chain, symbol)) = denom.split_once('.') {
        Asset::Layer1(Layer1Asset::new(chain.into(), symbol.into()))
    } else if denom == "rune" {
        Asset::Layer1(Layer1Asset::new("THOR".into(), "RUNE".into()))
    } else {
        Asset::Native(NativeAsset::new(denom))
    }
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExchangeExecuteMsg,
) -> ContractResult {
    let exchanges = vec![FinExchange {}];

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

            let swap_asset = parse_asset(&swap_amount.denom);
            let return_asset = parse_asset(&minimum_receive_amount.denom);

            match swap_asset {
                Asset::Native(_) => match return_asset {
                    Asset::Native(return_asset) => {
                        swap_native_to_native(deps, swap_amount, return_asset.denom_string(), info)
                    }
                    Asset::Secured(_) => {
                        Ok(Response::default())
                        // Handle native to secured asset swap
                    }
                    Asset::Layer1(return_asset) => {
                        if !return_asset.is_rune() {
                            return Err(ContractError::Generic(
                                "Layer 1 asset swaps only supported for RUNE",
                            ));
                        }

                        Ok(Response::default())
                    }
                },
                Asset::Secured(swap_asset) => {
                    Ok(Response::default())
                    // Handle secured asset case
                }
                Asset::Layer1(swap_asset) => {
                    if !swap_asset.is_rune() {
                        return Err(ContractError::Generic(
                            "Layer 1 asset swaps only supported for RUNE",
                        ));
                    }

                    match return_asset {
                        Asset::Native(return_asset) => swap_native_to_native(
                            deps,
                            swap_amount,
                            return_asset.denom_string(),
                            info,
                        ),
                        Asset::Secured(return_asset) => {
                            Ok(Response::default())
                            // Handle RUNE to secured asset swap
                        }
                        Asset::Layer1(return_asset) => {
                            if !return_asset.is_rune() {
                                return Err(ContractError::Generic(
                                    "Layer 1 asset swaps only supported for RUNE",
                                ));
                            }

                            Ok(Response::default())
                        }
                    }
                }
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

fn swap_native_to_native(
    deps: DepsMut,
    swap_amount: Coin,
    target_denom: String,
    info: MessageInfo,
) -> ContractResult {
    match find_pair(
        deps.storage,
        [swap_amount.denom.clone(), target_denom.clone()],
    ) {
        Ok(pair) => {
            let msg = to_json_binary(&FinExecuteMsg::Swap(SwapRequest {
                min_return: None,
                to: Some(info.sender.to_string()),
                callback: None,
            }))?;
            Ok(Response::new().add_message(Contract(pair.address).call(msg, vec![swap_amount])?))
        }
        Err(_) => Err(ContractError::Std(StdError::generic_err("Pair not found"))),
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: ExchangeQueryMsg) -> StdResult<Binary> {
    let exchanges = vec![FinExchange {}];

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
