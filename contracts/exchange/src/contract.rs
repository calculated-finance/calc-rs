use calc_rs::msg::{ExchangeExecuteMsg, ExchangeQueryMsg, SchedulerInstantiateMsg};
use calc_rs::types::{ContractError, ContractResult};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Response, StdError,
    StdResult, Uint128,
};
use rujira_rs::proto::types::{QueryQuoteSwapRequest, QueryQuoteSwapResponse};
use rujira_rs::NativeAsset;

use crate::exchanges::fin::{delete_pair, save_pair, FinExchange, Pair};
use crate::exchanges::thor::{Queryable, ThorExchange};
use crate::state::ADMIN;
use crate::types::Exchange;

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

#[entry_point]
pub fn migrate(_: DepsMut, __: Env, ___: SchedulerInstantiateMsg) -> ContractResult {
    Ok(Response::default())
}

#[cw_serde]
enum CustomMsg {
    CreatePairs { pairs: Vec<Pair> },
    DeletePairs { pairs: Vec<Pair> },
}

#[cfg(not(feature = "library"))]
pub fn get_exchanges() -> Vec<Box<dyn Exchange>> {
    vec![Box::new(FinExchange::new()), Box::new(ThorExchange::new())]
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExchangeExecuteMsg,
) -> ContractResult {
    let exchanges = get_exchanges();

    match msg {
        ExchangeExecuteMsg::Swap {
            minimum_receive_amount,
            recipient,
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
                .filter(|e| {
                    e.can_swap(deps.as_ref(), &swap_amount, &minimum_receive_amount)
                        .unwrap_or(false)
                })
                .max_by(|a, b| {
                    a.expected_receive_amount(
                        deps.as_ref(),
                        &swap_amount,
                        &NativeAsset::new(&target_denom),
                    )
                    .expect(
                        format!(
                            "Failed to get expected receive amount for {} to {}",
                            swap_amount.denom, target_denom
                        )
                        .as_str(),
                    )
                    .return_amount
                    .amount
                    .cmp(
                        &b.expected_receive_amount(
                            deps.as_ref(),
                            &swap_amount,
                            &NativeAsset::new(&target_denom),
                        )
                        .expect(
                            format!(
                                "Failed to get expected receive amount for {} to {}",
                                swap_amount.denom, target_denom
                            )
                            .as_str(),
                        )
                        .return_amount
                        .amount,
                    )
                });

            match best_exchange {
                Some(exchange) => exchange.swap(
                    deps.as_ref(),
                    env,
                    &swap_amount,
                    &minimum_receive_amount,
                    recipient.unwrap_or(info.sender.clone()),
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

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: ExchangeQueryMsg) -> StdResult<Binary> {
    let exchanges = get_exchanges();

    match msg {
        ExchangeQueryMsg::Custom {} => {
            let response = QueryQuoteSwapResponse::get(
                deps.querier,
                QueryQuoteSwapRequest {
                    from_asset: "THOR.RUNE".to_string(),
                    to_asset: "ETH.ETH".to_string(),
                    amount: 100000000.to_string(),
                    streaming_interval: 1.to_string(),
                    streaming_quantity: 1.to_string(),
                    destination: env.contract.address.to_string(),
                    tolerance_bps: 50.to_string(),
                    refund_address: env.contract.address.to_string(),
                    affiliate: vec![],
                    affiliate_bps: vec![],
                    height: 0.to_string(),
                },
            )
            .unwrap();

            Ok(to_json_binary(&response.expected_amount_out)?)
        }
        ExchangeQueryMsg::CanSwap {
            swap_amount,
            minimum_receive_amount,
        } => to_json_binary(&exchanges.iter().any(|e| {
            e.can_swap(deps, &swap_amount, &minimum_receive_amount)
                .unwrap_or(false)
        })),
        ExchangeQueryMsg::Route {
            swap_amount,
            target_denom,
        } => {
            let route = exchanges
                .iter()
                .filter(|e| {
                    e.can_swap(
                        deps,
                        &swap_amount,
                        &Coin {
                            denom: target_denom.clone(),
                            amount: Uint128::one(),
                        },
                    )
                    .unwrap_or(false)
                })
                .flat_map(|e| {
                    e.route(deps, &swap_amount, &NativeAsset::new(&target_denom))
                        .ok()
                })
                .max_by(|a, b| {
                    let empty = Coin {
                        denom: target_denom.clone(),
                        amount: Uint128::zero(),
                    };
                    a.last()
                        .unwrap_or(&empty)
                        .amount
                        .cmp(&b.last().unwrap_or(&empty).amount)
                })
                .into_iter()
                .collect::<Vec<_>>();

            if route.is_empty() {
                return Err(StdError::generic_err(format!(
                    "Unable to find an exchange for swapping {} to {}. Errors: [{}]",
                    swap_amount.denom,
                    target_denom,
                    exchanges
                        .iter()
                        .flat_map(|e| {
                            e.route(deps, &swap_amount, &NativeAsset::new(&target_denom))
                                .err()
                                .map(|e| e.to_string())
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }

            to_json_binary(&route)
        }
        ExchangeQueryMsg::ExpectedReceiveAmount {
            swap_amount,
            target_denom,
            ..
        } => exchanges
            .iter()
            .filter(|e| {
                e.can_swap(
                    deps,
                    &swap_amount,
                    &Coin {
                        denom: target_denom.clone(),
                        amount: Uint128::one(),
                    },
                )
                .unwrap_or(false)
            })
            .flat_map(|e| {
                e.expected_receive_amount(deps, &swap_amount, &NativeAsset::new(&target_denom))
                    .ok()
            })
            .max_by(|a, b| a.return_amount.amount.cmp(&b.return_amount.amount))
            .map_or_else(
                || {
                    Err(StdError::generic_err(format!(
                        "Unable to find an exchange for swapping {} to {}",
                        swap_amount.denom, target_denom
                    )))
                },
                |amount| to_json_binary(&amount),
            ),
        ExchangeQueryMsg::SpotPrice {
            swap_denom,
            target_denom,
            ..
        } => exchanges
            .iter()
            .map(|e| {
                e.spot_price(
                    deps,
                    &NativeAsset::new(&swap_denom),
                    &NativeAsset::new(&target_denom),
                )
                .ok()
            })
            .max_by(|a, b| a.cmp(&b))
            .map_or_else(
                || {
                    Err(StdError::generic_err(format!(
                        "Unable to find an exchange for swapping {} to {}",
                        swap_denom, target_denom
                    )))
                },
                |amount| to_json_binary(&amount),
            ),
    }
}
