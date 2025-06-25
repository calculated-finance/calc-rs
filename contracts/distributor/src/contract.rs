use std::collections::HashMap;

use calc_rs::types::{
    ContractError, ContractResult, DistributeStatistics, DistributeStrategyConfig, Distribution,
    DistributorExecuteMsg, DistributorQueryMsg, DomainEvent, Recipient,
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Coin, Coins, CosmosMsg, Decimal, Deps, DepsMut, Env,
    MessageInfo, Response, StdError, StdResult, Uint128,
};

use crate::state::{CONFIG, STATISTICS};

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: DistributeStrategyConfig,
) -> ContractResult {
    deps.api
        .addr_validate(&msg.owner.to_string())
        .map_err(|_| ContractError::generic_err(format!("Invalid owner address: {}", msg.owner)))?;

    let destinations = msg
        .mutable_destinations
        .iter()
        .chain(msg.immutable_destinations.iter())
        .collect::<Vec<_>>();

    if destinations.is_empty() {
        return Err(ContractError::generic_err(
            "Must provide at least one destination",
        ));
    }

    if destinations.len() > 20 {
        return Err(ContractError::generic_err(
            "Cannot provide more than 20 total destinations",
        ));
    }

    let has_native_denoms = msg.denoms.iter().any(|d| !d.contains("-"));
    let mut total_shares = Uint128::zero();

    for destination in destinations.clone() {
        if destination.shares.is_zero() {
            return Err(ContractError::generic_err(
                "Shares for each destination must be greater than zero",
            ));
        }

        match destination.recipient.clone() {
            Recipient::Bank { address, .. } | Recipient::Wasm { address, .. } => {
                deps.api.addr_validate(&address.to_string()).map_err(|_| {
                    ContractError::generic_err(format!("Invalid destination address: {}", address))
                })?;
            }
            Recipient::Withdraw { address, .. } => {
                if has_native_denoms && !address.contains("thor") {
                    return Err(ContractError::generic_err(format!(
                        "Cannot distribute native assets to a non thor address: {}",
                        address
                    )));
                }
            }
        }

        total_shares += destination.shares;
    }

    if total_shares < Uint128::new(10_000) {
        return Err(ContractError::generic_err(
            "Total shares must be at least 10,000",
        ));
    }

    CONFIG.save(deps.storage, &msg)?;

    STATISTICS.save(
        deps.storage,
        &DistributeStatistics {
            amount_distributed: HashMap::new(),
            amount_withdrawn: vec![],
        },
    )?;

    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: DistributorExecuteMsg,
) -> ContractResult {
    match msg {
        DistributorExecuteMsg::Distribute {} => {
            let config = CONFIG.load(deps.storage)?;

            if !config
                .conditions
                .iter()
                .all(|c| c.is_satisfied(deps.as_ref(), &env).unwrap_or(false))
            {
                return Ok(Response::default());
            }

            let mut distributions: Vec<Distribution> = vec![];

            for denom in config.denoms {
                let balance = deps.querier.query_balance(&env.contract.address, &denom)?;

                if balance.amount.is_zero() {
                    continue;
                }

                let destinations = config
                    .mutable_destinations
                    .iter()
                    .chain(config.immutable_destinations.iter());

                let total_shares = destinations
                    .clone()
                    .fold(Uint128::zero(), |acc, d| acc + d.shares);

                distributions.extend(
                    destinations
                        .map(|d| Distribution {
                            destination: d.clone(),
                            amount: vec![Coin {
                                denom: balance.denom.clone(),
                                amount: balance
                                    .amount
                                    .mul_floor(Decimal::from_ratio(d.shares, total_shares)),
                            }],
                        })
                        .collect::<Vec<Distribution>>(),
                );
            }

            let distribution_event = DomainEvent::FundsDistributed {
                contract_address: env.contract.address.clone(),
                to: distributions.clone(),
            };

            let mut messages: Vec<CosmosMsg> = vec![];

            let mut statistics = STATISTICS
                .load(deps.storage)
                .unwrap_or(DistributeStatistics {
                    amount_distributed: HashMap::new(),
                    amount_withdrawn: vec![],
                });

            for distribution in distributions.into_iter() {
                messages.push(distribution.clone().get_msg(deps.as_ref(), &env)?);

                statistics
                    .amount_distributed
                    .entry(distribution.destination.recipient.address())
                    .and_modify(|existing| {
                        let mut coins =
                            Coins::try_from(existing.as_ref()).unwrap_or(Coins::default());
                        for c in distribution.amount.iter() {
                            coins.add(c.clone()).unwrap_or(());
                        }
                    })
                    .or_insert(distribution.amount.clone());
            }

            STATISTICS.save(deps.storage, &statistics)?;

            Ok(Response::default()
                .add_messages(messages)
                .add_event(distribution_event))
        }
        DistributorExecuteMsg::Withdraw { amounts } => {
            let config = CONFIG.load(deps.storage)?;

            if config.owner != info.sender {
                return Err(ContractError::Unauthorized {});
            }

            let funds = amounts
                .iter()
                .filter_map(|amount| {
                    match deps
                        .querier
                        .query_balance(env.contract.address.clone(), amount.denom.clone())
                    {
                        Ok(balance) => {
                            if balance.amount < amount.amount {
                                return Some(Err(StdError::generic_err(format!(
                                    "Insufficient funds for withdrawal: {}",
                                    amount.denom
                                ))));
                            }
                            if balance.amount.is_zero() {
                                return None;
                            }
                            Some(Ok(amount.clone()))
                        }
                        Err(e) => Some(Err(e)),
                    }
                })
                .collect::<StdResult<Vec<Coin>>>()?;

            let send_assets_msg = BankMsg::Send {
                to_address: config.owner.to_string(),
                amount: funds.clone(),
            };

            let funds_withdrawn_event = DomainEvent::FundsWithdrawn {
                contract_address: env.contract.address.clone(),
                to: config.owner.clone(),
                funds,
            };

            let statistics = STATISTICS
                .load(deps.storage)
                .unwrap_or(DistributeStatistics {
                    amount_distributed: HashMap::new(),
                    amount_withdrawn: vec![],
                });

            let mut amount_withdrawn = Coins::try_from(statistics.amount_withdrawn)?;

            for amount in amounts.iter() {
                amount_withdrawn.add(amount.clone()).unwrap_or(());
            }

            STATISTICS.save(
                deps.storage,
                &DistributeStatistics {
                    amount_withdrawn: amount_withdrawn.into_vec(),
                    ..statistics
                },
            )?;

            Ok(Response::default()
                .add_message(send_assets_msg)
                .add_event(funds_withdrawn_event))
        }
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: DistributorQueryMsg) -> StdResult<Binary> {
    match msg {
        DistributorQueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
    }
}

#[cfg(test)]
fn default_destination() -> calc_rs::types::Destination {
    calc_rs::types::Destination {
        shares: Uint128::new(10000),
        recipient: Recipient::Bank {
            address: cosmwasm_std::testing::mock_dependencies()
                .api
                .addr_make("destination1"),
        },
        label: None,
    }
}

#[cfg(test)]
fn default_config() -> DistributeStrategyConfig {
    DistributeStrategyConfig {
        owner: cosmwasm_std::testing::mock_dependencies()
            .api
            .addr_make("owner"),
        denoms: vec!["rune".to_string()],
        mutable_destinations: vec![default_destination()],
        immutable_destinations: vec![default_destination()],
        conditions: vec![],
    }
}

#[cfg(test)]
mod instantiate_tests {
    use super::*;
    use calc_rs::types::Destination;
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr,
    };
    use rstest::rstest;

    #[rstest]
    #[case(
        DistributeStrategyConfig {
            owner: Addr::unchecked("owner"),
            ..default_config()
        },
        "Generic error: Invalid owner address: owner"
    )]
    #[case(
        DistributeStrategyConfig {
            mutable_destinations: vec![],
            immutable_destinations: vec![],
            ..default_config()
        },
        "Generic error: Must provide at least one destination"
    )]
    #[case(
        DistributeStrategyConfig {
            mutable_destinations: (0..30).map(|_| default_destination()).collect(),
            ..default_config()
        },
        "Generic error: Cannot provide more than 20 total destinations"
    )]
    #[case(
        DistributeStrategyConfig {
            mutable_destinations: vec![
                Destination {
                    shares: Uint128::zero(),
                    ..default_destination()
                },
                Destination {
                    shares: Uint128::new(10_000),
                    ..default_destination()
                }
            ],
            immutable_destinations: vec![],
            ..default_config()
        },
        "Generic error: Shares for each destination must be greater than zero"
    )]
    #[case(
        DistributeStrategyConfig {
            mutable_destinations: vec![Destination {
                shares: Uint128::new(10_000),
                recipient: Recipient::Bank {
                    address: Addr::unchecked("invalid_address"),
                },
                ..default_destination()
            }],
            immutable_destinations: vec![],
            ..default_config()
        },
        "Generic error: Invalid destination address: invalid_address"
    )]
    #[case(
        DistributeStrategyConfig {
            mutable_destinations: vec![Destination {
                shares: Uint128::new(10_000),
                recipient: Recipient::Withdraw {
                    address: "evm-address".to_string(),
                },
                ..default_destination()
            }],
            immutable_destinations: vec![],
            denoms: vec!["rune".to_string(), "eth-eth".to_string()],
            ..default_config()
        },
        "Generic error: Cannot distribute native assets to a non thor address: evm-address"
    )]
    #[case(
        DistributeStrategyConfig {
            mutable_destinations: vec![Destination {
                shares: Uint128::new(5000),
                ..default_destination()
            }],
            immutable_destinations: vec![],
            ..default_config()
        },
        "Generic error: Total shares must be at least 10,000"
    )]
    fn invalid_config_fails(#[case] msg: DistributeStrategyConfig, #[case] expected_error: &str) {
        let mut deps = mock_dependencies();

        assert_eq!(
            instantiate(
                deps.as_mut(),
                mock_env(),
                message_info(&msg.owner, &[]),
                msg
            )
            .unwrap_err()
            .to_string(),
            expected_error
        );
    }

    #[rstest]
    fn valid_config_succeeds() {
        let mut deps = mock_dependencies();
        let msg = default_config();

        instantiate(
            deps.as_mut(),
            mock_env(),
            message_info(&msg.owner, &[]),
            msg.clone(),
        )
        .unwrap();

        assert_eq!(CONFIG.load(&deps.storage).unwrap(), msg);
    }
}

#[cfg(test)]
mod distribute_tests {
    use super::*;
    use calc_rs::types::{Condition, Destination, MsgDeposit};
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr, CosmosMsg, Event, SubMsg, WasmMsg,
    };
    use rstest::rstest;

    #[test]
    fn does_nothing_if_conditions_not_met() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let config = DistributeStrategyConfig {
            conditions: vec![Condition::BalanceMet {
                address: env.contract.address.clone(),
                balance: Coin::new(1_000_u128, "rune"),
            }],
            ..default_config()
        };

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        deps.querier
            .bank
            .update_balance(&env.contract.address, vec![Coin::new(500_u128, "rune")]);

        let response = execute(
            deps.as_mut(),
            env,
            message_info(&Addr::unchecked("anyone"), &[]),
            DistributorExecuteMsg::Distribute {},
        )
        .unwrap();

        assert_eq!(response, Response::default());
    }

    #[test]
    fn distributes_funds_if_conditions_met() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let config = DistributeStrategyConfig {
            conditions: vec![Condition::BalanceMet {
                address: env.contract.address.clone(),
                balance: Coin::new(1_000_u128, "rune"),
            }],
            ..default_config()
        };

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        let balance = Coin::new(1_000_u128, "rune");

        deps.querier
            .bank
            .update_balance(&env.contract.address, vec![balance.clone()]);

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&Addr::unchecked("anyone"), &[]),
            DistributorExecuteMsg::Distribute {},
        )
        .unwrap();

        let destinations = config
            .mutable_destinations
            .into_iter()
            .chain(config.immutable_destinations.into_iter())
            .collect::<Vec<_>>();

        let total_shares = destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        let distributions = destinations
            .into_iter()
            .map(|d| Distribution {
                destination: d.clone(),
                amount: vec![Coin {
                    denom: balance.denom.clone(),
                    amount: balance
                        .amount
                        .mul_floor(Decimal::from_ratio(d.shares, total_shares)),
                }],
            })
            .collect::<Vec<_>>();

        let messages = distributions
            .into_iter()
            .map(|d| SubMsg::new(d.get_msg(deps.as_ref(), &env).unwrap()))
            .collect::<Vec<_>>();

        assert_eq!(response.messages, messages);
    }

    #[rstest]
    #[case(
        0_u128,
        vec![(Addr::unchecked("destination1"), 10_000, None)],
        vec![],
        vec![],
    )]
    #[case(
        0_u128,
        vec![(Addr::unchecked("destination1"), 10_000, Some(to_json_binary(&"test").unwrap()))],
        vec![],
        vec![],
    )]
    #[case(
        10_000_u128,
        vec![(Addr::unchecked("destination1"), 10_000, Some(to_json_binary(&"test").unwrap()))],
        vec![],
        vec![(Addr::unchecked("destination1"), 10_000, Some(to_json_binary(&"test").unwrap()))],
    )]
    #[case(
        10_000_u128,
        vec![(Addr::unchecked("destination1"), 10_000, None)],
        vec![],
        vec![(Addr::unchecked("destination1"), 10_000, None)],
    )]
    #[case(
        10_000_u128,
        vec![(Addr::unchecked("destination1"), 10_000, None)],
        vec![(Addr::unchecked("destination2"), 10_000, None)],
        vec![
            (Addr::unchecked("destination1"), 5_000, None),
            (Addr::unchecked("destination2"), 5_000, None)
        ],
    )]
    #[case(
        10_000_u128,
        vec![(Addr::unchecked("destination1"), 5_000, None)],
        vec![(Addr::unchecked("destination2"), 5_000, None)],
        vec![
            (Addr::unchecked("destination1"), 5_000, None),
            (Addr::unchecked("destination2"), 5_000, None)
        ],
    )]
    #[case(
        10,
        vec![
            (Addr::unchecked("destination1"), 5_000, None),
            (Addr::unchecked("destination2"), 5_000, None),
            (Addr::unchecked("destination3"), 5_000, Some(to_json_binary(&"test").unwrap())),
        ],
        vec![],
        vec![
            (Addr::unchecked("destination1"), 3, None),
            (Addr::unchecked("destination2"), 3, None),
            (Addr::unchecked("destination3"), 3, Some(to_json_binary(&"test").unwrap()))
        ],
    )]
    #[case(
        11,
        vec![
            (Addr::unchecked("destination1"), 5_000, None),
            (Addr::unchecked("destination2"), 5_000, None),
            (Addr::unchecked("destination3"), 5_000, Some(to_json_binary(&"test").unwrap())),
        ],
        vec![],
        vec![
            (Addr::unchecked("destination1"), 3, None),
            (Addr::unchecked("destination2"), 3, None),
            (Addr::unchecked("destination3"), 3, Some(to_json_binary(&"test").unwrap()))
        ],
    )]
    #[case(
        12,
        vec![
            (Addr::unchecked("destination1"), 5_000, None),
            (Addr::unchecked("destination2"), 5_000, None),
            (Addr::unchecked("destination3"), 5_000, Some(to_json_binary(&"test").unwrap())),
        ],
        vec![],
        vec![
            (Addr::unchecked("destination1"), 3, None),
            (Addr::unchecked("destination2"), 3, None),
            (Addr::unchecked("destination3"), 3, Some(to_json_binary(&"test").unwrap()))
        ],
    )]
    #[case(
        13,
        vec![
            (Addr::unchecked("destination1"), 5_000, None),
            (Addr::unchecked("destination2"), 5_000, None),
            (Addr::unchecked("destination3"), 5_000, Some(to_json_binary(&"test").unwrap())),
        ],
        vec![],
        vec![
            (Addr::unchecked("destination1"), 4, None),
            (Addr::unchecked("destination2"), 4, None),
            (Addr::unchecked("destination3"), 4, Some(to_json_binary(&"test").unwrap())),
        ],
    )]
    fn distributes_funds_correctly(
        #[case] balance: u128,
        #[case] mutable_destinations: Vec<(Addr, u128, Option<Binary>)>,
        #[case] immutable_destinations: Vec<(Addr, u128, Option<Binary>)>,
        #[case] distributions: Vec<(Addr, u128, Option<Binary>)>,
    ) {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier
            .bank
            .update_balance(&env.contract.address, vec![Coin::new(balance, "rune")]);

        CONFIG
            .save(
                deps.as_mut().storage,
                &DistributeStrategyConfig {
                    mutable_destinations: mutable_destinations
                        .clone()
                        .into_iter()
                        .map(|(addr, shares, msg)| {
                            msg.map_or(
                                Destination {
                                    recipient: Recipient::Bank {
                                        address: addr.clone(),
                                    },
                                    shares: Uint128::new(shares),
                                    label: None,
                                },
                                |msg| Destination {
                                    shares: Uint128::new(shares),
                                    recipient: Recipient::Wasm { address: addr, msg },
                                    label: None,
                                },
                            )
                        })
                        .collect(),
                    immutable_destinations: immutable_destinations
                        .clone()
                        .into_iter()
                        .map(|(addr, shares, msg)| {
                            msg.map_or(
                                Destination {
                                    recipient: Recipient::Bank {
                                        address: addr.clone(),
                                    },
                                    shares: Uint128::new(shares),
                                    label: None,
                                },
                                |msg| Destination {
                                    shares: Uint128::new(shares),
                                    recipient: Recipient::Wasm { address: addr, msg },
                                    label: None,
                                },
                            )
                        })
                        .collect(),
                    ..default_config()
                },
            )
            .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&Addr::unchecked("anyone"), &[]),
            DistributorExecuteMsg::Distribute {},
        )
        .unwrap();

        assert_eq!(
            response.messages,
            distributions
                .clone()
                .into_iter()
                .map(
                    |(addr, shares, msg)| SubMsg::reply_never(if let Some(msg) = msg {
                        CosmosMsg::Wasm(WasmMsg::Execute {
                            contract_addr: addr.to_string(),
                            msg,
                            funds: vec![Coin::new(shares, "rune")],
                        })
                    } else {
                        CosmosMsg::Bank(BankMsg::Send {
                            to_address: addr.to_string(),
                            amount: vec![Coin::new(shares, "rune")],
                        })
                    })
                )
                .collect::<Vec<_>>()
        );

        let destinations = mutable_destinations
            .into_iter()
            .chain(immutable_destinations.into_iter())
            .collect::<Vec<_>>();

        assert_eq!(
            response.events,
            vec![DomainEvent::FundsDistributed {
                contract_address: env.contract.address.clone(),
                to: distributions
                    .into_iter()
                    .enumerate()
                    .map(|(i, (addr, amount, msg))| Distribution {
                        destination: msg.map_or(
                            Destination {
                                recipient: Recipient::Bank {
                                    address: addr.clone()
                                },
                                shares: Uint128::new(destinations[i].1),
                                label: None,
                            },
                            |msg| Destination {
                                shares: Uint128::new(destinations[i].1),
                                recipient: Recipient::Wasm { address: addr, msg },
                                label: None,
                            },
                        ),
                        amount: vec![Coin::new(amount, "rune")],
                    })
                    .collect(),
            }]
            .into_iter()
            .map(Event::from)
            .collect::<Vec<Event>>(),
        );
    }

    #[test]
    fn distributes_secured_asset_correctly() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let recipient_address = "evm-address".to_string();

        let config = DistributeStrategyConfig {
            mutable_destinations: vec![Destination {
                shares: Uint128::new(10_000),
                recipient: Recipient::Withdraw {
                    address: recipient_address.clone(),
                },
                label: None,
            }],
            immutable_destinations: vec![],
            denoms: vec!["eth-eth".to_string()],
            ..default_config()
        };

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        deps.querier.bank.update_balance(
            &env.contract.address,
            vec![Coin::new(1_000_u128, "eth-eth")],
        );

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&Addr::unchecked("anyone"), &[]),
            DistributorExecuteMsg::Distribute {},
        )
        .unwrap();

        assert_eq!(
            response.messages,
            vec![SubMsg::new(CosmosMsg::from(MsgDeposit {
                memo: format!("SECURE-:{}", recipient_address),
                coins: vec![Coin::new(1_000_u128, "eth-eth")],
                signer: deps
                    .as_ref()
                    .api
                    .addr_canonicalize(&env.contract.address.as_str())
                    .unwrap(),
            }))]
        );
    }

    #[test]
    fn distributes_multiple_denoms() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let config = DistributeStrategyConfig {
            mutable_destinations: vec![
                Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Bank {
                        address: Addr::unchecked("destination1"),
                    },
                    label: None,
                },
                Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Bank {
                        address: Addr::unchecked("destination2"),
                    },
                    label: None,
                },
            ],
            immutable_destinations: vec![],
            denoms: vec!["rune".to_string(), "btc-btc".to_string()],
            ..default_config()
        };

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        deps.querier.bank.update_balance(
            &env.contract.address,
            vec![
                Coin::new(1_000_u128, "rune"),
                Coin::new(500_u128, "btc-btc"),
            ],
        );

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&Addr::unchecked("anyone"), &[]),
            DistributorExecuteMsg::Distribute {},
        )
        .unwrap();

        assert_eq!(
            response.messages,
            vec![
                SubMsg::reply_never(CosmosMsg::Bank(BankMsg::Send {
                    to_address: "destination1".to_string(),
                    amount: vec![Coin::new(500_u128, "rune")],
                })),
                SubMsg::reply_never(CosmosMsg::Bank(BankMsg::Send {
                    to_address: "destination2".to_string(),
                    amount: vec![Coin::new(500_u128, "rune")],
                })),
                SubMsg::reply_never(CosmosMsg::Bank(BankMsg::Send {
                    to_address: "destination1".to_string(),
                    amount: vec![Coin::new(250_u128, "btc-btc")],
                })),
                SubMsg::reply_never(CosmosMsg::Bank(BankMsg::Send {
                    to_address: "destination2".to_string(),
                    amount: vec![Coin::new(250_u128, "btc-btc")],
                })),
            ]
        );
    }

    #[test]
    fn updates_statistics() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let bank_recipient = Addr::unchecked("destination1");
        let wasm_recipient = Addr::unchecked("destination2");
        let deposit_recipient = "evm-address".to_string();

        let config = DistributeStrategyConfig {
            mutable_destinations: vec![
                Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Bank {
                        address: bank_recipient.clone(),
                    },
                    label: None,
                },
                Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Wasm {
                        address: wasm_recipient.clone(),
                        msg: to_json_binary(&"test").unwrap(),
                    },
                    label: None,
                },
                Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Withdraw {
                        address: deposit_recipient.clone(),
                    },
                    label: None,
                },
            ],
            immutable_destinations: vec![],
            ..default_config()
        };

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        let balance = Coin::new(1_000_u128, "rune");

        deps.querier
            .bank
            .update_balance(&env.contract.address, vec![balance.clone()]);

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&Addr::unchecked("anyone"), &[]),
            DistributorExecuteMsg::Distribute {},
        )
        .unwrap();

        let statistics = STATISTICS.load(deps.as_mut().storage).unwrap();

        assert_eq!(
            statistics.amount_distributed,
            HashMap::from([
                (
                    bank_recipient.to_string(),
                    vec![Coin::new(
                        balance.amount.mul_floor(Decimal::from_ratio(1u128, 3u128)),
                        "rune"
                    )]
                ),
                (
                    deposit_recipient.to_string(),
                    vec![Coin::new(
                        balance.amount.mul_floor(Decimal::from_ratio(1u128, 3u128)),
                        "rune"
                    )]
                ),
                (
                    wasm_recipient.to_string(),
                    vec![Coin::new(
                        balance.amount.mul_floor(Decimal::from_ratio(1u128, 3u128)),
                        "rune"
                    )]
                ),
            ])
        );
    }

    #[test]
    fn publishes_funds_distributed_event() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let config = DistributeStrategyConfig {
            conditions: vec![Condition::BalanceMet {
                address: env.contract.address.clone(),
                balance: Coin::new(1_000_u128, "rune"),
            }],
            ..default_config()
        };

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        let balance = Coin::new(1_000_u128, "rune");

        deps.querier
            .bank
            .update_balance(&env.contract.address, vec![balance.clone()]);

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&Addr::unchecked("anyone"), &[]),
            DistributorExecuteMsg::Distribute {},
        )
        .unwrap();

        let destinations = config
            .mutable_destinations
            .iter()
            .chain(config.immutable_destinations.iter())
            .collect::<Vec<_>>();

        let total_shares = destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        assert_eq!(
            response.events,
            vec![DomainEvent::FundsDistributed {
                contract_address: env.contract.address.clone(),
                to: config
                    .mutable_destinations
                    .iter()
                    .chain(config.immutable_destinations.iter())
                    .map(|d| Distribution {
                        destination: d.clone(),
                        amount: vec![Coin {
                            denom: "rune".to_string(),
                            amount: balance
                                .amount
                                .mul_floor(Decimal::from_ratio(d.shares, total_shares)),
                        }],
                    })
                    .collect(),
            }]
            .into_iter()
            .map(Event::from)
            .collect::<Vec<Event>>(),
        );
    }
}

#[cfg(test)]
mod withdraw_tests {
    use super::*;
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr, CosmosMsg, Event, SubMsg,
    };

    #[test]
    fn returns_unauthorised_when_sender_not_owner() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        CONFIG
            .save(deps.as_mut().storage, &default_config())
            .unwrap();

        let response = execute(
            deps.as_mut(),
            env,
            message_info(&Addr::unchecked("not_owner"), &[]),
            DistributorExecuteMsg::Withdraw {
                amounts: vec![Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(1000),
                }],
            },
        )
        .unwrap_err();

        assert_eq!(response.to_string(), "Unauthorized");
    }

    #[test]
    fn withdraws_funds_correctly() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        deps.querier.bank.update_balance(
            &env.contract.address,
            vec![
                Coin::new(1_000_u128, "rune"),
                Coin::new(1_000_u128, "uruji"),
                Coin::new(1_000_u128, "btc-btc"),
            ],
        );

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&config.owner, &[]),
            DistributorExecuteMsg::Withdraw {
                amounts: vec![Coin::new(1_000_u128, "rune"), Coin::new(500_u128, "uruji")],
            },
        )
        .unwrap();

        assert_eq!(
            response.messages,
            vec![SubMsg::reply_never(CosmosMsg::Bank(BankMsg::Send {
                to_address: config.owner.to_string(),
                amount: vec![Coin::new(1_000_u128, "rune"), Coin::new(500_u128, "uruji")],
            })),]
        );

        assert_eq!(
            response.events,
            vec![DomainEvent::FundsWithdrawn {
                contract_address: env.contract.address.clone(),
                to: config.owner,
                funds: vec![Coin::new(1_000_u128, "rune"), Coin::new(500_u128, "uruji")],
            }]
            .into_iter()
            .map(Event::from)
            .collect::<Vec<Event>>(),
        );
    }
}
