use calc_rs::{
    math::checked_mul,
    types::{Contract, ContractError, ContractResult, ExpectedReturnAmount},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, Decimal, Deps, Env, MessageInfo, QueryRequest, Response, StdError,
    StdResult, WasmQuery,
};
use rujira_rs::fin::{BookResponse, ExecuteMsg, QueryMsg, SimulationResponse, SwapRequest};

use crate::{state::find_pair, types::Exchange};

#[cw_serde]
#[derive(Hash)]
pub enum PositionType {
    Enter,
    Exit,
}

#[cw_serde]
pub struct Pair {
    pub base_denom: String,
    pub quote_denom: String,
    pub address: Addr,
    pub decimal_delta: i8,
    pub price_precision: u8,
}

impl Pair {
    pub fn position_type(&self, swap_denom: &str) -> PositionType {
        if self.quote_denom == swap_denom {
            PositionType::Enter
        } else {
            PositionType::Exit
        }
    }

    pub fn denoms(&self) -> [String; 2] {
        [self.base_denom.clone(), self.quote_denom.clone()]
    }

    pub fn other_denom(&self, swap_denom: String) -> String {
        if self.quote_denom == swap_denom {
            self.base_denom.clone()
        } else {
            self.quote_denom.clone()
        }
    }
}

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

    fn route(&self, deps: Deps, swap_amount: Coin, target_denom: &str) -> StdResult<Vec<Coin>> {
        let receive_amount =
            self.get_expected_receive_amount(deps, swap_amount.clone(), target_denom)?;

        Ok(vec![swap_amount, receive_amount.return_amount])
    }

    fn get_expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: Coin,
        target_denom: &str,
    ) -> StdResult<ExpectedReturnAmount> {
        match find_pair(
            deps.storage,
            [swap_amount.denom.clone(), target_denom.to_string()],
        ) {
            Ok(pair) => {
                let simulation = deps
                    .querier
                    .query::<SimulationResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: pair.address.into_string(),
                        msg: to_json_binary(&QueryMsg::Simulate(swap_amount.clone()))?,
                    }))?;

                let spot_price = self.get_spot_price(deps, &swap_amount.denom, &target_denom)?;

                let optimal_return_amount =
                    checked_mul(swap_amount.amount, Decimal::one() / spot_price).map_err(|e| {
                        StdError::generic_err(format!(
                            "Failed to calculate optimal return amount: {}",
                            e
                        ))
                    })?;

                let slippage = Decimal::one().checked_sub(Decimal::from_ratio(
                    simulation.returned,
                    optimal_return_amount,
                ))?;

                Ok(ExpectedReturnAmount {
                    return_amount: Coin {
                        denom: target_denom.to_string(),
                        amount: simulation.returned,
                    },
                    slippage,
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
        _env: Env,
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
