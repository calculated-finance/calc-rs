use calc_rs::types::{
    Callback, ContractResult, ExchangeExecuteMsg, ExchangeQueryMsg, ExpectedReceiveAmount, Route,
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Addr, Binary, Coin, Decimal, Deps, DepsMut, Env, MessageInfo, Reply, Response,
    StdError, StdResult, Uint128,
};
use rujira_rs::NativeAsset;

use crate::exchanges::fin::{delete_pair, save_pair, FinExchange, Pair};
use crate::exchanges::thorchain::{ThorchainExchange, SCHEDULER};
use crate::types::Exchange;

#[cw_serde]
pub struct InstantiateMsg {
    scheduler_address: Addr,
}

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> ContractResult {
    deps.api.addr_validate(msg.scheduler_address.as_str())?;
    SCHEDULER.save(deps.storage, &msg.scheduler_address)?;
    Ok(Response::default())
}

#[cw_serde]
pub struct MigrateMsg {}

#[entry_point]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, StdError> {
    Ok(Response::default())
}

#[cw_serde]
pub enum SudoMsg {
    CreatePairs { pairs: Vec<Pair> },
    DeletePairs { pairs: Vec<Pair> },
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(deps: DepsMut, _env: Env, msg: SudoMsg) -> ContractResult {
    match msg {
        SudoMsg::CreatePairs { pairs } => {
            for pair in pairs {
                save_pair(deps.storage, &pair)?;
            }
            Ok(Response::default())
        }
        SudoMsg::DeletePairs { pairs } => {
            for pair in pairs {
                delete_pair(deps.storage, &pair);
            }
            Ok(Response::default())
        }
    }
}

#[cfg(not(feature = "library"))]
pub fn get_exchanges(deps: Deps) -> Vec<Box<dyn Exchange>> {
    vec![
        Box::new(FinExchange::new()),
        Box::new(ThorchainExchange::new(deps)),
    ]
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: ExchangeQueryMsg) -> StdResult<Binary> {
    let exchanges = get_exchanges(deps);
    match msg {
        ExchangeQueryMsg::CanSwap {
            swap_amount,
            minimum_receive_amount,
            route,
        } => to_json_binary(&can_swap(
            exchanges,
            deps,
            &swap_amount,
            &minimum_receive_amount,
            &route,
        )),
        ExchangeQueryMsg::Path {
            swap_amount,
            target_denom,
            route,
        } => to_json_binary(&path(exchanges, deps, &swap_amount, target_denom, &route)?),
        ExchangeQueryMsg::ExpectedReceiveAmount {
            swap_amount,
            target_denom,
            route,
        } => to_json_binary(&expected_receive_amount(
            exchanges,
            deps,
            &swap_amount,
            target_denom,
            &route,
        )?),
        ExchangeQueryMsg::SpotPrice {
            swap_denom,
            target_denom,
            route,
        } => to_json_binary(&spot_price(
            exchanges,
            deps,
            swap_denom,
            target_denom,
            &route,
        )?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExchangeExecuteMsg,
) -> ContractResult {
    let exchanges = get_exchanges(deps.as_ref());
    match msg {
        ExchangeExecuteMsg::Swap {
            minimum_receive_amount,
            route,
            recipient,
            on_complete,
        } => swap(
            exchanges,
            deps.as_ref(),
            env,
            info,
            &minimum_receive_amount,
            &route,
            recipient,
            on_complete,
        ),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, reply: Reply) -> ContractResult {
    Ok(Response::default()
        .add_attribute("action", "reply")
        .add_attribute("payload", format!("{:?}", reply)))
}

fn can_swap(
    exchanges: Vec<Box<dyn Exchange>>,
    deps: Deps,
    swap_amount: &Coin,
    minimum_receive_amount: &Coin,
    route: &Option<Route>,
) -> bool {
    exchanges.iter().any(|e| {
        e.can_swap(deps, swap_amount, minimum_receive_amount, route)
            .unwrap_or(false)
    })
}

fn path(
    exchanges: Vec<Box<dyn Exchange>>,
    deps: Deps,
    swap_amount: &Coin,
    target_denom: String,
    route: &Option<Route>,
) -> StdResult<Vec<Coin>> {
    let path = exchanges
        .iter()
        .filter(|e| {
            e.can_swap(
                deps,
                &swap_amount,
                &Coin {
                    denom: target_denom.clone(),
                    amount: Uint128::one(),
                },
                route,
            )
            .unwrap_or(false)
        })
        .flat_map(|e| {
            e.path(deps, &swap_amount, &NativeAsset::new(&target_denom), route)
                .ok()
        }) // TODO: pull this out so that we can return a good error message without iterating again
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
        .unwrap_or_else(|| vec![]);

    if path.is_empty() {
        return Err(StdError::generic_err(format!(
            "Unable to find a path for swapping {} to {}. Errors: [{}]",
            swap_amount.denom,
            target_denom,
            exchanges
                .iter()
                .flat_map(|e| {
                    e.path(deps, &swap_amount, &NativeAsset::new(&target_denom), route)
                        .err()
                        .map(|e| e.to_string())
                })
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    Ok(path)
}

fn expected_receive_amount(
    exchanges: Vec<Box<dyn Exchange>>,
    deps: Deps,
    swap_amount: &Coin,
    target_denom: String,
    route: &Option<Route>,
) -> StdResult<ExpectedReceiveAmount> {
    exchanges
        .iter()
        .filter(|e| {
            e.can_swap(
                deps,
                &swap_amount,
                &Coin {
                    denom: target_denom.clone(),
                    amount: Uint128::one(),
                },
                route,
            )
            .unwrap_or(false)
        })
        .flat_map(|e| {
            e.expected_receive_amount(deps, &swap_amount, &NativeAsset::new(&target_denom), route)
                .ok()
        })
        .max_by(|a, b| a.receive_amount.amount.cmp(&b.receive_amount.amount))
        .map_or_else(
            || {
                Err(StdError::generic_err(format!(
                    "Unable to find a path for swapping {} to {}",
                    swap_amount.denom, target_denom
                )))
            },
            |amount| Ok(amount),
        )
}

fn spot_price(
    exchanges: Vec<Box<dyn Exchange>>,
    deps: Deps,
    swap_denom: String,
    target_denom: String,
    route: &Option<Route>,
) -> StdResult<Decimal> {
    exchanges
        .iter()
        .flat_map(|e| {
            e.spot_price(
                deps,
                &NativeAsset::new(&swap_denom),
                &NativeAsset::new(&target_denom),
                route,
            )
            .ok()
        })
        .min_by(|a, b| a.cmp(&b))
        .map_or_else(
            || {
                Err(StdError::generic_err(format!(
                    "Unable to find a path for swapping {} to {}",
                    swap_denom, target_denom
                )))
            },
            |amount| Ok(amount),
        )
}

fn swap(
    exchanges: Vec<Box<dyn Exchange>>,
    deps: Deps,
    env: Env,
    info: MessageInfo,
    minimum_receive_amount: &Coin,
    route: &Option<Route>,
    recipient: Option<Addr>,
    on_complete: Option<Callback>,
) -> ContractResult {
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
            e.can_swap(deps, &swap_amount, &minimum_receive_amount, route)
                .unwrap_or(false)
        })
        .max_by(|a, b| {
            a.expected_receive_amount(deps, &swap_amount, &NativeAsset::new(&target_denom), route)
                .expect(
                    format!(
                        "Failed to get expected receive amount for {} to {}",
                        swap_amount.denom, target_denom
                    )
                    .as_str(),
                )
                .receive_amount
                .amount
                .cmp(
                    &b.expected_receive_amount(
                        deps,
                        &swap_amount,
                        &NativeAsset::new(&target_denom),
                        route,
                    )
                    .expect(
                        format!(
                            "Failed to get expected receive amount for {} to {}",
                            swap_amount.denom, target_denom
                        )
                        .as_str(),
                    )
                    .receive_amount
                    .amount,
                )
        });

    match best_exchange {
        Some(exchange) => exchange.swap(
            deps,
            &env,
            &info,
            &swap_amount,
            &minimum_receive_amount,
            route,
            recipient.unwrap_or(info.sender.clone()),
            on_complete,
        ),
        None => Err(StdError::generic_err(format!(
            "Unable to find a path for swapping {} to {}",
            swap_amount.denom, target_denom
        ))
        .into()),
    }
}

#[cfg(test)]
mod can_swap_tests {
    use cosmwasm_std::{testing::mock_dependencies, Coin, StdError, Uint128};

    use crate::{contract::can_swap, exchanges::mock::MockExchange};

    #[test]
    fn returns_false_when_no_exchange_can_swap() {
        let mut mock = Box::new(MockExchange::default());
        mock.can_swap_fn = Box::new(|_, _, _, _| Ok(false));

        assert_eq!(
            can_swap(
                vec![mock],
                mock_dependencies().as_ref(),
                &Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(1000)
                },
                &Coin {
                    denom: "uruji".to_string(),
                    amount: Uint128::new(100)
                },
                &None,
            ),
            false
        );
    }

    #[test]
    fn returns_true_when_one_exchange_can_swap() {
        let mut mock_false = Box::new(MockExchange::default());
        mock_false.can_swap_fn = Box::new(|_, _, _, _| Ok(false));

        assert_eq!(
            can_swap(
                vec![mock_false, Box::new(MockExchange::default())],
                mock_dependencies().as_ref(),
                &Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(1000)
                },
                &Coin {
                    denom: "uruji".to_string(),
                    amount: Uint128::new(100)
                },
                &None,
            ),
            true
        );

        let mut mock_error = Box::new(MockExchange::default());
        mock_error.can_swap_fn =
            Box::new(|_, _, _, _| Err(StdError::generic_err("Not enough liquidity")));

        assert_eq!(
            can_swap(
                vec![mock_error, Box::new(MockExchange::default())],
                mock_dependencies().as_ref(),
                &Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(1000)
                },
                &Coin {
                    denom: "uruji".to_string(),
                    amount: Uint128::new(100)
                },
                &None,
            ),
            true
        );
    }

    #[test]
    fn returns_true_when_all_exchanges_can_swap() {
        assert_eq!(
            can_swap(
                vec![
                    Box::new(MockExchange::default()),
                    Box::new(MockExchange::default()),
                ],
                mock_dependencies().as_ref(),
                &Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(1000)
                },
                &Coin {
                    denom: "uruji".to_string(),
                    amount: Uint128::new(100)
                },
                &None,
            ),
            true
        );
    }
}

#[cfg(test)]
mod path_tests {
    use cosmwasm_std::{testing::mock_dependencies, Coin, StdError, Uint128};

    use crate::{contract::path, exchanges::mock::MockExchange};

    #[test]
    fn returns_error_when_no_path_found() {
        let mut mock = Box::new(MockExchange::default());
        mock.path_fn = Box::new(|_, _, _, _| Err(StdError::generic_err("Not enough liquidity")));

        assert_eq!(
            path(
                vec![mock],
                mock_dependencies().as_ref(),
                &Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(100)
                },
                "uruji".to_string(),
                &None,
            )
            .unwrap_err(),
            StdError::generic_err(
                "Unable to find a path for swapping rune to uruji. Errors: [Generic error: Not enough liquidity]"
            )
        )
    }

    #[test]
    fn returns_route_from_one_exchange() {
        let swap_amount = Coin {
            denom: "rune".to_string(),
            amount: Uint128::new(100),
        };

        let target_denom = "uruji".to_string();

        assert_eq!(
            path(
                vec![Box::new(MockExchange::default())],
                mock_dependencies().as_ref(),
                &swap_amount,
                target_denom.clone(),
                &None
            )
            .unwrap(),
            vec![
                swap_amount.clone(),
                Coin {
                    denom: target_denom,
                    amount: swap_amount.amount
                }
            ]
        )
    }

    #[test]
    fn returns_best_route_from_multiple_exchanges() {
        let swap_amount = Coin {
            denom: "rune".to_string(),
            amount: Uint128::new(100),
        };

        let target_denom = "uruji".to_string();

        let receive_amount = Coin {
            denom: target_denom.clone(),
            amount: swap_amount.amount.clone() * Uint128::new(2),
        };

        let expected_route = vec![swap_amount.clone(), receive_amount.clone()];

        let mut mock = Box::new(MockExchange::default());
        mock.path_fn = Box::new(move |_, _, _, _| Ok(expected_route.clone()));

        assert_eq!(
            path(
                vec![mock, Box::new(MockExchange::default())],
                mock_dependencies().as_ref(),
                &swap_amount,
                target_denom,
                &None,
            )
            .unwrap(),
            vec![swap_amount, receive_amount]
        )
    }
}

#[cfg(test)]
mod expected_receive_amount_tests {
    use crate::{contract::expected_receive_amount, exchanges::mock::MockExchange};
    use calc_rs::types::ExpectedReceiveAmount;
    use cosmwasm_std::{testing::mock_dependencies, Coin, StdError, Uint128};

    #[test]
    fn returns_error_when_no_exchange_can_swap() {
        let mut mock = Box::new(MockExchange::default());
        mock.get_expected_receive_amount_fn =
            Box::new(|_, _, _, _| Err(StdError::generic_err("Not enough liquidity")));

        assert_eq!(
            expected_receive_amount(
                vec![mock],
                mock_dependencies().as_ref(),
                &Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(1000)
                },
                "uruji".to_string(),
                &None,
            )
            .unwrap_err(),
            StdError::generic_err("Unable to find a path for swapping rune to uruji")
        );
    }

    #[test]
    fn returns_expected_amount_from_one_exchange() {
        let swap_amount = Coin {
            denom: "rune".to_string(),
            amount: Uint128::new(1000),
        };

        let target_denom = "uruji".to_string();

        let receive_amount = Coin {
            denom: target_denom.clone(),
            amount: Uint128::new(2000),
        };

        let slippage_bps = 100;

        let expected_response = ExpectedReceiveAmount {
            receive_amount: receive_amount.clone(),
            slippage_bps,
        };

        let mut mock = Box::new(MockExchange::default());
        mock.get_expected_receive_amount_fn =
            Box::new(move |_, _, _, _| Ok(expected_response.clone()));

        assert_eq!(
            expected_receive_amount(
                vec![mock],
                mock_dependencies().as_ref(),
                &swap_amount,
                target_denom,
                &None,
            )
            .unwrap(),
            ExpectedReceiveAmount {
                receive_amount,
                slippage_bps,
            }
        );
    }

    #[test]
    fn returns_best_expected_amount_from_multiple_exchanges() {
        let swap_amount = Coin {
            denom: "rune".to_string(),
            amount: Uint128::new(1000),
        };

        let target_denom = "uruji".to_string();

        let receive_amount = Coin {
            denom: target_denom.clone(),
            amount: Uint128::new(2000),
        };

        let slippage_bps = 100;

        let expected_response = ExpectedReceiveAmount {
            receive_amount: receive_amount.clone(),
            slippage_bps,
        };

        let mut mock = Box::new(MockExchange::default());
        mock.get_expected_receive_amount_fn =
            Box::new(move |_, _, _, _| Ok(expected_response.clone()));

        assert_eq!(
            expected_receive_amount(
                vec![mock, Box::new(MockExchange::default())],
                mock_dependencies().as_ref(),
                &swap_amount,
                target_denom.clone(),
                &None,
            )
            .unwrap(),
            ExpectedReceiveAmount {
                receive_amount,
                slippage_bps,
            }
        );
    }
}

#[cfg(test)]
mod spot_price_tests {
    use std::str::FromStr;

    use crate::{contract::spot_price, exchanges::mock::MockExchange};
    use cosmwasm_std::{testing::mock_dependencies, Decimal, StdError};

    #[test]
    fn returns_error_when_no_exchange_can_swap() {
        let mut mock = Box::new(MockExchange::default());
        mock.get_spot_price_fn =
            Box::new(|_, _, _, _| Err(StdError::generic_err("Not enough liquidity")));

        assert_eq!(
            spot_price(
                vec![mock],
                mock_dependencies().as_ref(),
                "rune".to_string(),
                "uruji".to_string(),
                &None,
            )
            .unwrap_err(),
            StdError::generic_err("Unable to find a path for swapping rune to uruji")
        );
    }

    #[test]
    fn returns_spot_price_from_one_exchange() {
        let swap_denom = "rune".to_string();
        let target_denom = "uruji".to_string();

        let expected_spot_price = Decimal::from_str("1.5").unwrap();

        let mut mock = Box::new(MockExchange::default());
        mock.get_spot_price_fn = Box::new(move |_, _, _, _| Ok(expected_spot_price.clone()));

        assert_eq!(
            spot_price(
                vec![mock],
                mock_dependencies().as_ref(),
                swap_denom.clone(),
                target_denom.clone(),
                &None,
            )
            .unwrap(),
            expected_spot_price
        );
    }

    #[test]
    fn returns_best_spot_price_from_multiple_exchanges() {
        let swap_denom = "rune".to_string();
        let target_denom = "uruji".to_string();

        let expected_spot_price = Decimal::from_str("2.0").unwrap();

        let mut mock = Box::new(MockExchange::default());
        mock.get_spot_price_fn = Box::new(move |_, _, _, _| Ok(expected_spot_price.clone()));

        let deps = mock_dependencies();

        assert_eq!(
            spot_price(
                vec![mock, Box::new(MockExchange::default())],
                deps.as_ref(),
                swap_denom.clone(),
                target_denom.clone(),
                &None,
            )
            .unwrap(),
            spot_price(
                vec![Box::new(MockExchange::default())],
                deps.as_ref(),
                swap_denom.clone(),
                target_denom.clone(),
                &None,
            )
            .unwrap(),
        );
    }
}

#[cfg(test)]
mod swap_tests {
    use calc_rs::types::ExpectedReceiveAmount;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Addr, Coin, MessageInfo, Response, Uint128,
    };
    use rujira_rs::NativeAsset;

    use crate::{contract::swap, exchanges::mock::MockExchange, types::Exchange};

    #[test]
    fn returns_error_when_no_exchange_can_swap() {
        let mut mock = Box::new(MockExchange::default());
        mock.can_swap_fn = Box::new(|_, _, _, _| Ok(false));

        let swap_amount = Coin {
            denom: "rune".to_string(),
            amount: Uint128::new(1000),
        };

        let minimum_receive_amount = Coin {
            denom: "uruji".to_string(),
            amount: Uint128::new(100),
        };

        assert_eq!(
            swap(
                vec![mock],
                mock_dependencies().as_ref(),
                mock_env(),
                MessageInfo {
                    sender: Addr::unchecked("sender"),
                    funds: vec![swap_amount.clone()],
                },
                &minimum_receive_amount,
                &None,
                None,
                None
            )
            .unwrap_err()
            .to_string(),
            format!(
                "Generic error: Unable to find a path for swapping {} to {}",
                swap_amount.denom, minimum_receive_amount.denom
            )
        );
    }

    #[test]
    fn swaps_when_one_exchange_can_swap() {
        let mut mock = Box::new(MockExchange::default());
        mock.can_swap_fn = Box::new(|_, _, _, _| Ok(false));

        assert_eq!(
            swap(
                vec![mock, Box::new(MockExchange::default())],
                mock_dependencies().as_ref(),
                mock_env(),
                MessageInfo {
                    sender: Addr::unchecked("sender"),
                    funds: vec![Coin {
                        denom: "rune".to_string(),
                        amount: Uint128::new(100)
                    }],
                },
                &Coin {
                    denom: "uruji".to_string(),
                    amount: Uint128::new(100)
                },
                &None,
                None,
                None
            )
            .unwrap(),
            Response::default()
        );
    }

    #[test]
    fn swaps_when_all_exchanges_can_swap() {
        assert_eq!(
            swap(
                vec![
                    Box::new(MockExchange::default()),
                    Box::new(MockExchange::default()),
                ],
                mock_dependencies().as_ref(),
                mock_env(),
                MessageInfo {
                    sender: Addr::unchecked("sender"),
                    funds: vec![Coin {
                        denom: "rune".to_string(),
                        amount: Uint128::new(100)
                    }],
                },
                &Coin {
                    denom: "uruji".to_string(),
                    amount: Uint128::new(100)
                },
                &None,
                None,
                None
            )
            .unwrap(),
            Response::default()
        );
    }

    #[test]
    fn selects_best_exchange_for_swap() {
        let swap_amount = Coin {
            denom: "rune".to_string(),
            amount: Uint128::new(1000),
        };

        let minimum_receive_amount = Coin {
            denom: "uruji".to_string(),
            amount: Uint128::new(100),
        };

        let deps = mock_dependencies();

        let expected_response = MockExchange::default()
            .expected_receive_amount(
                deps.as_ref(),
                &swap_amount.clone(),
                &NativeAsset::new(&minimum_receive_amount.denom.clone()),
                &None,
            )
            .unwrap();

        let mut mock = Box::new(MockExchange::default());

        mock.get_expected_receive_amount_fn = Box::new(move |_, _, _, _| {
            Ok(ExpectedReceiveAmount {
                receive_amount: Coin {
                    denom: expected_response.receive_amount.denom.clone(),
                    amount: expected_response.receive_amount.amount * Uint128::new(2),
                },
                slippage_bps: expected_response.slippage_bps,
            })
        });

        mock.swap_fn = Box::new(move |_, _, _, _, _, _, _, _| {
            Ok(Response::default().add_attribute("action", "test-swap"))
        });

        assert_eq!(
            swap(
                vec![mock, Box::new(MockExchange::default())],
                deps.as_ref(),
                mock_env(),
                MessageInfo {
                    sender: Addr::unchecked("sender"),
                    funds: vec![swap_amount.clone()],
                },
                &minimum_receive_amount,
                &None,
                None,
                None
            )
            .unwrap(),
            Response::default().add_attribute("action", "test-swap")
        );
    }
}
