use calc_rs::core::{Callback, ContractResult};
use calc_rs::exchanger::{ExchangeExecuteMsg, ExchangeQueryMsg, ExpectedReceiveAmount, Route};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Addr, Binary, Coin, Coins, Deps, DepsMut, Env, MessageInfo, Reply, Response,
    StdError, StdResult, Uint128,
};

use crate::exchanges::{fin_market::FinMarketExchange, thorchain::ThorchainExchange};

use crate::state::CONFIG;
use crate::types::{Exchange, ExchangeConfig};

#[cw_serde]
pub struct InstantiateMsg {
    scheduler_address: Addr,
    affiliate_code: Option<String>,
    affiliate_bps: Option<u64>,
}

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> ContractResult {
    CONFIG.save(
        deps,
        ExchangeConfig {
            scheduler_address: msg.scheduler_address,
            affiliate_code: None,
            affiliate_bps: None,
        },
    )?;

    Ok(Response::default())
}

#[cw_serde]
pub struct MigrateMsg {
    scheduler_address: Addr,
    affiliate_code: Option<String>,
    affiliate_bps: Option<u64>,
}

#[entry_point]
pub fn migrate(deps: DepsMut, _env: Env, msg: MigrateMsg) -> Result<Response, StdError> {
    CONFIG.save(
        deps,
        ExchangeConfig {
            scheduler_address: msg.scheduler_address,
            affiliate_code: msg.affiliate_code,
            affiliate_bps: msg.affiliate_bps,
        },
    )?;

    Ok(Response::default())
}

#[cfg(not(feature = "library"))]
pub fn get_exchanges(deps: Deps) -> StdResult<Vec<Box<dyn Exchange>>> {
    let config = CONFIG.load(deps)?;
    Ok(vec![
        Box::new(FinMarketExchange::new()),
        Box::new(ThorchainExchange::new(config)),
    ])
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: ExchangeQueryMsg) -> StdResult<Binary> {
    let exchanges = get_exchanges(deps)?;
    match msg {
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
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExchangeExecuteMsg,
) -> ContractResult {
    let exchanges = get_exchanges(deps.as_ref())?;
    match msg {
        ExchangeExecuteMsg::Swap {
            minimum_receive_amount,
            maximum_slippage_bps,
            route,
            recipient,
            on_complete,
        } => swap(
            exchanges,
            deps.as_ref(),
            env,
            info,
            &minimum_receive_amount,
            maximum_slippage_bps,
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

fn expected_receive_amount(
    exchanges: Vec<Box<dyn Exchange>>,
    deps: Deps,
    swap_amount: &Coin,
    target_denom: String,
    route: &Option<Route>,
) -> StdResult<ExpectedReceiveAmount> {
    exchanges
        .iter()
        .flat_map(|e| {
            e.expected_receive_amount(deps, &swap_amount, &target_denom, route)
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

fn swap(
    exchanges: Vec<Box<dyn Exchange>>,
    deps: Deps,
    env: Env,
    info: MessageInfo,
    minimum_receive_amount: &Coin,
    maximum_slippage_bps: u128,
    route: &Option<Route>,
    recipient: Option<Addr>,
    on_complete: Option<Callback>,
) -> ContractResult {
    let mut funds = Coins::try_from(info.funds.clone())?;

    if let Some(on_complete) = on_complete.clone() {
        for rebate in on_complete.execution_rebate.into_iter() {
            funds.sub(rebate.clone()).map_err(|_| {
                StdError::generic_err(format!(
                    "Execution rebate amount not included in provided funds: {:#?}",
                    rebate
                ))
            })?;
        }
    }

    if funds.len() != 1 {
        return Err(StdError::generic_err("Must provide exactly one coin to swap").into());
    }

    let swap_amount = funds.to_vec()[0].clone();

    if swap_amount.amount.is_zero() {
        return Err(StdError::generic_err("Must provide a non-zero amount to swap").into());
    }

    let target_denom = minimum_receive_amount.denom.clone();

    let best_exchange = exchanges
        .iter()
        .map(|exchange| {
            (
                exchange,
                exchange
                    .expected_receive_amount(deps, &swap_amount, &target_denom, route)
                    .unwrap_or(ExpectedReceiveAmount {
                        receive_amount: Coin::new(0u128, target_denom.clone()),
                        slippage_bps: 10_000,
                    }),
            )
        })
        .filter(|(_, result)| result.receive_amount.amount > Uint128::zero())
        .max_by(|(_, a), (_, b)| a.receive_amount.amount.cmp(&b.receive_amount.amount));

    match best_exchange {
        Some((exchange, _)) => exchange.swap(
            deps,
            &env,
            &info,
            &swap_amount,
            &minimum_receive_amount,
            maximum_slippage_bps,
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
mod expected_receive_amount_tests {
    use crate::{contract::expected_receive_amount, exchanges::mock::MockExchange};
    use calc_rs::exchanger::ExpectedReceiveAmount;
    use cosmwasm_std::{testing::mock_dependencies, Coin, StdError};

    #[test]
    fn returns_error_when_no_exchange_can_swap() {
        let mut mock = Box::new(MockExchange::default());
        mock.get_expected_receive_amount_fn =
            Box::new(|_, _, _, _| Err(StdError::generic_err("Not enough liquidity")));

        assert_eq!(
            expected_receive_amount(
                vec![mock],
                mock_dependencies().as_ref(),
                &Coin::new(1000u128, "rune"),
                "uruji".to_string(),
                &None,
            )
            .unwrap_err(),
            StdError::generic_err("Unable to find a path for swapping rune to uruji")
        );
    }

    #[test]
    fn returns_expected_amount_from_one_exchange() {
        let swap_amount = &Coin::new(1000u128, "rune");
        let target_denom = "uruji".to_string();
        let receive_amount = Coin::new(2000u128, target_denom.clone());
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
        let swap_amount = &Coin::new(1000u128, "rune");
        let target_denom = "uruji".to_string();
        let receive_amount = Coin::new(2000u128, target_denom.clone());
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
mod swap_tests {
    use calc_rs::{
        core::{Callback, ContractError},
        exchanger::ExpectedReceiveAmount,
    };
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Addr, Binary, Coin, MessageInfo, Response, Uint128,
    };

    use crate::{contract::swap, exchanges::mock::MockExchange, types::Exchange};

    #[test]
    fn returns_error_when_no_exchange_can_swap() {
        let mut mock = Box::new(MockExchange::default());

        let swap_amount = &Coin::new(1000u128, "rune");
        let minimum_receive_amount = Coin::new(100u128, "uruji");

        mock.swap_fn = Box::new(move |_, _, _, _, _, _, _, _, _| {
            Err(ContractError::generic_err(format!(
                "Unable to find a path for swapping rune to uruji",
            )))
        });

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
                0,
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
    fn returns_error_when_execution_rebate_not_included_in_funds() {
        let swap_amount = Coin::new(1000u128, "rune");
        let minimum_receive_amount = Coin::new(100u128, "rune");
        let execution_rebate = Coin::new(123u128, "test");

        assert_eq!(
            swap(
                vec![Box::new(MockExchange::default())],
                mock_dependencies().as_ref(),
                mock_env(),
                MessageInfo {
                    sender: Addr::unchecked("sender"),
                    funds: vec![swap_amount.clone()],
                },
                &minimum_receive_amount,
                0,
                &None,
                None,
                Some(calc_rs::core::Callback {
                    contract: Addr::unchecked("callback"),
                    msg: Binary::default(),
                    execution_rebate: vec![execution_rebate.clone()],
                })
            )
            .unwrap_err()
            .to_string(),
            format!(
                "Generic error: Execution rebate amount not included in provided funds: {:#?}",
                execution_rebate
            )
        );
    }

    #[test]
    fn swaps_when_one_exchange_can_swap() {
        let mut mock = Box::new(MockExchange::default());

        let minimum_receive_amount = Coin::new(100u128, "uruji");

        mock.get_expected_receive_amount_fn = Box::new(|_, _, _, _| {
            Ok(ExpectedReceiveAmount {
                receive_amount: Coin::new(100u128, "uruji"),
                slippage_bps: 0,
            })
        });

        assert_eq!(
            swap(
                vec![mock, Box::new(MockExchange::default())],
                mock_dependencies().as_ref(),
                mock_env(),
                MessageInfo {
                    sender: Addr::unchecked("sender"),
                    funds: vec![Coin::new(100u128, "rune")],
                },
                &minimum_receive_amount,
                0,
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
                    funds: vec![Coin::new(100u128, "rune")],
                },
                &Coin::new(100u128, "uruji"),
                0,
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
        let deps = mock_dependencies();

        let swap_amount = Coin::new(1000u128, "rune");
        let minimum_receive_amount = Coin::new(100u128, "uruji");

        let expected_response = MockExchange::default()
            .expected_receive_amount(
                deps.as_ref(),
                &swap_amount.clone(),
                &minimum_receive_amount.denom.clone(),
                &None,
            )
            .unwrap();

        let mut mock = Box::new(MockExchange::default());

        mock.get_expected_receive_amount_fn = Box::new(move |_, _, _, _| {
            Ok(ExpectedReceiveAmount {
                receive_amount: Coin::new(
                    expected_response.receive_amount.amount * Uint128::new(2),
                    expected_response.receive_amount.denom.clone(),
                ),
                slippage_bps: expected_response.slippage_bps,
            })
        });

        mock.swap_fn = Box::new(move |_, _, _, _, _, _, _, _, _| {
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
                0,
                &None,
                None,
                None
            )
            .unwrap(),
            Response::default().add_attribute("action", "test-swap")
        );
    }

    #[test]
    fn swaps_with_execution_rebate_included() {
        let mut mock = Box::new(MockExchange::default());

        let minimum_receive_amount = Coin::new(100u128, "uruji");

        mock.get_expected_receive_amount_fn = Box::new(|_, _, _, _| {
            Ok(ExpectedReceiveAmount {
                receive_amount: Coin::new(101u128, "uruji"),
                slippage_bps: 0,
            })
        });

        mock.swap_fn = Box::new(move |_, _, _, _, _, _, _, _, _| {
            Ok(Response::default().add_attribute("action", "rebate-swap"))
        });

        let execution_rebate = vec![Coin::new(123u128, "test")];

        assert_eq!(
            swap(
                vec![mock],
                mock_dependencies().as_ref(),
                mock_env(),
                MessageInfo {
                    sender: Addr::unchecked("sender"),
                    funds: vec![Coin::new(100u128, "rune"), execution_rebate[0].clone()],
                },
                &minimum_receive_amount,
                0,
                &None,
                None,
                Some(Callback {
                    contract: Addr::unchecked("callback"),
                    msg: Binary::default(),
                    execution_rebate,
                })
            )
            .unwrap(),
            Response::default().add_attribute("action", "rebate-swap")
        );
    }
}
