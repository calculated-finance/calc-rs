use calc_rs::types::{Contract, ContractError, ContractResult};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Coin, Decimal, Deps, MessageInfo, QueryRequest, Response, StdError, StdResult,
    WasmQuery,
};
use rujira_rs::fin::{BookResponse, ExecuteMsg, QueryMsg, SimulationResponse, SwapRequest};

use crate::{
    state::find_pair,
    types::{Exchange, PositionType},
};

#[cw_serde]
pub struct FinExchange {
    pub name: String,
}

impl FinExchange {
    pub fn new() -> Self {
        FinExchange {
            name: "Fin".to_string(),
        }
    }
}

impl Exchange for FinExchange {
    fn can_swap(&self, deps: Deps, swap_denom: &str, target_denom: &str) -> StdResult<bool> {
        Ok(find_pair(
            deps.storage,
            [swap_denom.to_string(), target_denom.to_string()],
        )
        .is_ok())
    }

    fn get_expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: Coin,
        target_denom: &str,
    ) -> StdResult<Coin> {
        match find_pair(
            deps.storage,
            [swap_amount.denom.clone(), target_denom.to_string()],
        ) {
            Ok(pair) => {
                let simulation = deps
                    .querier
                    .query::<SimulationResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: pair.address.into_string(),
                        msg: to_json_binary(&QueryMsg::Simulate(swap_amount))?,
                    }))?;

                Ok(Coin {
                    denom: target_denom.to_string(),
                    amount: simulation.returned,
                })
            }
            Err(_) => Err(StdError::generic_err("Pair not found")),
        }
    }

    fn get_spot_price(
        &self,
        deps: Deps,
        swap_denom: &str,
        target_denom: &str,
    ) -> StdResult<Decimal> {
        match find_pair(
            deps.storage,
            [swap_denom.to_string(), target_denom.to_string()],
        ) {
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

                Ok(match position_type {
                    PositionType::Enter => quote_price,
                    PositionType::Exit => Decimal::one()
                        .checked_div(quote_price)
                        .expect("should return a valid inverted price for fin sell"),
                })
            }
            Err(_) => Err(StdError::generic_err("Pair not found")),
        }
    }

    fn swap(
        &self,
        deps: Deps,
        info: MessageInfo,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
    ) -> ContractResult {
        match find_pair(
            deps.storage,
            [
                swap_amount.denom.clone(),
                minimum_receive_amount.denom.clone(),
            ],
        ) {
            Ok(pair) => {
                let msg = to_json_binary(&ExecuteMsg::Swap(SwapRequest {
                    min_return: None,
                    to: Some(info.sender.to_string()),
                    callback: None,
                }))?;
                Ok(Response::new()
                    .add_message(Contract(pair.address).call(msg, vec![swap_amount])?))
            }
            Err(_) => Err(ContractError::Generic("Pair not found")),
        }
    }
}
