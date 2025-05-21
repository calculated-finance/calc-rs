use calc_rs::msg::{ExchangeExecuteMsg, ExchangeQueryMsg};
use calc_rs::types::{Contract, ContractError, ContractResult};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Binary, Coin, Decimal, Deps, DepsMut, Env, MessageInfo,
    QueryRequest, Response, StdError, StdResult, WasmQuery,
};
use rujira_rs::fin::{
    BookResponse, ExecuteMsg as FinExecuteMsg, QueryMsg, SimulationResponse, SwapRequest,
};

use crate::state::{delete_pair, find_pair, save_pair, ADMIN};
use crate::types::{Pair, PositionType};

#[cw_serde]
pub struct InstantiateMsg {
    admin: Addr,
}

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> ContractResult {
    deps.api.addr_validate(&msg.admin.to_string())?;
    ADMIN.save(deps.storage, &msg.admin)?;
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
    match msg {
        ExchangeExecuteMsg::Swap {
            minimum_receive_amount,
            callback,
            ..
        } => {
            if info.funds.len() != 1 {
                return Err(StdError::generic_err("Must provide exactly one coin to swap").into());
            }

            if info.funds[0].amount.is_zero() {
                return Err(StdError::generic_err("Must provide a non-zero amount to swap").into());
            }

            match find_pair(
                deps.storage,
                [
                    info.funds[0].denom.clone(),
                    minimum_receive_amount.denom.clone(),
                ],
            ) {
                Ok(pair) => Ok(Response::new().add_message(Contract(pair.address).call(
                    to_json_binary(&FinExecuteMsg::Swap(SwapRequest {
                        min_return: Some(minimum_receive_amount.amount),
                        to: Some(info.sender.to_string()),
                        callback,
                    }))?,
                    info.funds,
                )?)),
                Err(_) => Err(ContractError::Std(StdError::generic_err("Pair not found"))),
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
    match msg {
        ExchangeQueryMsg::GetExpectedReceiveAmount {
            swap_amount,
            target_denom,
            ..
        } => match find_pair(
            deps.storage,
            [swap_amount.denom.clone(), target_denom.clone()],
        ) {
            Ok(pair) => {
                let res = deps
                    .querier
                    .query::<SimulationResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: pair.address.into_string(),
                        msg: to_json_binary(&QueryMsg::Simulate(swap_amount))?,
                    }))?;

                to_json_binary(&Coin {
                    denom: target_denom,
                    amount: res.returned,
                })
            }
            Err(_) => Err(StdError::generic_err("Pair not found")),
        },
        ExchangeQueryMsg::GetSpotPrice {
            swap_denom,
            target_denom,
            ..
        } => match find_pair(deps.storage, [swap_denom.clone(), target_denom.clone()]) {
            Ok(pair) => {
                let position_type = match swap_denom == pair.quote_denom {
                    true => PositionType::Enter,
                    false => PositionType::Exit,
                };

                let book_response = deps.querier.query_wasm_smart::<BookResponse>(
                    pair.address.clone(),
                    &QueryMsg::Book {
                        limit: Some(1),
                        offset: None,
                    },
                )?;

                let book = match position_type {
                    PositionType::Enter => book_response.base,
                    PositionType::Exit => book_response.quote,
                };

                if book.is_empty() {
                    return Err(StdError::generic_err(format!(
                        "No orders found for {} at fin pair {}",
                        swap_denom, pair.address
                    )));
                }

                let quote_price = book[0].price;

                to_json_binary(&match position_type {
                    PositionType::Enter => quote_price,
                    PositionType::Exit => Decimal::one()
                        .checked_div(quote_price)
                        .expect("should return a valid inverted price for fin sell"),
                })
            }
            Err(_) => Err(StdError::generic_err("Pair not found")),
        },
    }
}

#[cfg(test)]
mod tests {}
