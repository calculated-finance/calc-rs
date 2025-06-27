use std::{cmp::max, str::FromStr};

use calc_rs::types::{Callback, Contract, ContractResult, ExpectedReceiveAmount, Route};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, BankMsg, Coin, Decimal, Deps, Env, MessageInfo, Order, QueryRequest,
    Response, StdError, StdResult, Storage, Uint128, WasmQuery,
};
use cw_storage_plus::{Bound, Map};
use rujira_rs::{
    fin::{BookResponse, ConfigResponse, ExecuteMsg, QueryMsg, SimulationResponse, SwapRequest},
    NativeAsset,
};

use crate::types::Exchange;

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

const PAIRS: Map<String, Pair> = Map::new("pairs_v1");

pub fn save_pair(storage: &mut dyn Storage, pair: &Pair) -> StdResult<()> {
    PAIRS.save(storage, key_from(pair.denoms()), pair)
}

fn key_from(mut denoms: [String; 2]) -> String {
    denoms.sort();
    format!("{}-{}", denoms[0], denoms[1])
}

pub fn find_pair(storage: &dyn Storage, denoms: [String; 2]) -> StdResult<Pair> {
    PAIRS.load(storage, key_from(denoms.clone())).map_err(|_| {
        StdError::generic_err(format!(
            "No pair found for swapping from {} into {}",
            denoms[0], denoms[1]
        ))
    })
}

pub fn get_pairs(
    storage: &dyn Storage,
    start_after: Option<[String; 2]>,
    limit: Option<u16>,
) -> Vec<Pair> {
    PAIRS
        .range(
            storage,
            start_after.map(|denoms| Bound::exclusive(key_from(denoms))),
            None,
            Order::Ascending,
        )
        .take(limit.unwrap_or(30) as usize)
        .flat_map(|result| result.map(|(_, pair)| pair))
        .collect::<Vec<Pair>>()
}

pub fn delete_pair(storage: &mut dyn Storage, pair: &Pair) {
    PAIRS.remove(storage, key_from(pair.denoms()))
}

pub fn get_pair(
    deps: Deps,
    swap_denom: &str,
    target_denom: &str,
    route: &Option<Route>,
) -> StdResult<Pair> {
    let pair = if let Some(route) = route {
        match route {
            Route::Fin { address } => {
                let config = deps
                    .querier
                    .query_wasm_smart::<ConfigResponse>(address, &QueryMsg::Config {})?;

                let denoms = [config.denoms.base(), config.denoms.quote()];

                if !denoms.contains(&swap_denom) {
                    return Err(StdError::generic_err(format!(
                        "Pair at {} does not support swapping {}",
                        address, swap_denom
                    )));
                }

                if !denoms.contains(&target_denom) {
                    return Err(StdError::generic_err(format!(
                        "Pair at {} does not support swapping {}",
                        address, target_denom
                    )));
                }

                Pair {
                    base_denom: config.denoms.base().to_string(),
                    quote_denom: config.denoms.quote().to_string(),
                    address: Addr::unchecked(address),
                }
            }
            _ => {
                return Err(StdError::generic_err(
                    "Route not supported for Fin exchange",
                ));
            }
        }
    } else {
        find_pair(
            deps.storage,
            [swap_denom.to_string(), target_denom.to_string()],
        )?
    };

    Ok(pair)
}

#[cw_serde]
pub struct FinExchange {}

impl FinExchange {
    pub fn new() -> Self {
        FinExchange {}
    }
}

impl Exchange for FinExchange {
    fn can_swap(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        route: &Option<Route>,
    ) -> StdResult<bool> {
        let expected_receive_amount = self.expected_receive_amount(
            deps,
            swap_amount,
            &NativeAsset::new(&minimum_receive_amount.denom),
            route,
        )?;

        Ok(expected_receive_amount.receive_amount.amount >= minimum_receive_amount.amount)
    }

    fn path(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
        route: &Option<Route>,
    ) -> StdResult<Vec<Coin>> {
        let receive_amount =
            self.expected_receive_amount(deps, swap_amount, target_denom, route)?;

        Ok(vec![swap_amount.clone(), receive_amount.receive_amount])
    }

    fn expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
        route: &Option<Route>,
    ) -> StdResult<ExpectedReceiveAmount> {
        let pair = get_pair(
            deps,
            &swap_amount.denom,
            &target_denom.denom_string(),
            route,
        )?;

        let simulation = deps
            .querier
            .query::<SimulationResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: pair.address.into_string(),
                msg: to_json_binary(&QueryMsg::Simulate(swap_amount.clone()))?,
            }))?;

        let spot_price = self.spot_price(
            deps,
            &NativeAsset::new(&swap_amount.denom),
            &target_denom,
            route,
        )?;

        let optimal_return_amount = max(
            simulation.returned,
            swap_amount.amount.mul_floor(Decimal::one() / spot_price),
        );

        let slippage = Uint128::new(10_000).mul_ceil(
            Decimal::one()
                .checked_sub(Decimal::from_ratio(
                    simulation.returned,
                    optimal_return_amount,
                ))
                .unwrap_or(Decimal::one()),
        );

        Ok(ExpectedReceiveAmount {
            receive_amount: Coin {
                denom: target_denom.denom_string(),
                amount: simulation.returned,
            },
            slippage_bps: slippage.into(),
        })
    }

    fn spot_price(
        &self,
        deps: Deps,
        swap_denom: &NativeAsset,
        target_denom: &NativeAsset,
        route: &Option<Route>,
    ) -> StdResult<Decimal> {
        let pair = get_pair(
            deps,
            &swap_denom.denom_string(),
            &target_denom.denom_string(),
            route,
        )?;

        let position_type = match swap_denom.denom_string() == pair.quote_denom {
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

        if book_response.base.is_empty() || book_response.quote.is_empty() {
            return Err(StdError::generic_err(format!(
                "Not enough orders found for {} at fin pair {}",
                swap_denom, pair.address
            )));
        }

        let quote_price = (book_response.base[0].price + book_response.quote[0].price)
            / Decimal::from_str("2").unwrap();

        Ok(match position_type {
            PositionType::Enter => quote_price,
            PositionType::Exit => Decimal::one()
                .checked_div(quote_price)
                .expect("should return a valid inverted price for fin sell"),
        })
    }

    fn swap(
        &self,
        deps: Deps,
        _env: &Env,
        info: &MessageInfo,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        route: &Option<Route>,
        recipient: Addr,
        on_complete: Option<Callback>,
    ) -> ContractResult {
        let pair = get_pair(
            deps,
            &swap_amount.denom,
            &minimum_receive_amount.denom,
            route,
        )?;

        let swap_msg = Contract(pair.address).call(
            to_json_binary(&ExecuteMsg::Swap(SwapRequest {
                min_return: Some(minimum_receive_amount.amount),
                to: Some(recipient.to_string()),
                callback: None,
            }))?,
            vec![swap_amount.clone()],
        );

        let mut messages = vec![swap_msg];

        if let Some(callback) = on_complete {
            let rebate_msg = BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: callback.execution_rebate,
            };

            messages.push(rebate_msg.into());
        }

        Ok(Response::new().add_messages(messages))
    }
}

#[cfg(test)]
mod find_pair_tests {
    use super::*;
    use cosmwasm_std::{testing::mock_dependencies, Addr};

    impl Default for Pair {
        fn default() -> Self {
            Pair {
                base_denom: "uruji".to_string(),
                quote_denom: "usdc".to_string(),
                address: Addr::unchecked("pair-address"),
            }
        }
    }

    #[test]
    fn saves_and_finds_pair() {
        let mut deps = mock_dependencies();
        let pair = Pair::default();

        save_pair(deps.as_mut().storage, &pair).unwrap();

        let mut denoms = pair.denoms();
        assert_eq!(pair, find_pair(&deps.storage, denoms.clone()).unwrap());
        denoms.reverse();
        assert_eq!(pair, find_pair(&deps.storage, denoms).unwrap());
    }

    #[test]
    fn find_pair_that_does_not_exist_fails() {
        let deps = mock_dependencies();

        let result = find_pair(&deps.storage, Pair::default().denoms()).unwrap_err();

        assert_eq!(
            result.to_string(),
            format!(
                "Generic error: No pair found for swapping from {} into {}",
                Pair::default().base_denom,
                Pair::default().quote_denom
            )
        );
    }
}

#[cfg(test)]
mod get_pairs_tests {
    use cosmwasm_std::{testing::mock_dependencies, Addr};

    use crate::exchanges::fin::Pair;

    use super::{get_pairs, save_pair};

    #[test]
    fn fetches_all_pairs() {
        let mut deps = mock_dependencies();

        for i in 0..10 {
            let pair = Pair {
                base_denom: format!("base_denom_{}", i),
                quote_denom: format!("quote_denom_{}", i),
                address: Addr::unchecked(format!("address_{}", i)),
            };

            save_pair(deps.as_mut().storage, &pair).unwrap();
        }

        let pairs = get_pairs(deps.as_ref().storage, None, None);

        assert_eq!(pairs.len(), 10);
    }

    #[test]
    fn fetches_all_pairs_with_limit() {
        let mut deps = mock_dependencies();

        for i in 0..10 {
            let pair = Pair {
                base_denom: format!("base_denom_{}", i),
                quote_denom: format!("quote_denom_{}", i),
                address: Addr::unchecked(format!("address_{}", i)),
            };

            save_pair(deps.as_mut().storage, &pair).unwrap();
        }

        let pairs = get_pairs(deps.as_ref().storage, None, Some(5));

        assert_eq!(pairs.len(), 5);
    }

    #[test]
    fn fetches_all_pairs_with_start_after() {
        let mut deps = mock_dependencies();

        for i in 0..10 {
            let pair = Pair {
                base_denom: format!("base_denom_{}", i),
                quote_denom: format!("quote_denom_{}", i),
                address: Addr::unchecked(format!("address_{}", i)),
            };

            save_pair(deps.as_mut().storage, &pair).unwrap();
        }

        let pairs = get_pairs(
            deps.as_ref().storage,
            Some(["base_denom_5".to_string(), "quote_denom_5".to_string()]),
            None,
        );

        assert_eq!(pairs.len(), 4);
        assert_eq!(pairs[0].base_denom, "base_denom_6");
    }

    #[test]
    fn fetches_all_pairs_with_start_after_and_limit() {
        let mut deps = mock_dependencies();

        for i in 0..10 {
            let pair = Pair {
                base_denom: format!("base_denom_{}", i),
                quote_denom: format!("quote_denom_{}", i),
                address: Addr::unchecked(format!("address_{}", i)),
            };

            save_pair(deps.as_mut().storage, &pair).unwrap();
        }

        let pairs = get_pairs(
            deps.as_ref().storage,
            Some(["base_denom_3".to_string(), "quote_denom_3".to_string()]),
            Some(2),
        );

        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].base_denom, "base_denom_4");
    }
}

#[cfg(test)]
mod can_swap_tests {

    use std::str::FromStr;

    use cosmwasm_std::{
        from_json, testing::mock_dependencies, to_json_binary, Addr, Coin, ContractResult, Decimal,
        SystemResult, Uint128, WasmQuery,
    };
    use rujira_rs::fin::{BookItemResponse, BookResponse, QueryMsg, SimulationResponse};

    use crate::{
        exchanges::fin::{save_pair, FinExchange, Pair},
        types::Exchange,
    };

    #[test]
    fn can_swap_with_existing_pair() {
        let mut deps = mock_dependencies();

        save_pair(
            deps.as_mut().storage,
            &Pair {
                base_denom: "uruji".to_string(),
                quote_denom: "usdc".to_string(),
                address: Addr::unchecked("pair-address"),
            },
        )
        .unwrap();

        let exchange = FinExchange::new();

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json::<QueryMsg>(msg).unwrap() {
                    QueryMsg::Simulate(_) => to_json_binary(&SimulationResponse {
                        returned: Uint128::new(300),
                        fee: Uint128::new(10),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.5").unwrap(),
                            total: Uint128::new(1000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.5").unwrap(),
                            total: Uint128::new(1000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!("Unexpected query type"),
                },
                _ => panic!("Unexpected query type"),
            }))
        });

        let swap_amount = Coin {
            denom: "uruji".to_string().clone(),
            amount: Uint128::new(100),
        };

        let minimum_receive_amount = Coin {
            denom: "usdc".to_string().clone(),
            amount: Uint128::new(50),
        };

        assert!(exchange
            .can_swap(deps.as_ref(), &swap_amount, &minimum_receive_amount, &None)
            .unwrap());
    }

    #[test]
    fn cannot_swap_with_non_existing_pair() {
        let deps = mock_dependencies();

        let exchange = FinExchange::new();

        let swap_amount = Coin {
            denom: "uruji".to_string().clone(),
            amount: Uint128::new(100),
        };

        let minimum_receive_amount = Coin {
            denom: "usdc".to_string().clone(),
            amount: Uint128::new(50),
        };

        assert!(!exchange
            .can_swap(deps.as_ref(), &swap_amount, &minimum_receive_amount, &None)
            .unwrap_or(false));
    }
}

#[cfg(test)]
mod route_tests {
    use std::str::FromStr;

    use cosmwasm_std::{
        from_json, testing::mock_dependencies, to_json_binary, Addr, Coin, ContractResult, Decimal,
        StdError, SystemResult, Uint128, WasmQuery,
    };
    use rujira_rs::{
        fin::{BookItemResponse, BookResponse, QueryMsg, SimulationResponse},
        NativeAsset,
    };

    use crate::{
        exchanges::fin::{save_pair, FinExchange, Pair},
        types::Exchange,
    };

    #[test]
    fn fails_to_get_route_with_non_existing_pair() {
        let deps = mock_dependencies();

        let exchange = FinExchange::new();
        let swap_amount = Coin {
            denom: "uruji".to_string(),
            amount: 100u128.into(),
        };
        let target_denom = NativeAsset::new("usdc");

        let result = exchange
            .path(deps.as_ref(), &swap_amount, &target_denom, &None)
            .unwrap_err();

        assert_eq!(
            result,
            StdError::generic_err("No pair found for swapping from uruji into usdc")
        );
    }

    #[test]
    fn gets_route_with_existing_pair() {
        let mut deps = mock_dependencies();

        let pair = Pair {
            base_denom: "uruji".to_string(),
            quote_denom: "usdc".to_string(),
            address: Addr::unchecked("pair-address"),
        };

        save_pair(deps.as_mut().storage, &pair).unwrap();

        let exchange = FinExchange::new();

        let swap_amount = Coin {
            denom: "uruji".to_string(),
            amount: 100u128.into(),
        };
        let target_denom = NativeAsset::new("usdc");

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json::<QueryMsg>(msg).unwrap() {
                    QueryMsg::Simulate(_) => to_json_binary(&SimulationResponse {
                        returned: Uint128::new(300),
                        fee: Uint128::new(10),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.5").unwrap(),
                            total: Uint128::new(1000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.5").unwrap(),
                            total: Uint128::new(1000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!("Unexpected query type"),
                },
                _ => panic!("Unexpected query type"),
            }))
        });

        let result = exchange
            .path(deps.as_ref(), &swap_amount, &target_denom, &None)
            .unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0], swap_amount);
        assert_eq!(
            result[1],
            Coin {
                denom: target_denom.denom_string(),
                amount: Uint128::new(300),
            }
        );
    }
}

#[cfg(test)]
mod expected_receive_amount_tests {
    use std::str::FromStr;

    use cosmwasm_std::{
        from_json, testing::mock_dependencies, to_json_binary, Addr, Coin, ContractResult, Decimal,
        StdError, SystemResult, Uint128, WasmQuery,
    };
    use rujira_rs::{
        fin::{BookItemResponse, BookResponse, QueryMsg, SimulationResponse},
        NativeAsset,
    };

    use crate::{
        exchanges::fin::{save_pair, FinExchange, Pair},
        types::Exchange,
    };

    #[test]
    fn fails_to_get_expected_receive_amount_from_non_existing_pair() {
        let deps = mock_dependencies();

        let swap_amount = Coin {
            denom: "uruji".to_string(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("usdc");

        let result = FinExchange::new()
            .expected_receive_amount(deps.as_ref(), &swap_amount, &target_denom, &None)
            .unwrap_err();

        assert_eq!(
            result,
            StdError::generic_err("No pair found for swapping from uruji into usdc")
        );
    }

    #[test]
    fn gets_expected_receive_amount() {
        let mut deps = mock_dependencies();

        let pair = Pair {
            base_denom: "uruji".to_string(),
            quote_denom: "usdc".to_string(),
            address: Addr::unchecked("pair-address"),
        };

        save_pair(deps.as_mut().storage, &pair).unwrap();

        let swap_amount = Coin {
            denom: "uruji".to_string(),
            amount: Uint128::new(100),
        };

        let target_denom = NativeAsset::new("usdc");

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json::<QueryMsg>(msg).unwrap() {
                    QueryMsg::Simulate(_) => to_json_binary(&SimulationResponse {
                        returned: Uint128::new(130),
                        fee: Uint128::new(10),
                    })
                    .unwrap(),
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.5").unwrap(),
                            total: Uint128::new(1000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.5").unwrap(),
                            total: Uint128::new(1000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!("Unexpected query type"),
                },
                _ => panic!("Unexpected query type"),
            }))
        });

        let expected_amount = FinExchange::new()
            .expected_receive_amount(deps.as_ref(), &swap_amount, &target_denom, &None)
            .unwrap();

        assert_eq!(
            expected_amount.receive_amount,
            Coin {
                denom: target_denom.denom_string(),
                amount: Uint128::new(130),
            }
        );

        assert_eq!(
            expected_amount.slippage_bps,
            Uint128::new(10_000)
                .mul_ceil(
                    Decimal::one()
                        - Decimal::from_ratio(
                            expected_amount.receive_amount.amount,
                            Uint128::new(150)
                        )
                )
                .into()
        );
    }
}

#[cfg(test)]
mod spot_price_tests {
    use std::str::FromStr;

    use cosmwasm_std::{
        from_json, testing::mock_dependencies, to_json_binary, Addr, ContractResult, Decimal,
        StdError, SystemResult, Uint128, WasmQuery,
    };
    use rujira_rs::{
        fin::{BookItemResponse, BookResponse, QueryMsg},
        NativeAsset,
    };

    use crate::{
        exchanges::fin::{save_pair, FinExchange, Pair},
        types::Exchange,
    };

    #[test]
    fn fails_to_get_spot_price_from_non_existing_pair() {
        let deps = mock_dependencies();

        let exchange = FinExchange::new();

        let swap_denom = NativeAsset::new("uruji");
        let target_denom = NativeAsset::new("usdc");

        let result = exchange
            .spot_price(deps.as_ref(), &swap_denom, &target_denom, &None)
            .unwrap_err();

        assert_eq!(
            result,
            StdError::generic_err("No pair found for swapping from uruji into usdc")
        );
    }

    #[test]
    fn gets_spot_prices_for_enter_and_exit_positions() {
        let mut deps = mock_dependencies();

        let base_denom = NativeAsset::new("uruji");
        let quote_denom = NativeAsset::new("usdc");

        let pair = Pair {
            base_denom: base_denom.denom_string(),
            quote_denom: quote_denom.denom_string(),
            address: Addr::unchecked("pair-address"),
        };

        save_pair(deps.as_mut().storage, &pair).unwrap();

        let exchange = FinExchange::new();

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json::<QueryMsg>(msg).unwrap() {
                    QueryMsg::Book { .. } => to_json_binary(&BookResponse {
                        base: vec![BookItemResponse {
                            price: Decimal::from_str("1.5").unwrap(),
                            total: Uint128::new(1000),
                        }],
                        quote: vec![BookItemResponse {
                            price: Decimal::from_str("1.5").unwrap(),
                            total: Uint128::new(1000),
                        }],
                    })
                    .unwrap(),
                    _ => panic!("Unexpected query type"),
                },
                _ => panic!("Unexpected query type"),
            }))
        });

        let enter_spot_price = exchange
            .spot_price(deps.as_ref(), &quote_denom, &base_denom, &None)
            .unwrap();

        assert_eq!(enter_spot_price, Decimal::from_str("1.5").unwrap());

        let exit_spot_price = exchange
            .spot_price(deps.as_ref(), &base_denom, &quote_denom, &None)
            .unwrap();

        assert_eq!(
            exit_spot_price,
            Decimal::one()
                .checked_div(Decimal::from_str("1.5").unwrap())
                .unwrap()
        );
    }
}

#[cfg(test)]
mod swap_tests {
    use std::str::FromStr;
    use std::vec;

    use calc_rs::types::{Contract, ContractError, Route};
    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::{testing::mock_dependencies, to_json_binary, Addr, Coin};
    use cosmwasm_std::{ContractResult, Decimal, MessageInfo, StdError, SystemResult, Uint128};
    use rujira_rs::fin::{ConfigResponse, Denoms, ExecuteMsg, SwapRequest, Tick};

    use crate::{
        exchanges::fin::{save_pair, FinExchange, Pair},
        types::Exchange,
    };

    #[test]
    fn fails_to_swap_with_non_existing_pair() {
        let deps = mock_dependencies();

        let swap_amount = Coin {
            denom: "uruji".to_string(),
            amount: Uint128::new(100),
        };

        let minimum_receive_amount = Coin {
            denom: "rune".to_string(),
            amount: Uint128::new(50),
        };

        let result = FinExchange::new()
            .swap(
                deps.as_ref(),
                &mock_env(),
                &MessageInfo {
                    sender: Addr::unchecked("sender-address"),
                    funds: vec![swap_amount.clone()],
                },
                &swap_amount,
                &minimum_receive_amount,
                &None,
                Addr::unchecked("recipient-address"),
                None,
            )
            .unwrap_err();

        assert_eq!(
            result,
            ContractError::Std(StdError::generic_err(format!(
                "No pair found for swapping from {} into {}",
                swap_amount.denom, minimum_receive_amount.denom
            )))
        );
    }

    #[test]
    fn fails_to_swap_with_non_fin_route() {
        let deps = mock_dependencies();

        let swap_amount = Coin {
            denom: "uruji".to_string(),
            amount: Uint128::new(100),
        };

        let minimum_receive_amount = Coin {
            denom: "rune".to_string(),
            amount: Uint128::new(50),
        };

        let result = FinExchange::new()
            .swap(
                deps.as_ref(),
                &mock_env(),
                &MessageInfo {
                    sender: Addr::unchecked("sender-address"),
                    funds: vec![swap_amount.clone()],
                },
                &swap_amount,
                &minimum_receive_amount,
                &Some(Route::Thorchain {}),
                Addr::unchecked("recipient-address"),
                None,
            )
            .unwrap_err();

        assert_eq!(
            result,
            ContractError::Std(StdError::generic_err(
                "Route not supported for Fin exchange"
            ))
        );
    }

    #[test]
    fn fails_to_swap_with_incorrect_route() {
        let mut deps = mock_dependencies();

        let base_denom = "uruji".to_string();
        let quote_denom = "usdc".to_string();

        let pair = Pair {
            base_denom: base_denom.clone(),
            quote_denom: quote_denom.clone(),
            address: Addr::unchecked("pair-address"),
        };

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&ConfigResponse {
                    denoms: Denoms::new(&"not-uruji", &quote_denom.clone()),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(10),
                    fee_taker: Decimal::from_str("0.01").unwrap(),
                    fee_maker: Decimal::from_str("0.01").unwrap(),
                    fee_address: "fee-address".to_string(),
                })
                .unwrap(),
            ))
        });

        let swap_amount = Coin {
            denom: pair.base_denom.clone(),
            amount: 100u128.into(),
        };

        let minimum_receive_amount = Coin {
            denom: pair.quote_denom.clone(),
            amount: 50u128.into(),
        };

        assert_eq!(
            FinExchange::new()
                .swap(
                    deps.as_ref(),
                    &mock_env(),
                    &MessageInfo {
                        sender: Addr::unchecked("sender-address"),
                        funds: vec![swap_amount.clone()],
                    },
                    &swap_amount,
                    &minimum_receive_amount,
                    &Some(Route::Fin {
                        address: pair.address.clone(),
                    }),
                    Addr::unchecked("recipient-address"),
                    None,
                )
                .unwrap_err(),
            ContractError::Std(StdError::generic_err(format!(
                "Pair at {} does not support swapping {}",
                pair.address, pair.base_denom
            )))
        );
    }

    #[test]
    fn swaps_with_existing_pair() {
        let mut deps = mock_dependencies();

        let pair = Pair {
            base_denom: "uruji".to_string(),
            quote_denom: "usdc".to_string(),
            address: Addr::unchecked("pair-address"),
        };

        save_pair(deps.as_mut().storage, &pair).unwrap();

        let exchange = FinExchange::new();

        let swap_amount = Coin {
            denom: "uruji".to_string(),
            amount: 100u128.into(),
        };

        let minimum_receive_amount = Coin {
            denom: "usdc".to_string(),
            amount: 50u128.into(),
        };

        let recipient = Addr::unchecked("recipient-address");

        let response = exchange
            .swap(
                deps.as_ref(),
                &mock_env(),
                &MessageInfo {
                    sender: Addr::unchecked("sender-address"),
                    funds: vec![swap_amount.clone()],
                },
                &swap_amount,
                &minimum_receive_amount,
                &None,
                recipient.clone(),
                None,
            )
            .unwrap();

        assert_eq!(response.messages.len(), 1);
        assert_eq!(
            response.messages[0].msg,
            Contract(pair.address).call(
                to_json_binary(&ExecuteMsg::Swap(SwapRequest {
                    min_return: Some(minimum_receive_amount.amount),
                    to: Some(recipient.to_string()),
                    callback: None,
                }))
                .unwrap(),
                vec![swap_amount.clone()]
            )
        );
    }

    #[test]
    fn swaps_with_provided_route() {
        let mut deps = mock_dependencies();

        let base_denom = "uruji".to_string();
        let quote_denom = "usdc".to_string();

        let pair = Pair {
            base_denom: base_denom.clone(),
            quote_denom: quote_denom.clone(),
            address: Addr::unchecked("pair-address"),
        };

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&ConfigResponse {
                    denoms: Denoms::new(&base_denom.clone(), &quote_denom.clone()),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(10),
                    fee_taker: Decimal::from_str("0.01").unwrap(),
                    fee_maker: Decimal::from_str("0.01").unwrap(),
                    fee_address: "fee-address".to_string(),
                })
                .unwrap(),
            ))
        });

        let swap_amount = Coin {
            denom: pair.base_denom.clone(),
            amount: 100u128.into(),
        };

        let minimum_receive_amount = Coin {
            denom: pair.quote_denom.clone(),
            amount: 50u128.into(),
        };

        let recipient = Addr::unchecked("recipient-address");

        let response = FinExchange::new()
            .swap(
                deps.as_ref(),
                &mock_env(),
                &MessageInfo {
                    sender: Addr::unchecked("sender-address"),
                    funds: vec![swap_amount.clone()],
                },
                &swap_amount,
                &minimum_receive_amount,
                &Some(Route::Fin {
                    address: pair.address.clone(),
                }),
                recipient.clone(),
                None,
            )
            .unwrap();

        assert_eq!(response.messages.len(), 1);
        assert_eq!(
            response.messages[0].msg,
            Contract(pair.address).call(
                to_json_binary(&ExecuteMsg::Swap(SwapRequest {
                    min_return: Some(minimum_receive_amount.amount),
                    to: Some(recipient.to_string()),
                    callback: None,
                }))
                .unwrap(),
                vec![swap_amount.clone()]
            )
        );
    }
}
