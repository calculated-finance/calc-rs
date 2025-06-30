use std::{cmp::min, collections::HashMap};

use calc_rs::{
    core::{ContractError, ContractResult},
    distributor::{
        Distribution, DistributorConfig, DistributorExecuteMsg, DistributorQueryMsg,
        DistributorStatistics, DomainEvent,
    },
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Coin, Coins, CosmosMsg, Decimal, Deps, DepsMut, Env,
    MessageInfo, Response, StdResult, Uint128,
};

use crate::state::{CONFIG, STATS};

#[entry_point]
pub fn instantiate(
    mut deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    mut msg: DistributorConfig,
) -> ContractResult {
    CONFIG.save(&mut deps, &env, &mut msg)?;

    STATS.save(
        deps.storage,
        &DistributorStatistics {
            distributed: HashMap::new(),
            withdrawn: vec![],
        },
    )?;

    Ok(Response::default())
}

#[cw_serde]
pub struct DistributeMigrateMsg {}

#[entry_point]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: DistributeMigrateMsg) -> ContractResult {
    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: DistributorExecuteMsg,
) -> ContractResult {
    let mut config = CONFIG.load(deps.storage)?;
    let mut stats = STATS.load(deps.storage)?;

    let mut messages: Vec<CosmosMsg> = vec![];
    let mut events: Vec<DomainEvent> = vec![];

    match msg {
        DistributorExecuteMsg::Update(new_config) => {
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            config = DistributorConfig {
                immutable_destinations: config.immutable_destinations,
                ..new_config
            };
        }
        DistributorExecuteMsg::Distribute {} => {
            if config
                .conditions
                .iter()
                .all(|c| c.check(deps.as_ref(), &env).is_ok())
            {
                let mut distributions: Vec<Distribution> = vec![];

                let destinations = config
                    .mutable_destinations
                    .iter()
                    .chain(config.immutable_destinations.iter());

                let total_shares = destinations
                    .clone()
                    .fold(Uint128::zero(), |acc, d| acc + d.shares);

                for denom in config.denoms.clone() {
                    let balance = deps.querier.query_balance(&env.contract.address, &denom)?;

                    if balance.amount.is_zero() {
                        continue;
                    }

                    for destination in destinations.clone() {
                        let distribution = Distribution {
                            destination: destination.clone(),
                            amount: vec![Coin::new(
                                balance.amount.mul_floor(Decimal::from_ratio(
                                    destination.shares,
                                    total_shares,
                                )),
                                balance.denom.clone(),
                            )],
                        };

                        stats
                            .distributed
                            .entry(distribution.destination.recipient.key())
                            .and_modify(|existing| {
                                let mut coins =
                                    Coins::try_from(existing.as_ref()).unwrap_or(Coins::default());
                                for c in distribution.amount.iter() {
                                    coins.add(c.clone()).unwrap_or(());
                                }
                            })
                            .or_insert(distribution.amount.clone());

                        distributions.push(distribution);
                    }
                }

                let distribution_messages = distributions
                    .clone()
                    .into_iter()
                    .flat_map(|d| d.get_msg(deps.as_ref(), &env))
                    .collect::<Vec<CosmosMsg>>();

                let funds_distributed_event = DomainEvent::FundsDistributed {
                    contract_address: env.contract.address.clone(),
                    to: distributions,
                };

                messages.extend(distribution_messages);
                events.push(funds_distributed_event);
            }
        }
        DistributorExecuteMsg::Withdraw { amounts } => {
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            let mut withdrawals = Coins::default();
            let mut amount_withdrawn = Coins::try_from(stats.withdrawn)?;

            for amount in amounts {
                let balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), amount.denom.clone())?;

                if balance.amount >= Uint128::zero() {
                    withdrawals.add(Coin::new(
                        min(balance.amount, amount.amount),
                        amount.denom.clone(),
                    ))?;

                    amount_withdrawn.add(Coin::new(
                        min(balance.amount, amount.amount),
                        amount.denom.clone(),
                    ))?;
                }
            }

            if !withdrawals.is_empty() {
                messages.push(
                    BankMsg::Send {
                        to_address: info.sender.to_string(),
                        amount: withdrawals.to_vec(),
                    }
                    .into(),
                );
            }

            let funds_withdrawn_event = DomainEvent::FundsWithdrawn {
                contract_address: env.contract.address.clone(),
                to: config.owner.clone(),
                funds: withdrawals.to_vec(),
            };

            events.push(funds_withdrawn_event);

            stats.withdrawn = amount_withdrawn.to_vec();
        }
    };

    CONFIG.save(&mut deps, &env, &mut config)?;
    STATS.save(deps.storage, &stats)?;

    Ok(Response::new().add_messages(messages).add_events(events))
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: DistributorQueryMsg) -> StdResult<Binary> {
    match msg {
        DistributorQueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        DistributorQueryMsg::Statistics {} => to_json_binary(&STATS.load(deps.storage)?),
    }
}

#[cfg(test)]
mod instantiate_tests {
    use crate::test::default_config;

    use super::*;
    use cosmwasm_std::testing::{message_info, mock_dependencies, mock_env};

    #[test]
    fn saves_config_and_statistics() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let msg = default_config();

        instantiate(
            deps.as_mut(),
            env,
            message_info(&msg.owner, &[]),
            msg.clone(),
        )
        .unwrap();

        let config = CONFIG.load(deps.as_ref().storage).unwrap();
        assert_eq!(config, msg);

        let statistics = STATS.load(deps.as_ref().storage).unwrap();
        assert_eq!(
            statistics,
            DistributorStatistics {
                distributed: HashMap::new(),
                withdrawn: vec![]
            }
        );
    }
}

#[cfg(test)]
mod update_tests {
    use crate::test::default_config;

    use super::*;

    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr,
    };

    #[test]
    fn returns_unauthorised_when_sender_not_owner() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let config = default_config();
        CONFIG
            .save(&mut deps.as_mut(), &env, &mut config.clone())
            .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            message_info(&Addr::unchecked("not-owner"), &[]),
            DistributorExecuteMsg::Update(config.clone()),
        )
        .unwrap_err();

        assert_eq!(err, ContractError::Unauthorized {});
    }

    #[test]
    fn does_not_update_immutable_destinations() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let mut config = default_config();
        CONFIG.save(&mut deps.as_mut(), &env, &mut config).unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let new_config = DistributorConfig {
            owner: deps.api.addr_make(&"new-owner"),
            mutable_destinations: config.immutable_destinations.clone(),
            immutable_destinations: config.mutable_destinations.clone(),
            conditions: vec![],
            ..config
        };

        execute(
            deps.as_mut(),
            env,
            message_info(&config.owner, &[]),
            DistributorExecuteMsg::Update(new_config.clone()),
        )
        .unwrap();

        let updated_config = CONFIG.load(deps.as_ref().storage).unwrap();

        assert_eq!(
            updated_config,
            DistributorConfig {
                immutable_destinations: config.immutable_destinations,
                ..new_config
            }
        );
    }
}

#[cfg(test)]
mod distribute_tests {
    use crate::test::default_config;

    use super::*;
    use calc_rs::core::Condition;
    use calc_rs::distributor::{Destination, Recipient};
    use calc_rs::thorchain::MsgDeposit;
    use calc_rs_test::test::mock_dependencies_with_custom_grpc_querier;
    use cosmwasm_std::SystemResult;
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr, ContractResult as CosmwasmResult, CosmosMsg, Event, SubMsg, WasmMsg,
    };
    use prost::Message;
    use rstest::rstest;
    use rujira_rs::proto::types::QueryNetworkResponse;

    #[test]
    fn does_nothing_if_conditions_not_met() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let mut config = DistributorConfig {
            conditions: vec![Condition::BalanceAvailable {
                address: env.contract.address.clone(),
                amount: Coin::new(1_000_u128, "rune"),
            }],
            ..default_config()
        };

        CONFIG
            .save(&mut deps.as_mut(), &mock_env(), &mut config)
            .unwrap();

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

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let mut config = DistributorConfig {
            conditions: vec![Condition::BalanceAvailable {
                address: env.contract.address.clone(),
                amount: Coin::new(1_000_u128, "rune"),
            }],
            ..default_config()
        };

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(&mut deps.as_mut(), &env, &mut config).unwrap();

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
                amount: vec![Coin::new(
                    balance
                        .amount
                        .mul_floor(Decimal::from_ratio(d.shares, total_shares)),
                    balance.denom.clone(),
                )],
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
        vec![("destination1".to_string(), 10_000, None)],
        vec![],
        vec![],
    )]
    #[case(
        0_u128,
        vec![("destination1".to_string(), 10_000, Some(to_json_binary(&"test").unwrap()))],
        vec![],
        vec![],
    )]
    #[case(
        10_000_u128,
        vec![("destination1".to_string(), 10_000, Some(to_json_binary(&"test").unwrap()))],
        vec![],
        vec![("destination1".to_string(), 10_000, Some(to_json_binary(&"test").unwrap()))],
    )]
    #[case(
        10_000_u128,
        vec![("destination1".to_string(), 10_000, None)],
        vec![],
        vec![("destination1".to_string(), 10_000, None)],
    )]
    #[case(
        10_000_u128,
        vec![("destination1".to_string(), 10_000, None)],
        vec![("destination2".to_string(), 10_000, None)],
        vec![
            ("destination1".to_string(), 5_000, None),
            ("destination2".to_string(), 5_000, None)
        ],
    )]
    #[case(
        10_000_u128,
        vec![("destination1".to_string(), 5_000, None)],
        vec![("destination2".to_string(), 5_000, None)],
        vec![
            ("destination1".to_string(), 5_000, None),
            ("destination2".to_string(), 5_000, None)
        ],
    )]
    #[case(
        10,
        vec![
            ("destination1".to_string(), 5_000, None),
            ("destination2".to_string(), 5_000, None),
            ("destination3".to_string(), 5_000, Some(to_json_binary(&"test").unwrap())),
        ],
        vec![],
        vec![
            ("destination1".to_string(), 3, None),
            ("destination2".to_string(), 3, None),
            ("destination3".to_string(), 3, Some(to_json_binary(&"test").unwrap()))
        ],
    )]
    #[case(
        11,
        vec![
            ("destination1".to_string(), 5_000, None),
            ("destination2".to_string(), 5_000, None),
            ("destination3".to_string(), 5_000, Some(to_json_binary(&"test").unwrap())),
        ],
        vec![],
        vec![
            ("destination1".to_string(), 3, None),
            ("destination2".to_string(), 3, None),
            ("destination3".to_string(), 3, Some(to_json_binary(&"test").unwrap()))
        ],
    )]
    #[case(
        12,
        vec![
            ("destination1".to_string(), 5_000, None),
            ("destination2".to_string(), 5_000, None),
            ("destination3".to_string(), 5_000, Some(to_json_binary(&"test").unwrap())),
        ],
        vec![],
        vec![
            ("destination1".to_string(), 3, None),
            ("destination2".to_string(), 3, None),
            ("destination3".to_string(), 3, Some(to_json_binary(&"test").unwrap()))
        ],
    )]
    #[case(
        13,
        vec![
            ("destination1".to_string(), 5_000, None),
            ("destination2".to_string(), 5_000, None),
            ("destination3".to_string(), 5_000, Some(to_json_binary(&"test").unwrap())),
        ],
        vec![],
        vec![
            ("destination1".to_string(), 4, None),
            ("destination2".to_string(), 4, None),
            ("destination3".to_string(), 4, Some(to_json_binary(&"test").unwrap())),
        ],
    )]
    fn distributes_funds_accurately(
        #[case] balance: u128,
        #[case] mutable_destinations: Vec<(String, u128, Option<Binary>)>,
        #[case] immutable_destinations: Vec<(String, u128, Option<Binary>)>,
        #[case] distributions: Vec<(String, u128, Option<Binary>)>,
    ) {
        let mut deps = mock_dependencies();
        let env = mock_env();

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        deps.querier
            .bank
            .update_balance(&env.contract.address, vec![Coin::new(balance, "rune")]);

        CONFIG
            .save(
                &mut deps.as_mut(),
                &env,
                &mut DistributorConfig {
                    mutable_destinations: mutable_destinations
                        .clone()
                        .into_iter()
                        .map(|(addr, shares, msg)| {
                            msg.map_or(
                                Destination {
                                    recipient: Recipient::Bank {
                                        address: mock_dependencies().api.addr_make(&addr),
                                    },
                                    shares: Uint128::new(shares),
                                    label: None,
                                },
                                |msg| Destination {
                                    shares: Uint128::new(shares),
                                    recipient: Recipient::Wasm {
                                        address: mock_dependencies().api.addr_make(&addr),
                                        msg,
                                    },
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
                                        address: mock_dependencies().api.addr_make(&addr),
                                    },
                                    shares: Uint128::new(shares),
                                    label: None,
                                },
                                |msg| Destination {
                                    shares: Uint128::new(shares),
                                    recipient: Recipient::Wasm {
                                        address: mock_dependencies().api.addr_make(&addr),
                                        msg,
                                    },
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
                            contract_addr: deps.api.addr_make(&addr).to_string(),
                            msg,
                            funds: vec![Coin::new(shares, "rune")],
                        })
                    } else {
                        CosmosMsg::Bank(BankMsg::Send {
                            to_address: deps.api.addr_make(&addr).to_string(),
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
                                    address: deps.api.addr_make(&addr)
                                },
                                shares: Uint128::new(destinations[i].1),
                                label: None,
                            },
                            |msg| Destination {
                                shares: Uint128::new(destinations[i].1),
                                recipient: Recipient::Wasm {
                                    address: deps.api.addr_make(&addr),
                                    msg
                                },
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
        let mut deps = mock_dependencies_with_custom_grpc_querier();
        let env = mock_env();

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let recipient_address = "evm-address".to_string();

        let mut config = DistributorConfig {
            mutable_destinations: vec![Destination {
                shares: Uint128::new(10_000),
                recipient: Recipient::Deposit {
                    memo: format!("SECURE-:{}", recipient_address),
                },
                label: None,
            }],
            immutable_destinations: vec![],
            denoms: vec!["eth-eth".to_string()],
            ..default_config()
        };

        let deposit_fee = 2_000_000_u128;

        deps.querier.with_grpc_handler(move |_| {
            let response = QueryNetworkResponse {
                bond_reward_rune: "4726527489".to_string(),
                total_bond_units: "277404".to_string(),
                effective_security_bond: "90126604378071".to_string(),
                total_reserve: "4994080222948541".to_string(),
                vaults_migrating: true,
                gas_spent_rune: "0".to_string(),
                gas_withheld_rune: "0".to_string(),
                outbound_fee_multiplier: "30000".to_string(),
                native_outbound_fee_rune: "2000000".to_string(),
                native_tx_fee_rune: deposit_fee.to_string(),
                tns_register_fee_rune: "1000000000".to_string(),
                tns_fee_per_block_rune: "20".to_string(),
                rune_price_in_tor: "1.14130903".to_string(),
                tor_price_in_rune: "0.87618688".to_string(),
            };

            let mut buf = Vec::new();
            response.encode(&mut buf).unwrap();

            SystemResult::Ok(CosmwasmResult::Ok(buf.into()))
        });

        CONFIG.save(&mut deps.as_mut(), &env, &mut config).unwrap();

        deps.querier.default.bank.update_balance(
            &env.contract.address,
            vec![
                Coin::new(1_000_u128, "eth-eth"),
                Coin::new(deposit_fee, "rune"),
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
            vec![SubMsg::new(CosmosMsg::from(
                MsgDeposit {
                    memo: format!("SECURE-:{}", recipient_address),
                    coins: vec![Coin::new(1_000_u128, "eth-eth")],
                    signer: deps
                        .as_ref()
                        .api
                        .addr_canonicalize(&env.contract.address.as_str())
                        .unwrap(),
                }
                .into_cosmos_msg()
                .unwrap()
            ))]
        );
    }

    #[test]
    fn distributes_multiple_denoms() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let address_1 = deps.api.addr_make("destination1");
        let address_2 = deps.api.addr_make("destination2");

        let mut config = DistributorConfig {
            mutable_destinations: vec![
                Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Bank {
                        address: address_1.clone(),
                    },
                    label: None,
                },
                Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Bank {
                        address: address_2.clone(),
                    },
                    label: None,
                },
            ],
            immutable_destinations: vec![],
            denoms: vec!["rune".to_string(), "btc-btc".to_string()],
            ..default_config()
        };

        CONFIG.save(&mut deps.as_mut(), &env, &mut config).unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

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
                    to_address: address_1.to_string(),
                    amount: vec![Coin::new(500_u128, "rune")],
                })),
                SubMsg::reply_never(CosmosMsg::Bank(BankMsg::Send {
                    to_address: address_2.to_string(),
                    amount: vec![Coin::new(500_u128, "rune")],
                })),
                SubMsg::reply_never(CosmosMsg::Bank(BankMsg::Send {
                    to_address: address_1.to_string(),
                    amount: vec![Coin::new(250_u128, "btc-btc")],
                })),
                SubMsg::reply_never(CosmosMsg::Bank(BankMsg::Send {
                    to_address: address_2.to_string(),
                    amount: vec![Coin::new(250_u128, "btc-btc")],
                })),
            ]
        );
    }

    #[test]
    fn updates_statistics() {
        let mut deps = mock_dependencies_with_custom_grpc_querier();
        let env = mock_env();

        let bank_recipient = deps.api.addr_make("destination1");
        let wasm_recipient = deps.api.addr_make("destination2");
        let deposit_recipient = "evm-address".to_string();
        let denom = "eth-eth".to_string();

        let mut config = DistributorConfig {
            denoms: vec![denom.clone()],
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
                    recipient: Recipient::Deposit {
                        memo: format!("SECURE-:{}", deposit_recipient),
                    },
                    label: None,
                },
            ],
            immutable_destinations: vec![],
            ..default_config()
        };

        let deposit_fee = 2_000_000_u128;

        deps.querier.with_grpc_handler(move |_| {
            let response = QueryNetworkResponse {
                bond_reward_rune: "4726527489".to_string(),
                total_bond_units: "277404".to_string(),
                effective_security_bond: "90126604378071".to_string(),
                total_reserve: "4994080222948541".to_string(),
                vaults_migrating: true,
                gas_spent_rune: "0".to_string(),
                gas_withheld_rune: "0".to_string(),
                outbound_fee_multiplier: "30000".to_string(),
                native_outbound_fee_rune: "2000000".to_string(),
                native_tx_fee_rune: deposit_fee.to_string(),
                tns_register_fee_rune: "1000000000".to_string(),
                tns_fee_per_block_rune: "20".to_string(),
                rune_price_in_tor: "1.14130903".to_string(),
                tor_price_in_rune: "0.87618688".to_string(),
            };

            let mut buf = Vec::new();
            response.encode(&mut buf).unwrap();

            SystemResult::Ok(CosmwasmResult::Ok(buf.into()))
        });

        CONFIG.save(&mut deps.as_mut(), &env, &mut config).unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let balance = Coin::new(1_000_u128, denom.clone());

        deps.querier.default.bank.update_balance(
            &env.contract.address,
            vec![balance.clone(), Coin::new(deposit_fee, "rune")],
        );

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&Addr::unchecked("anyone"), &[]),
            DistributorExecuteMsg::Distribute {},
        )
        .unwrap();

        let statistics = STATS.load(deps.as_mut().storage).unwrap();

        let destinations = config
            .mutable_destinations
            .into_iter()
            .chain(config.immutable_destinations.into_iter())
            .collect::<Vec<_>>();

        let total_shares = destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        assert_eq!(
            statistics.distributed,
            destinations
                .iter()
                .map(|d| (
                    d.recipient.key(),
                    vec![Coin::new(
                        balance
                            .amount
                            .mul_floor(Decimal::from_ratio(d.shares, total_shares)),
                        denom.clone()
                    )]
                ))
                .collect::<HashMap<_, _>>()
        );
    }

    #[test]
    fn publishes_funds_distributed_event() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let mut config = DistributorConfig {
            conditions: vec![Condition::BalanceAvailable {
                address: env.contract.address.clone(),
                amount: Coin::new(1_000_u128, "rune"),
            }],
            ..default_config()
        };

        CONFIG.save(&mut deps.as_mut(), &env, &mut config).unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

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
                        amount: vec![Coin::new(
                            balance
                                .amount
                                .mul_floor(Decimal::from_ratio(d.shares, total_shares)),
                            "rune"
                        )],
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

    use crate::test::default_config;

    #[test]
    fn returns_unauthorised_when_sender_not_owner() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG
            .save(&mut deps.as_mut(), &env, &mut default_config())
            .unwrap();

        let response = execute(
            deps.as_mut(),
            env,
            message_info(&Addr::unchecked("not_owner"), &[]),
            DistributorExecuteMsg::Withdraw {
                amounts: vec![Coin::new(1000u128, "rune")],
            },
        )
        .unwrap_err();

        assert_eq!(response.to_string(), "Unauthorized");
    }

    #[test]
    fn withdraws_funds_correctly() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let mut config = default_config();

        CONFIG.save(&mut deps.as_mut(), &env, &mut config).unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

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

    #[test]
    fn updates_statistics() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let mut config = default_config();

        CONFIG.save(&mut deps.as_mut(), &env, &mut config).unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &DistributorStatistics {
                    distributed: HashMap::new(),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        deps.querier.bank.update_balance(
            &env.contract.address,
            vec![
                Coin::new(1_000_u128, "rune"),
                Coin::new(1_000_u128, "uruji"),
                Coin::new(1_000_u128, "btc-btc"),
            ],
        );

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&config.owner, &[]),
            DistributorExecuteMsg::Withdraw {
                amounts: vec![Coin::new(1_000_u128, "rune"), Coin::new(500_u128, "uruji")],
            },
        )
        .unwrap();

        assert_eq!(
            STATS.load(deps.as_mut().storage).unwrap(),
            DistributorStatistics {
                distributed: HashMap::new(),
                withdrawn: vec![Coin::new(1_000_u128, "rune"), Coin::new(500_u128, "uruji")],
            },
        )
    }
}
