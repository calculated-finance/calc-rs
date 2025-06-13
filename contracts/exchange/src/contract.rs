use calc_rs::msg::{ExchangeExecuteMsg, ExchangeQueryMsg, SchedulerInstantiateMsg};
use calc_rs::types::{ContractError, ContractResult};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Response, StdError,
    StdResult, Uint128,
};

use crate::exchanges::fin::FinExchange;
use crate::exchanges::pool::PoolExchange;
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

#[entry_point]
pub fn migrate(_: DepsMut, __: Env, ___: SchedulerInstantiateMsg) -> ContractResult {
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
    env: Env,
    info: MessageInfo,
    msg: ExchangeExecuteMsg,
) -> ContractResult {
    let exchanges: Vec<Box<dyn Exchange>> =
        vec![Box::new(FinExchange::new()), Box::new(PoolExchange::new())];

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
                .filter(|e| {
                    e.can_swap(deps.as_ref(), &swap_amount.denom, &target_denom)
                        .unwrap_or(false)
                })
                .max_by(|a, b| {
                    a.get_expected_receive_amount(deps.as_ref(), swap_amount.clone(), &target_denom)
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
                            .return_amount
                            .amount,
                        )
                });

            match best_exchange {
                Some(exchange) => exchange.swap(
                    deps.as_ref(),
                    env,
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
    let exchanges: Vec<Box<dyn Exchange>> =
        vec![Box::new(FinExchange::new()), Box::new(PoolExchange::new())];

    match msg {
        ExchangeQueryMsg::Route {
            swap_amount,
            target_denom,
        } => {
            let route = exchanges
                .iter()
                .filter(|e| {
                    e.can_swap(deps, &swap_amount.denom, &target_denom)
                        .unwrap_or(false)
                })
                .flat_map(|e| e.route(deps, swap_amount.clone(), &target_denom).ok())
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
                    "Unable to find an exchange for swapping {} to {}",
                    swap_amount.denom, target_denom
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
                e.can_swap(deps, &swap_amount.denom, &target_denom)
                    .unwrap_or(false)
            })
            .flat_map(|e| {
                e.get_expected_receive_amount(deps, swap_amount.clone(), &target_denom)
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
            .filter(|e| {
                e.can_swap(deps, &swap_denom, &target_denom)
                    .unwrap_or(false)
            })
            .flat_map(|e| e.get_spot_price(deps, &swap_denom, &target_denom).ok())
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

#[cfg(test)]
mod tests {}
