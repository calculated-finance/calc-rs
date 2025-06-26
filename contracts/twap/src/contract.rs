use std::cmp::min;

use calc_rs::types::{
    Affiliate, Callback, Condition, Contract, ContractError, ContractResult, CreateTrigger,
    DcaInstantiateMsg, Destination, DistributorConfig, DistributorExecuteMsg, DomainEvent,
    ExchangeExecuteMsg, ManagerExecuteMsg, ManagerQueryMsg, Recipient, SchedulerExecuteMsg,
    StrategyStatus, TriggerConditionsThreshold, TwapConfig, TwapExecuteMsg, TwapQueryMsg,
    TwapStatistics,
};
#[cfg(test)]
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    instantiate2_address, to_json_binary, BankMsg, Binary, Coin, Coins, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Reply, Response, StdResult, SubMsg, Uint128, WasmMsg,
};

use crate::state::{CONFIG, STATE, STATS};

const BASE_FEE_BPS: u64 = 15;
const EXECUTE_REPLY_ID: u64 = 1;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: DcaInstantiateMsg,
) -> ContractResult {
    if info.sender != msg.manager_contract {
        return Err(ContractError::Unauthorized {});
    }

    let total_shares = msg
        .mutable_destinations
        .iter()
        .chain(msg.immutable_destinations.iter())
        .into_iter()
        .fold(Uint128::zero(), |acc, d| acc + d.shares);

    let total_shares_with_fees = total_shares.mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

    let fee_destinations = match msg.affiliate_code.clone() {
        Some(code) => {
            let affiliate = deps.querier.query_wasm_smart::<Affiliate>(
                msg.manager_contract.clone(),
                &ManagerQueryMsg::Affiliate { code },
            )?;

            if affiliate.bps > 7 {
                return Err(ContractError::generic_err(
                    "Affiliate BPS cannot be greater than 7",
                ));
            }

            vec![
                Destination {
                    recipient: Recipient::Bank {
                        address: msg.fee_collector,
                    },
                    shares: total_shares_with_fees
                        .mul_floor(Decimal::bps(BASE_FEE_BPS - affiliate.bps)),
                    label: Some("CALC".to_string()),
                },
                Destination {
                    recipient: Recipient::Bank {
                        address: affiliate.address,
                    },
                    shares: total_shares_with_fees.mul_floor(Decimal::bps(affiliate.bps)),
                    label: Some(format!("Affiliate: {}", affiliate.code).to_string()),
                },
            ]
        }
        None => vec![Destination {
            recipient: Recipient::Bank {
                address: msg.fee_collector,
            },
            shares: total_shares_with_fees.mul_floor(Decimal::bps(BASE_FEE_BPS)),
            label: Some("CALC".to_string()),
        }],
    };

    let salt = to_json_binary(&(msg.owner.clone(), env.block.time, msg.distributor_code_id))?;

    let distributor_address = deps.api.addr_humanize(&instantiate2_address(
        &deps
            .querier
            .query_wasm_code_info(msg.distributor_code_id)?
            .checksum
            .as_slice(),
        &deps.api.addr_canonicalize(env.contract.address.as_str())?,
        &salt,
    )?)?;

    let instantiate_distributor_msg = WasmMsg::Instantiate2 {
        admin: Some(env.contract.address.to_string()),
        code_id: msg.distributor_code_id,
        label: "Distributor".to_string(),
        msg: to_json_binary(&DistributorConfig {
            owner: env.contract.address.clone(),
            denoms: vec![msg.minimum_receive_amount.denom.clone()],
            mutable_destinations: msg.mutable_destinations,
            immutable_destinations: [msg.immutable_destinations, fee_destinations].concat(),
            conditions: msg.minimum_distribute_amount.map_or(vec![], |amount| {
                vec![Condition::BalanceAvailable {
                    address: distributor_address.clone(),
                    amount,
                }]
            }),
        })?,
        funds: vec![],
        salt,
    };

    CONFIG.save(
        deps.storage,
        &env,
        &TwapConfig {
            owner: msg.owner,
            manager_contract: msg.manager_contract,
            exchanger_contract: msg.exchanger_contract,
            scheduler_contract: msg.scheduler_contract,
            distributor_contract: distributor_address.clone(),
            swap_amount: msg.swap_amount.clone(),
            minimum_receive_amount: msg.minimum_receive_amount,
            maximum_slippage_bps: msg.maximum_slippage_bps,
            swap_cadence: msg.swap_cadence,
            swap_conditions: vec![],
            schedule_conditions: vec![],
            execution_rebate: msg.execution_rebate,
        },
    )?;

    let execute_msg = Contract(env.contract.address.clone())
        .call(to_json_binary(&TwapExecuteMsg::Execute {})?, vec![]);

    let strategy_instantiated_event = DomainEvent::StrategyInstantiated {
        contract_address: env.contract.address,
        config: CONFIG.load(deps.storage)?,
    };

    STATS.save(
        deps.storage,
        &TwapStatistics {
            amount_swapped: Coin::new(0u128, msg.swap_amount.denom),
        },
    )?;

    Ok(Response::default()
        .add_message(instantiate_distributor_msg)
        .add_message(execute_msg)
        .add_event(strategy_instantiated_event))
}

#[entry_point]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: TwapExecuteMsg) -> ContractResult {
    let state = STATE.may_load(deps.storage)?;

    if let Some(state) = state {
        if msg == state {
            return Err(ContractError::generic_err(
                "Contract is already in the requested state",
            ));
        }
    }

    match msg {
        TwapExecuteMsg::Clear {} => {}
        _ => STATE.save(deps.storage, &msg)?,
    }

    let mut config = CONFIG.load(deps.storage)?;

    println!("Config: {:?}", config);

    if info.funds.len() > 1
        || (info.funds.len() == 1 && info.funds[0].denom != config.swap_amount.denom)
    {
        return Err(ContractError::generic_err(format!(
            "Invalid funds provided, only {} is allowed",
            config.swap_amount.denom
        )));
    }

    let mut stats = STATS.load(deps.storage)?;

    let mut messages: Vec<CosmosMsg> = vec![];
    let mut sub_messages: Vec<SubMsg> = vec![];
    let mut events: Vec<DomainEvent> = vec![];

    match msg.clone() {
        TwapExecuteMsg::Update(new_config) => {
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            if new_config.manager_contract != config.manager_contract {
                return Err(ContractError::generic_err(
                    "Cannot change the manager contract",
                ));
            }

            if new_config.distributor_contract != config.distributor_contract {
                return Err(ContractError::generic_err(
                    "Cannot change the distributor contract",
                ));
            }

            if new_config.swap_amount.denom != config.swap_amount.denom {
                return Err(ContractError::generic_err(format!(
                    "Cannot change the swap amount denomination from {} to {}",
                    config.swap_amount.denom, new_config.swap_amount.denom
                )));
            }

            if new_config.minimum_receive_amount.denom != config.minimum_receive_amount.denom {
                return Err(ContractError::generic_err(format!(
                    "Cannot change the minimum receive amount denomination from {} to {}",
                    config.minimum_receive_amount.denom, new_config.minimum_receive_amount.denom
                )));
            }

            config = new_config;
        }
        TwapExecuteMsg::Execute {} => {
            if info.sender != config.manager_contract && info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            if !config.swap_cadence.is_due(&env) {
                return Err(ContractError::generic_err(format!(
                    "DCA strategy is not due for execution until {:?}",
                    config.swap_cadence.into_condition(&env).description()
                )));
            }

            let swap_checks = config
                .swap_conditions
                .iter()
                .map(|c| c.check(deps.as_ref(), &env))
                .collect::<Vec<_>>();

            if swap_checks.iter().all(|c| c.is_ok()) {
                let balance = deps.querier.query_balance(
                    env.contract.address.clone(),
                    config.swap_amount.denom.clone(),
                )?;

                let swap_amount = Coin {
                    denom: config.swap_amount.denom.clone(),
                    amount: min(balance.amount, config.swap_amount.amount),
                };

                let minimum_receive_amount =
                    config
                        .minimum_receive_amount
                        .amount
                        .mul_ceil(Decimal::from_ratio(
                            swap_amount.amount,
                            config.swap_amount.amount,
                        ));

                let swap_msg = Contract(config.exchanger_contract.clone()).call(
                    to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: Coin::new(
                            minimum_receive_amount,
                            config.minimum_receive_amount.denom.clone(),
                        ),
                        recipient: Some(config.distributor_contract.clone()),
                        on_complete: Some(Callback {
                            contract: config.distributor_contract.clone(),
                            msg: to_json_binary(&DistributorExecuteMsg::Distribute {})?,
                            execution_rebate: config
                                .execution_rebate
                                .clone()
                                .map_or(vec![], |f| vec![f]),
                        }),
                    })?,
                    vec![swap_amount.clone()],
                );

                sub_messages.push(SubMsg::reply_always(swap_msg, EXECUTE_REPLY_ID));

                stats.amount_swapped.amount += swap_amount.amount;
            } else {
                let execution_skipped_event = DomainEvent::ExecutionSkipped {
                    contract_address: env.contract.address.clone(),
                    reason: format!(
                        "Execution skipped due to the following reasons:\n* {}",
                        swap_checks
                            .into_iter()
                            .filter_map(|c| c.err())
                            .map(|e| e.to_string())
                            .collect::<Vec<_>>()
                            .join("\n *")
                    ),
                };

                events.push(execution_skipped_event);
            }

            let schedule_checks = config
                .schedule_conditions
                .iter()
                .map(|c| c.check(deps.as_ref(), &env))
                .collect::<Vec<_>>();

            if schedule_checks.iter().all(|c| c.is_ok()) {
                config.swap_cadence = config.swap_cadence.next(&env);

                let set_triggers_msg = Contract(config.scheduler_contract.clone()).call(
                    to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
                        conditions: vec![config.swap_cadence.into_condition(&env)],
                        threshold: TriggerConditionsThreshold::All,
                        to: config.manager_contract.clone(),
                        msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                            contract_address: env.contract.address.clone(),
                            msg: Some(to_json_binary(&TwapExecuteMsg::Execute {})?),
                        })?,
                    }]))?,
                    config.execution_rebate.clone().map_or(vec![], |c| vec![c]),
                );

                messages.push(set_triggers_msg);
            } else {
                let schedule_skipped_event = DomainEvent::SchedulingSkipped {
                    contract_address: env.contract.address.clone(),
                    reason: format!(
                        "Scheduling skipped due to the following reasons:\n* {}",
                        schedule_checks
                            .into_iter()
                            .filter_map(|c| c.err())
                            .map(|e| e.to_string())
                            .collect::<Vec<_>>()
                            .join("\n *")
                    ),
                };

                events.push(schedule_skipped_event);
            }
        }
        TwapExecuteMsg::Withdraw { amounts } => {
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            let mut withdrawals = Coins::default();

            for amount in amounts {
                let balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), amount.denom.clone())?;

                if balance.amount >= Uint128::zero() {
                    withdrawals.add(Coin::new(
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
        }
        TwapExecuteMsg::UpdateStatus(status) => {
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            match status {
                StrategyStatus::Active => {
                    let schedule_checks = config
                        .schedule_conditions
                        .iter()
                        .map(|c| c.check(deps.as_ref(), &env))
                        .collect::<Vec<_>>();

                    if schedule_checks.iter().all(|c| c.is_ok()) {
                        config.swap_cadence = config.swap_cadence.next(&env);

                        let set_triggers_msg = Contract(config.scheduler_contract.clone()).call(
                            to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![
                                CreateTrigger {
                                    conditions: vec![config.swap_cadence.into_condition(&env)],
                                    threshold: TriggerConditionsThreshold::All,
                                    to: config.manager_contract.clone(),
                                    msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                                        contract_address: env.contract.address.clone(),
                                        msg: Some(to_json_binary(&TwapExecuteMsg::Execute {})?),
                                    })?,
                                },
                            ]))?,
                            config.execution_rebate.clone().map_or(vec![], |c| vec![c]),
                        );

                        messages.push(set_triggers_msg);
                    } else {
                        let schedule_skipped_event = DomainEvent::SchedulingSkipped {
                            contract_address: env.contract.address.clone(),
                            reason: format!(
                                "Scheduling skipped due to the following reasons:\n* {}",
                                schedule_checks
                                    .into_iter()
                                    .filter_map(|c| c.err())
                                    .map(|e| e.to_string())
                                    .collect::<Vec<_>>()
                                    .join("\n *")
                            ),
                        };

                        events.push(schedule_skipped_event);
                    }
                }
                StrategyStatus::Paused | StrategyStatus::Archived => {
                    let set_triggers_msg = Contract(config.scheduler_contract.clone()).call(
                        to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![]))?,
                        vec![],
                    );

                    messages.push(set_triggers_msg);
                }
            }

            let update_status_msg = Contract(config.manager_contract.clone()).call(
                to_json_binary(&ManagerExecuteMsg::UpdateStatus {
                    status: status.clone(),
                })?,
                vec![],
            );

            let status_updated_event = DomainEvent::StrategyStatusUpdated {
                contract_address: env.contract.address.clone(),
                status,
            };

            events.push(status_updated_event);
            messages.push(update_status_msg);
        }
        TwapExecuteMsg::Clear {} => {
            if info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            STATE.remove(deps.storage);
        }
    };

    CONFIG.save(deps.storage, &env, &config)?;
    STATS.save(deps.storage, &stats)?;

    match msg {
        TwapExecuteMsg::Clear {} => {}
        _ => {
            messages.push(
                Contract(env.contract.address.clone())
                    .call(to_json_binary(&TwapExecuteMsg::Clear {})?, vec![]),
            );
        }
    }

    Ok(Response::default()
        .add_submessages(sub_messages)
        .add_messages(messages)
        .add_events(events))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, _reply: Reply) -> ContractResult {
    Ok(Response::default())
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: TwapQueryMsg) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;

    match msg {
        TwapQueryMsg::Config {} => to_json_binary(&config),
    }
}

#[cfg(test)]
#[cw_serde]
struct CodeInfoResponse {
    pub checksum: cosmwasm_std::Checksum,
    pub code_id: u64,
    pub creator: cosmwasm_std::Addr,
}

#[cfg(test)]
fn default_config() -> TwapConfig {
    let deps = cosmwasm_std::testing::mock_dependencies();
    TwapConfig {
        owner: deps.api.addr_make("owner"),
        manager_contract: deps.api.addr_make("manager"),
        exchanger_contract: deps.api.addr_make("exchanger"),
        scheduler_contract: deps.api.addr_make("scheduler"),
        distributor_contract: deps.api.addr_make("distributor"),
        swap_amount: Coin::new(1000u128, "rune"),
        minimum_receive_amount: Coin::new(900u128, "uruji"),
        maximum_slippage_bps: 100,
        swap_cadence: calc_rs::types::Schedule::Blocks {
            interval: 100,
            previous: None,
        },
        swap_conditions: vec![],
        schedule_conditions: vec![],
        execution_rebate: None,
    }
}

#[cfg(test)]
mod instantiate_tests {
    use std::vec;

    use calc_rs::types::{
        Affiliate, Condition, DcaInstantiateMsg, Destination, DistributorConfig, DomainEvent,
        ManagerQueryMsg, Recipient, Schedule, TwapExecuteMsg,
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        to_json_binary, Addr, Checksum, Coin, ContractResult, Decimal, Event, SubMsg, SystemResult,
        Uint128, WasmMsg, WasmQuery,
    };

    use crate::{
        contract::{instantiate, CodeInfoResponse, BASE_FEE_BPS},
        state::CONFIG,
    };

    fn default_instantiate_msg() -> DcaInstantiateMsg {
        let deps = mock_dependencies();
        DcaInstantiateMsg {
            owner: Addr::unchecked("owner"),
            swap_amount: Coin::new(1000u128, "rune"),
            minimum_receive_amount: Coin::new(900u128, "uruji"),
            maximum_slippage_bps: 100,
            swap_cadence: Schedule::Blocks {
                interval: 100,
                previous: None,
            },
            exchanger_contract: deps.api.addr_make("exchange"),
            scheduler_contract: deps.api.addr_make("scheduler"),
            distributor_code_id: 1,
            minimum_distribute_amount: None,
            fee_collector: deps.api.addr_make("fee_collector"),
            affiliate_code: None,
            mutable_destinations: vec![Destination {
                recipient: Recipient::Bank {
                    address: deps.api.addr_make("destination"),
                },
                shares: Uint128::new(10000),
                label: Some("Mutable Destination".to_string()),
            }],
            immutable_destinations: vec![],
            execution_rebate: None,
            manager_contract: deps.api.addr_make("manager"),
        }
    }

    #[test]
    fn adds_calc_fee_collector_destination() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let msg = default_instantiate_msg();
        let info = message_info(&msg.manager_contract, &[]);

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&CodeInfoResponse {
                    code_id: msg.distributor_code_id,
                    creator: Addr::unchecked("creator"),
                    checksum: Checksum::from_hex(
                        "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
                    )
                    .unwrap(),
                })
                .unwrap(),
            ))
        });

        let response = instantiate(deps.as_mut(), env.clone(), info, msg.clone()).unwrap();

        let total_shares_with_fees = msg
            .mutable_destinations
            .iter()
            .chain(msg.immutable_destinations.iter())
            .fold(Uint128::zero(), |acc, d| acc + d.shares)
            .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

        let calc_fee_collector_destination = Destination {
            recipient: Recipient::Bank {
                address: msg.fee_collector.clone(),
            },
            shares: total_shares_with_fees.mul_floor(Decimal::bps(BASE_FEE_BPS)),
            label: Some("CALC".to_string()),
        };

        assert_eq!(
            response.messages[0],
            SubMsg::new(WasmMsg::Instantiate2 {
                admin: Some(env.contract.address.to_string()),
                code_id: msg.distributor_code_id,
                label: "Distributor".to_string(),
                msg: to_json_binary(&DistributorConfig {
                    owner: env.contract.address.clone(),
                    denoms: vec![msg.minimum_receive_amount.denom.clone()],
                    mutable_destinations: msg.mutable_destinations,
                    immutable_destinations: vec![calc_fee_collector_destination],
                    conditions: vec![],
                })
                .unwrap(),
                funds: vec![],
                salt: to_json_binary(&(msg.owner, env.block.time, msg.distributor_code_id))
                    .unwrap(),
            })
        );
    }

    #[test]
    fn adds_affiliate_fee_collector_destination() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let msg = DcaInstantiateMsg {
            affiliate_code: Some("affiliate_code".to_string()),
            ..default_instantiate_msg()
        };
        let manager = msg.manager_contract.clone();
        let info = message_info(&manager, &[]);

        let affiliate_bps = 5;

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::CodeInfo { code_id } => {
                    assert_eq!(code_id, &msg.distributor_code_id);
                    to_json_binary(&CodeInfoResponse {
                        code_id: msg.distributor_code_id.clone(),
                        creator: Addr::unchecked("creator"),
                        checksum: Checksum::from_hex(
                            "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
                        )
                        .unwrap(),
                    })
                    .unwrap()
                }
                WasmQuery::Smart { contract_addr, msg } => {
                    assert_eq!(contract_addr, manager.as_str());
                    assert_eq!(
                        msg,
                        &to_json_binary(&ManagerQueryMsg::Affiliate {
                            code: "affiliate_code".to_string(),
                        })
                        .unwrap()
                    );
                    to_json_binary(&Affiliate {
                        code: "affiliate_code".to_string(),
                        address: deps.api.addr_make("affiliate"),
                        bps: affiliate_bps,
                    })
                    .unwrap()
                }
                _ => panic!("Unexpected query type"),
            }))
        });

        let response = instantiate(deps.as_mut(), env.clone(), info, msg.clone()).unwrap();

        let total_shares_with_fees = msg
            .mutable_destinations
            .iter()
            .chain(msg.immutable_destinations.iter())
            .fold(Uint128::zero(), |acc, d| acc + d.shares)
            .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

        let calc_fee_collector_destination = Destination {
            recipient: Recipient::Bank {
                address: msg.fee_collector.clone(),
            },
            shares: total_shares_with_fees.mul_floor(Decimal::bps(BASE_FEE_BPS - affiliate_bps)),
            label: Some("CALC".to_string()),
        };

        let affiliate_destination = Destination {
            recipient: Recipient::Bank {
                address: deps.api.addr_make("affiliate"),
            },
            shares: total_shares_with_fees.mul_floor(Decimal::bps(affiliate_bps)),
            label: Some(format!(
                "Affiliate: {}",
                msg.affiliate_code.clone().unwrap()
            )),
        };

        assert_eq!(
            response.messages[0],
            SubMsg::new(WasmMsg::Instantiate2 {
                admin: Some(env.contract.address.to_string()),
                code_id: msg.distributor_code_id,
                label: "Distributor".to_string(),
                msg: to_json_binary(&DistributorConfig {
                    owner: env.contract.address.clone(),
                    denoms: vec![msg.minimum_receive_amount.denom.clone()],
                    mutable_destinations: msg.mutable_destinations,
                    immutable_destinations: vec![
                        calc_fee_collector_destination,
                        affiliate_destination
                    ],
                    conditions: vec![],
                })
                .unwrap(),
                funds: vec![],
                salt: to_json_binary(&(msg.owner, env.block.time, msg.distributor_code_id))
                    .unwrap(),
            })
        );
    }

    #[test]
    fn adds_minimum_distribution_amount_condition() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let msg = DcaInstantiateMsg {
            minimum_distribute_amount: Some(Coin::new(1000u128, "rune")),
            ..default_instantiate_msg()
        };
        let manager = msg.manager_contract.clone();
        let info = message_info(&manager, &[]);

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&CodeInfoResponse {
                    code_id: msg.distributor_code_id.clone(),
                    creator: Addr::unchecked("creator"),
                    checksum: Checksum::from_hex(
                        "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
                    )
                    .unwrap(),
                })
                .unwrap(),
            ))
        });

        let response = instantiate(deps.as_mut(), env.clone(), info, msg.clone()).unwrap();
        let config = CONFIG.load(deps.as_ref().storage).unwrap();

        let total_shares_with_fees = msg
            .mutable_destinations
            .iter()
            .chain(msg.immutable_destinations.iter())
            .fold(Uint128::zero(), |acc, d| acc + d.shares)
            .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

        let calc_fee_collector_destination = Destination {
            recipient: Recipient::Bank {
                address: msg.fee_collector.clone(),
            },
            shares: total_shares_with_fees.mul_floor(Decimal::bps(BASE_FEE_BPS)),
            label: Some("CALC".to_string()),
        };

        assert_eq!(
            response.messages[0],
            SubMsg::new(WasmMsg::Instantiate2 {
                admin: Some(env.contract.address.to_string()),
                code_id: msg.distributor_code_id,
                label: "Distributor".to_string(),
                msg: to_json_binary(&DistributorConfig {
                    owner: env.contract.address.clone(),
                    denoms: vec![msg.minimum_receive_amount.denom.clone()],
                    mutable_destinations: msg.mutable_destinations,
                    immutable_destinations: vec![calc_fee_collector_destination],
                    conditions: vec![Condition::BalanceAvailable {
                        address: config.distributor_contract.clone(),
                        amount: msg.minimum_distribute_amount.unwrap(),
                    }],
                })
                .unwrap(),
                funds: vec![],
                salt: to_json_binary(&(msg.owner, env.block.time, msg.distributor_code_id))
                    .unwrap(),
            })
        );
    }

    #[test]
    fn adds_execute_msg() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let msg = default_instantiate_msg();
        let manager = msg.manager_contract.clone();
        let info = message_info(&manager, &[]);

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&CodeInfoResponse {
                    code_id: msg.distributor_code_id.clone(),
                    creator: Addr::unchecked("creator"),
                    checksum: Checksum::from_hex(
                        "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
                    )
                    .unwrap(),
                })
                .unwrap(),
            ))
        });

        let response = instantiate(deps.as_mut(), env.clone(), info, msg.clone()).unwrap();

        assert_eq!(
            response.messages[1],
            SubMsg::new(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_json_binary(&TwapExecuteMsg::Execute {}).unwrap(),
                funds: vec![]
            })
        );
    }

    #[test]
    fn adds_swap_conditions() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let msg = DcaInstantiateMsg {
            minimum_distribute_amount: Some(Coin::new(1000u128, "rune")),
            ..default_instantiate_msg()
        };
        let manager = msg.manager_contract.clone();
        let info = message_info(&manager, &[]);

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&CodeInfoResponse {
                    code_id: msg.distributor_code_id.clone(),
                    creator: Addr::unchecked("creator"),
                    checksum: Checksum::from_hex(
                        "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
                    )
                    .unwrap(),
                })
                .unwrap(),
            ))
        });

        instantiate(deps.as_mut(), env.clone(), info, msg.clone()).unwrap();
        let config = CONFIG.load(deps.as_ref().storage).unwrap();

        assert_eq!(
            config.swap_conditions,
            vec![
                match msg.swap_cadence {
                    Schedule::Blocks { interval, previous } => {
                        Condition::BlocksCompleted(previous.unwrap_or(env.block.height) + interval)
                    }
                    Schedule::Time { duration, previous } => Condition::TimestampElapsed(
                        previous
                            .unwrap_or(env.block.time)
                            .plus_seconds(duration.as_secs())
                    ),
                },
                Condition::BalanceAvailable {
                    address: env.contract.address.clone(),
                    amount: Coin::new(1u128, msg.swap_amount.denom.clone()),
                },
                Condition::ExchangeLiquidityProvided {
                    swap_amount: msg.swap_amount.clone(),
                    minimum_receive_amount: msg.minimum_receive_amount.clone(),
                    maximum_slippage_bps: msg.maximum_slippage_bps,
                },
            ]
        );
    }

    #[test]
    fn publishes_strategy_instantiated_event() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let msg = default_instantiate_msg();
        let manager = msg.manager_contract.clone();
        let info = message_info(&manager, &[]);

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&CodeInfoResponse {
                    code_id: msg.distributor_code_id.clone(),
                    creator: Addr::unchecked("creator"),
                    checksum: Checksum::from_hex(
                        "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
                    )
                    .unwrap(),
                })
                .unwrap(),
            ))
        });

        let response = instantiate(deps.as_mut(), env.clone(), info, msg.clone()).unwrap();
        let config = CONFIG.load(deps.as_ref().storage).unwrap();

        assert_eq!(
            response.events[0],
            Event::from(DomainEvent::StrategyInstantiated {
                contract_address: env.contract.address,
                config
            })
        );
    }
}

#[cfg(test)]
mod update_tests {
    use calc_rs::types::{ContractError, Schedule, TwapConfig, TwapExecuteMsg, TwapStatistics};
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr, Coin,
    };

    use crate::{
        contract::{default_config, execute},
        state::{CONFIG, STATE, STATS},
    };

    #[test]
    fn only_allows_owner_to_update() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("not-owner"), &[]),
                TwapExecuteMsg::Update(config.clone())
            )
            .unwrap_err(),
            ContractError::Unauthorized {}
        );

        STATE.remove(deps.as_mut().storage);

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.owner, &[]),
                TwapExecuteMsg::Update(config.clone())
            )
            .is_ok(),
            true
        );
    }

    #[test]
    fn cannot_update_manager_contract() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        let new_config = TwapExecuteMsg::Update(TwapConfig {
            manager_contract: Addr::unchecked("new-manager"),
            ..config.clone()
        });

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.owner, &[]),
                new_config
            )
            .unwrap_err(),
            ContractError::generic_err("Cannot change the manager contract")
        );
    }

    #[test]
    fn cannot_update_distributor_contract() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        let new_config = TwapExecuteMsg::Update(TwapConfig {
            distributor_contract: Addr::unchecked("new-distributor"),
            ..config.clone()
        });

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.owner, &[]),
                new_config
            )
            .unwrap_err(),
            ContractError::generic_err("Cannot change the distributor contract")
        );
    }

    #[test]
    fn cannot_update_swap_amount_denomination() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        let new_config = TwapExecuteMsg::Update(TwapConfig {
            swap_amount: Coin::new(0u128, "new-denom"),
            ..config.clone()
        });

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.owner, &[]),
                new_config
            )
            .unwrap_err(),
            ContractError::generic_err("Cannot change the swap amount denomination")
        );
    }

    #[test]
    fn cannot_update_minimum_receive_amount_denomination() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        let new_config = TwapExecuteMsg::Update(TwapConfig {
            minimum_receive_amount: Coin::new(0u128, "new-denom"),
            ..config.clone()
        });

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.owner, &[]),
                new_config
            )
            .unwrap_err(),
            ContractError::generic_err("Cannot change the minimum receive amount denomination")
        );
    }

    #[test]
    fn cannot_set_maximum_slippage_bps_to_more_than_10_000() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        let new_config = TwapExecuteMsg::Update(TwapConfig {
            maximum_slippage_bps: 10_001,
            ..config.clone()
        });

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.owner, &[]),
                new_config
            )
            .unwrap_err(),
            ContractError::generic_err("Maximum slippage basis points cannot exceed 10,000 (100%)")
        );
    }

    #[test]
    fn updates_config() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        let new_config = TwapConfig {
            swap_amount: Coin::new(32867423_u128, config.swap_amount.denom.clone()),
            minimum_receive_amount: Coin::new(
                32867423_u128,
                config.minimum_receive_amount.denom.clone(),
            ),
            maximum_slippage_bps: 8767,
            swap_cadence: Schedule::Blocks {
                interval: 236473,
                previous: Some(1265),
            },
            ..config.clone()
        };

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&config.owner, &[]),
            TwapExecuteMsg::Update(new_config.clone()),
        )
        .unwrap();

        assert_eq!(CONFIG.load(deps.as_ref().storage).unwrap(), new_config);
    }
}

#[cfg(test)]
mod execute_tests {
    use calc_rs::types::{
        Callback, Condition, ContractError, CreateTrigger, DistributorExecuteMsg, DomainEvent,
        ExchangeExecuteMsg, ManagerExecuteMsg, Schedule, SchedulerExecuteMsg,
        TriggerConditionsThreshold, TwapConfig, TwapExecuteMsg, TwapStatistics,
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        to_json_binary, Addr, Coin, Event, SubMsg, WasmMsg,
    };

    use crate::{
        contract::{default_config, execute, EXECUTE_REPLY_ID},
        state::{CONFIG, STATE, STATS},
    };

    #[test]
    fn prevents_recursive_execution() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&config.manager_contract, &[]),
            TwapExecuteMsg::Execute {},
        )
        .unwrap();

        assert!(response.messages.contains(&SubMsg::new(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&TwapExecuteMsg::Clear {}).unwrap(),
            funds: vec![]
        })));

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                TwapExecuteMsg::Execute {}
            )
            .unwrap_err(),
            ContractError::generic_err("Contract is already in the requested state")
        );

        assert_eq!(
            STATE.load(deps.as_ref().storage).unwrap(),
            TwapExecuteMsg::Execute {}
        );
    }

    #[test]
    fn clears_state() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        STATE
            .save(deps.as_mut().storage, &TwapExecuteMsg::Execute {})
            .unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&env.contract.address, &[]),
            TwapExecuteMsg::Clear {},
        )
        .unwrap();

        assert!(STATE.may_load(deps.as_ref().storage).unwrap().is_none());
    }

    #[test]
    fn only_allows_manager_and_contract_owner_to_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("not-manager"), &[]),
                TwapExecuteMsg::Execute {}
            )
            .unwrap_err(),
            ContractError::Unauthorized {}
        );

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
        STATE.remove(deps.as_mut().storage);

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                TwapExecuteMsg::Execute {}
            )
            .is_ok(),
            true
        );

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
        STATE.remove(deps.as_mut().storage);

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&env.contract.address, &[]),
                TwapExecuteMsg::Execute {}
            )
            .is_ok(),
            true
        );
    }

    #[test]
    fn only_allows_valid_funds() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(
                    &config.manager_contract,
                    &[Coin::new(1000u128, config.swap_amount.denom.clone())]
                ),
                TwapExecuteMsg::Execute {}
            )
            .is_ok(),
            true
        );

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
        STATE.remove(deps.as_mut().storage);

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&env.contract.address, &[Coin::new(500u128, "random")]),
                TwapExecuteMsg::Execute {}
            )
            .unwrap_err(),
            ContractError::generic_err(format!(
                "Invalid funds provided, only {} is allowed",
                config.swap_amount.denom
            ))
        );

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
        STATE.remove(deps.as_mut().storage);

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(
                    &env.contract.address,
                    &[
                        Coin::new(1000u128, config.swap_amount.denom.clone()),
                        Coin::new(500u128, "random")
                    ]
                ),
                TwapExecuteMsg::Execute {}
            )
            .unwrap_err(),
            ContractError::generic_err(format!(
                "Invalid funds provided, only {} is allowed",
                config.swap_amount.denom
            ))
        );
    }

    #[test]
    fn returns_error_if_not_due_for_execution() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = TwapConfig {
            swap_cadence: Schedule::Blocks {
                interval: 100,
                previous: Some(env.block.height + 50),
            },
            ..default_config()
        };

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                TwapExecuteMsg::Execute {}
            )
            .unwrap_err(),
            ContractError::generic_err(format!(
                "DCA strategy is not due for execution until {:?}",
                config.swap_cadence.into_condition(&env).description()
            ))
        );
    }

    #[test]
    fn executes_swap_if_conditions_met() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![config.swap_amount.clone()],
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                TwapExecuteMsg::Execute {}
            )
            .unwrap()
            .messages[0],
            SubMsg::reply_always(
                WasmMsg::Execute {
                    contract_addr: config.exchanger_contract.to_string(),
                    msg: to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: config.minimum_receive_amount.clone(),
                        recipient: Some(config.distributor_contract.clone()),
                        on_complete: Some(Callback {
                            contract: config.distributor_contract.clone(),
                            msg: to_json_binary(&DistributorExecuteMsg::Distribute {}).unwrap(),
                            execution_rebate: config
                                .execution_rebate
                                .clone()
                                .map_or(vec![], |f| vec![f]),
                        }),
                    })
                    .unwrap(),
                    funds: vec![config.swap_amount.clone()],
                },
                EXECUTE_REPLY_ID
            )
        );
    }

    #[test]
    fn skips_execution_if_conditions_not_met() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = TwapConfig {
            swap_conditions: vec![Condition::BalanceAvailable {
                address: env.contract.address.clone(),
                amount: default_config().swap_amount,
            }],
            ..default_config()
        };

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                TwapExecuteMsg::Execute {}
            )
            .unwrap()
            .events[0],
            Event::from(DomainEvent::ExecutionSkipped {
                contract_address: env.contract.address.clone(),
                reason: format!(
                    "Execution skipped due to the following reasons:\n* {}",
                    config
                        .swap_conditions
                        .iter()
                        .map(|c| c.check(deps.as_ref(), &env).unwrap_err().to_string())
                        .collect::<Vec<_>>()
                        .join("\n *")
                )
            })
        );
    }

    #[test]
    fn schedules_next_execution_if_conditions_met() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = TwapConfig {
            schedule_conditions: vec![Condition::BalanceAvailable {
                address: env.contract.address.clone(),
                amount: default_config().swap_amount,
            }],
            ..default_config()
        };

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![config.swap_amount.clone()],
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                TwapExecuteMsg::Execute {}
            )
            .unwrap()
            .messages[1],
            SubMsg::new(WasmMsg::Execute {
                contract_addr: config.scheduler_contract.to_string(),
                msg: to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
                    conditions: vec![config.swap_cadence.into_condition(&env)],
                    threshold: TriggerConditionsThreshold::All,
                    to: config.manager_contract.clone(),
                    msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                        contract_address: env.contract.address.clone(),
                        msg: Some(to_json_binary(&TwapExecuteMsg::Execute {}).unwrap()),
                    })
                    .unwrap(),
                }]))
                .unwrap(),
                funds: config.execution_rebate.map_or(vec![], |c| vec![c])
            })
        );
    }

    #[test]
    fn skips_scheduling_if_conditions_not_met() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = TwapConfig {
            schedule_conditions: vec![Condition::BalanceAvailable {
                address: env.contract.address.clone(),
                amount: default_config().swap_amount,
            }],
            ..default_config()
        };

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                TwapExecuteMsg::Execute {}
            )
            .unwrap()
            .events[0],
            Event::from(DomainEvent::SchedulingSkipped {
                contract_address: env.contract.address.clone(),
                reason: format!(
                    "Scheduling skipped due to the following reasons:\n* {}",
                    config
                        .schedule_conditions
                        .iter()
                        .map(|c| c.check(deps.as_ref(), &env).unwrap_err().to_string())
                        .collect::<Vec<_>>()
                        .join("\n *")
                )
            })
        );
    }

    #[test]
    fn updates_statistics() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![config.swap_amount.clone()],
        );

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&config.manager_contract, &[]),
            TwapExecuteMsg::Execute {},
        )
        .unwrap();

        let stats = STATS.load(deps.as_ref().storage).unwrap();
        assert_eq!(stats.amount_swapped, config.swap_amount);
    }
}

#[cfg(test)]
mod withdraw_tests {
    use calc_rs::types::{ContractError, TwapExecuteMsg, TwapStatistics};
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Coin,
    };

    use crate::{
        contract::{default_config, execute},
        state::{CONFIG, STATE, STATS},
    };

    #[test]
    fn only_allows_owner_to_withdraw() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    amount_swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                TwapExecuteMsg::Withdraw { amounts: vec![] }
            )
            .unwrap_err(),
            ContractError::Unauthorized {}
        );

        STATE.remove(deps.as_mut().storage);

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&env.contract.address, &[]),
                TwapExecuteMsg::Withdraw { amounts: vec![] }
            )
            .unwrap_err(),
            ContractError::Unauthorized {}
        );

        STATE.remove(deps.as_mut().storage);

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.owner, &[]),
                TwapExecuteMsg::Withdraw { amounts: vec![] }
            )
            .is_ok(),
            true
        );
    }
}
