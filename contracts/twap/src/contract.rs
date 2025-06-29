use std::cmp::min;

use calc_rs::{
    core::{Callback, Condition, Contract, ContractError, ContractResult, StrategyStatus},
    distributor::{
        Destination, DistributorConfig, DistributorExecuteMsg, DistributorQueryMsg,
        DistributorStatistics, Recipient,
    },
    exchanger::ExchangeExecuteMsg,
    manager::{
        Affiliate, CreateStrategyConfig, ManagerExecuteMsg, ManagerQueryMsg, StrategyConfig,
        StrategyExecuteMsg, StrategyInstantiateMsg, StrategyQueryMsg, StrategyStatistics,
    },
    scheduler::{CreateTrigger, SchedulerExecuteMsg, TriggerConditionsThreshold},
    twap::TwapConfig,
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    instantiate2_address, to_json_binary, BankMsg, Binary, Coin, Coins, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Reply, Response, StdResult, SubMsg, SubMsgResult, Uint128, WasmMsg,
};

use crate::{
    state::{CONFIG, STATE, STATS},
    types::{DomainEvent, TwapStatistics},
};

const BASE_FEE_BPS: u64 = 15;
const EXECUTE_REPLY_ID: u64 = 1;
const SCHEDULE_REPLY_ID: u64 = 2;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    match msg.config {
        CreateStrategyConfig::Twap(config) => {
            let total_shares = config
                .mutable_destinations
                .iter()
                .chain(config.immutable_destinations.iter())
                .into_iter()
                .fold(Uint128::zero(), |acc, d| acc + d.shares);

            let total_shares_with_fees = total_shares.mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

            let fee_destinations = match config.affiliate_code.clone() {
                Some(code) => {
                    let affiliate = deps.querier.query_wasm_smart::<Affiliate>(
                        info.sender.clone(),
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

            let salt = to_json_binary(&(
                env.contract.address.to_string().truncate(16),
                env.block.time.seconds(),
                config.distributor_code_id,
            ))?;

            let distributor_address = deps.api.addr_humanize(&instantiate2_address(
                &deps
                    .querier
                    .query_wasm_code_info(config.distributor_code_id)?
                    .checksum
                    .as_slice(),
                &deps.api.addr_canonicalize(env.contract.address.as_str())?,
                &salt,
            )?)?;

            let instantiate_distributor_msg = WasmMsg::Instantiate2 {
                admin: Some(env.contract.address.to_string()),
                code_id: config.distributor_code_id,
                label: "Distributor".to_string(),
                msg: to_json_binary(&DistributorConfig {
                    owner: config.owner.clone(),
                    denoms: vec![config.minimum_receive_amount.denom.clone()],
                    mutable_destinations: config.mutable_destinations,
                    immutable_destinations: [config.immutable_destinations, fee_destinations]
                        .concat(),
                    conditions: config.minimum_distribute_amount.map_or(vec![], |amount| {
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
                    owner: config.owner,
                    manager_contract: info.sender,
                    exchanger_contract: config.exchanger_contract,
                    scheduler_contract: config.scheduler_contract,
                    distributor_contract: distributor_address.clone(),
                    swap_amount: config.swap_amount.clone(),
                    minimum_receive_amount: config.minimum_receive_amount,
                    maximum_slippage_bps: config.maximum_slippage_bps,
                    route: config.route,
                    swap_cadence: config.swap_cadence,
                    swap_conditions: vec![],
                    schedule_conditions: vec![],
                    execution_rebate: config.execution_rebate,
                },
            )?;

            let execute_msg = Contract(env.contract.address.clone())
                .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, vec![]);

            let strategy_instantiated_event = DomainEvent::TwapStrategyCreated {
                contract_address: env.contract.address,
                config: CONFIG.load(deps.storage)?,
            };

            STATS.save(
                deps.storage,
                &TwapStatistics {
                    swapped: Coin::new(0u128, config.swap_amount.denom),
                    withdrawn: vec![],
                },
            )?;

            Ok(Response::default()
                .add_message(instantiate_distributor_msg)
                .add_message(execute_msg)
                .add_event(strategy_instantiated_event))
        }
    }
}

#[cw_serde]
pub struct MigrateMsg {}

#[entry_point]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> ContractResult {
    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyExecuteMsg,
) -> ContractResult {
    let state = STATE.may_load(deps.storage)?;

    if let Some(state) = state {
        if msg == state {
            return Err(ContractError::generic_err(
                "Contract is already in the requested state",
            ));
        }
    }

    match msg {
        StrategyExecuteMsg::Clear {} => {}
        _ => STATE.save(deps.storage, &msg)?,
    }

    let mut config = CONFIG.load(deps.storage)?;

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
        StrategyExecuteMsg::Update(new_config) => {
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            match new_config {
                StrategyConfig::Twap(new_config) => {
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

                    if new_config.minimum_receive_amount.denom
                        != config.minimum_receive_amount.denom
                    {
                        return Err(ContractError::generic_err(format!(
                            "Cannot change the minimum receive amount denomination from {} to {}",
                            config.minimum_receive_amount.denom,
                            new_config.minimum_receive_amount.denom
                        )));
                    }

                    let updated_event = DomainEvent::TwapStrategyUpdated {
                        contract_address: env.contract.address.clone(),
                        old_config: config.clone(),
                        new_config: new_config.clone(),
                    };

                    events.push(updated_event);

                    config = new_config;
                }
            }
        }
        StrategyExecuteMsg::Execute {} => {
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
                        maximum_slippage_bps: config.maximum_slippage_bps,
                        route: config.route.clone(),
                        // send funds to the distributor contract
                        recipient: Some(config.distributor_contract.clone()),
                        // callback to the distributor contract after swap
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

                let execution_attempted_event = DomainEvent::TwapExecutionAttempted {
                    contract_address: env.contract.address.clone(),
                    swap_amount: swap_amount.clone(),
                    minimum_receive_amount: config.minimum_receive_amount.clone(),
                    maximum_slippage_bps: config.maximum_slippage_bps,
                };

                events.push(execution_attempted_event);

                stats.swapped.amount += swap_amount.amount;
            } else {
                let execution_skipped_event = DomainEvent::TwapExecutionSkipped {
                    contract_address: env.contract.address.clone(),
                    reason: format!(
                        "Execution skipped due to the following reasons:\n* {}",
                        swap_checks
                            .into_iter()
                            .filter_map(|c| c.err())
                            .map(|e| e.to_string())
                            .collect::<Vec<_>>()
                            .join("\n* ")
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
                let trigger_conditions = vec![config.swap_cadence.into_condition(&env)];

                let set_triggers_msg = Contract(config.scheduler_contract.clone()).call(
                    // Set triggers to execute the strategy when next scheduled
                    to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
                        conditions: trigger_conditions.clone(),
                        threshold: TriggerConditionsThreshold::All,
                        to: config.manager_contract.clone(),
                        msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                            contract_address: env.contract.address.clone(),
                            msg: Some(to_json_binary(&StrategyExecuteMsg::Execute {})?),
                        })?,
                    }]))?,
                    config.execution_rebate.clone().map_or(vec![], |c| vec![c]),
                );

                sub_messages.push(SubMsg::reply_always(set_triggers_msg, SCHEDULE_REPLY_ID));

                let scheduling_attempted_event = DomainEvent::TwapSchedulingAttempted {
                    contract_address: env.contract.address.clone(),
                    conditions: trigger_conditions.clone(),
                };

                events.push(scheduling_attempted_event);
            } else {
                let schedule_skipped_event = DomainEvent::TwapSchedulingSkipped {
                    contract_address: env.contract.address.clone(),
                    reason: format!(
                        "Scheduling skipped due to the following reasons:\n* {}",
                        schedule_checks
                            .into_iter()
                            .filter_map(|c| c.err())
                            .map(|e| e.to_string())
                            .collect::<Vec<_>>()
                            .join("\n* ")
                    ),
                };

                events.push(schedule_skipped_event);
            }
        }
        StrategyExecuteMsg::Withdraw { amounts } => {
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

            let funds_withdrawn_event = DomainEvent::TwapFundsWithdrawn {
                contract_address: env.contract.address.clone(),
                to: config.owner.clone(),
                funds: withdrawals.to_vec(),
            };

            events.push(funds_withdrawn_event);

            stats.withdrawn = amount_withdrawn.to_vec();
        }
        StrategyExecuteMsg::UpdateStatus(status) => {
            if info.sender != config.manager_contract {
                return Err(ContractError::Unauthorized {});
            }

            match status {
                StrategyStatus::Active => {
                    let execute_msg = Contract(env.contract.address.clone())
                        .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, vec![]);

                    messages.push(execute_msg);
                }
                StrategyStatus::Paused | StrategyStatus::Archived => {
                    let set_triggers_msg = Contract(config.scheduler_contract.clone()).call(
                        to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![]))?,
                        vec![],
                    );

                    messages.push(set_triggers_msg);
                }
            }
        }
        StrategyExecuteMsg::Clear {} => {
            if info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            STATE.remove(deps.storage);
        }
    };

    CONFIG.save(deps.storage, &env, &config)?;
    STATS.save(deps.storage, &stats)?;

    match msg {
        StrategyExecuteMsg::Clear {} => {}
        _ => {
            messages.push(
                Contract(env.contract.address.clone())
                    .call(to_json_binary(&StrategyExecuteMsg::Clear {})?, vec![]),
            );
        }
    }

    Ok(Response::default()
        .add_submessages(sub_messages)
        .add_messages(messages)
        .add_events(events))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, env: Env, reply: Reply) -> ContractResult {
    let mut events: Vec<DomainEvent> = vec![];

    match reply.id {
        EXECUTE_REPLY_ID => match reply.result {
            SubMsgResult::Ok(_) => {
                let execution_succeeded_event = DomainEvent::TwapExecutionSucceeded {
                    contract_address: env.contract.address.clone(),
                    statistics: STATS.load(_deps.storage)?,
                };

                events.push(execution_succeeded_event);
            }
            SubMsgResult::Err(err) => {
                let execution_failed_event = DomainEvent::TwapExecutionFailed {
                    contract_address: env.contract.address.clone(),
                    reason: err.to_string(),
                };

                events.push(execution_failed_event);
            }
        },
        SCHEDULE_REPLY_ID => match reply.result {
            SubMsgResult::Ok(_) => {
                let scheduling_succeeded_event = DomainEvent::TwapSchedulingSucceeded {
                    contract_address: env.contract.address.clone(),
                };

                events.push(scheduling_succeeded_event);
            }
            SubMsgResult::Err(err) => {
                let scheduling_failed_event = DomainEvent::TwapSchedulingFailed {
                    contract_address: env.contract.address.clone(),
                    reason: err.to_string(),
                };

                events.push(scheduling_failed_event);
            }
        },
        _ => {}
    }

    Ok(Response::default().add_events(events))
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: StrategyQueryMsg) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;
    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&config),
        StrategyQueryMsg::Statistics {} => {
            let distributor_stats = deps.querier.query_wasm_smart::<DistributorStatistics>(
                config.distributor_contract.clone(),
                &DistributorQueryMsg::Statistics {},
            )?;

            let stats = STATS.load(deps.storage)?;

            let remaining = deps.querier.query_balance(
                env.contract.address.clone(),
                config.swap_amount.denom.clone(),
            )?;

            let received = deps.querier.query_balance(
                config.distributor_contract.clone(),
                config.minimum_receive_amount.denom.clone(),
            )?;

            let mut withdrawn = Coins::try_from(stats.withdrawn)?;

            for amount in distributor_stats.withdrawn {
                withdrawn.add(amount)?;
            }

            to_json_binary(&StrategyStatistics::Twap {
                remaining,
                swapped: stats.swapped,
                received,
                distributed: distributor_stats.distributed,
                withdrawn: withdrawn.to_vec(),
            })
        }
    }
}

// We defining our own CodeInfoResponse because the library one restricts creation
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
        route: None,
        swap_cadence: calc_rs::core::Schedule::Blocks {
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
    use super::*;

    use calc_rs::{
        core::{Condition, Schedule},
        twap::InstantiateTwapCommand,
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        to_json_binary, Addr, Checksum, Coin, ContractResult, Decimal, Event, SubMsg, SystemResult,
        Uint128, WasmMsg, WasmQuery,
    };

    use crate::{
        contract::{instantiate, CodeInfoResponse, BASE_FEE_BPS},
        state::CONFIG,
        types::DomainEvent,
    };

    fn default_instantiate_msg() -> InstantiateTwapCommand {
        let deps = mock_dependencies();
        InstantiateTwapCommand {
            owner: Addr::unchecked("owner"),
            exchanger_contract: deps.api.addr_make("exchange"),
            scheduler_contract: deps.api.addr_make("scheduler"),
            swap_amount: Coin::new(1000u128, "rune"),
            minimum_receive_amount: Coin::new(900u128, "uruji"),
            maximum_slippage_bps: 100,
            route: None,
            swap_cadence: Schedule::Blocks {
                interval: 100,
                previous: None,
            },
            distributor_code_id: 1,
            minimum_distribute_amount: None,
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
        }
    }

    #[test]
    fn adds_calc_fee_collector_destination() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let msg = default_instantiate_msg();
        let info = message_info(&deps.api.addr_make("manager"), &[]);

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

        let fee_collector = Addr::unchecked("fee-collector");

        let response = instantiate(
            deps.as_mut(),
            env.clone(),
            info,
            StrategyInstantiateMsg {
                fee_collector: fee_collector.clone(),
                config: CreateStrategyConfig::Twap(msg.clone()),
            },
        )
        .unwrap();

        let total_shares_with_fees = msg
            .mutable_destinations
            .iter()
            .chain(msg.immutable_destinations.iter())
            .fold(Uint128::zero(), |acc, d| acc + d.shares)
            .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

        let calc_fee_collector_destination = Destination {
            recipient: Recipient::Bank {
                address: fee_collector.clone(),
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
                    owner: msg.owner.clone(),
                    denoms: vec![msg.minimum_receive_amount.denom.clone()],
                    mutable_destinations: msg.mutable_destinations,
                    immutable_destinations: vec![calc_fee_collector_destination],
                    conditions: vec![],
                })
                .unwrap(),
                funds: vec![],
                salt: to_json_binary(&(
                    msg.owner.to_string().truncate(16),
                    env.block.time.seconds(),
                    msg.distributor_code_id
                ))
                .unwrap(),
            })
        );
    }

    #[test]
    fn adds_affiliate_fee_collector_destination() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let msg = InstantiateTwapCommand {
            affiliate_code: Some("affiliate_code".to_string()),
            ..default_instantiate_msg()
        };
        let manager = deps.api.addr_make("manager");
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

        let fee_collector = Addr::unchecked("fee-collector");

        let response = instantiate(
            deps.as_mut(),
            env.clone(),
            info,
            StrategyInstantiateMsg {
                fee_collector: fee_collector.clone(),
                config: CreateStrategyConfig::Twap(msg.clone()),
            },
        )
        .unwrap();

        let total_shares_with_fees = msg
            .mutable_destinations
            .iter()
            .chain(msg.immutable_destinations.iter())
            .fold(Uint128::zero(), |acc, d| acc + d.shares)
            .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

        let calc_fee_collector_destination = Destination {
            recipient: Recipient::Bank {
                address: fee_collector.clone(),
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
                    owner: msg.owner.clone(),
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
                salt: to_json_binary(&(
                    msg.owner.to_string().truncate(16),
                    env.block.time.seconds(),
                    msg.distributor_code_id
                ))
                .unwrap(),
            })
        );
    }

    #[test]
    fn adds_minimum_distribution_amount_condition() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let msg = InstantiateTwapCommand {
            minimum_distribute_amount: Some(Coin::new(1000u128, "rune")),
            ..default_instantiate_msg()
        };
        let manager = deps.api.addr_make("manager");
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

        let fee_collector = Addr::unchecked("fee-collector");

        let response = instantiate(
            deps.as_mut(),
            env.clone(),
            info,
            StrategyInstantiateMsg {
                fee_collector: fee_collector.clone(),
                config: CreateStrategyConfig::Twap(msg.clone()),
            },
        )
        .unwrap();

        let config = CONFIG.load(deps.as_ref().storage).unwrap();

        let total_shares_with_fees = msg
            .mutable_destinations
            .iter()
            .chain(msg.immutable_destinations.iter())
            .fold(Uint128::zero(), |acc, d| acc + d.shares)
            .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

        let calc_fee_collector_destination = Destination {
            recipient: Recipient::Bank {
                address: fee_collector.clone(),
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
                    owner: msg.owner.clone(),
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
                salt: to_json_binary(&(
                    msg.owner.to_string().truncate(16),
                    env.block.time.seconds(),
                    msg.distributor_code_id
                ))
                .unwrap(),
            })
        );
    }

    #[test]
    fn adds_execute_msg() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let msg = default_instantiate_msg();
        let manager = deps.api.addr_make("manager");
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

        let response = instantiate(
            deps.as_mut(),
            env.clone(),
            info,
            StrategyInstantiateMsg {
                fee_collector: Addr::unchecked("fee-collector"),
                config: CreateStrategyConfig::Twap(msg.clone()),
            },
        )
        .unwrap();

        assert_eq!(
            response.messages[1],
            SubMsg::new(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_json_binary(&StrategyExecuteMsg::Execute {}).unwrap(),
                funds: vec![]
            })
        );
    }

    #[test]
    fn adds_swap_conditions() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let msg = InstantiateTwapCommand {
            minimum_distribute_amount: Some(Coin::new(1000u128, "rune")),
            ..default_instantiate_msg()
        };
        let manager = deps.api.addr_make("manager");
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

        instantiate(
            deps.as_mut(),
            env.clone(),
            info,
            StrategyInstantiateMsg {
                fee_collector: Addr::unchecked("fee-collector"),
                config: CreateStrategyConfig::Twap(msg.clone()),
            },
        )
        .unwrap();

        let config = CONFIG.load(deps.as_ref().storage).unwrap();

        assert_eq!(
            config.swap_conditions,
            vec![
                match msg.swap_cadence {
                    Schedule::Blocks { interval, previous } => {
                        Condition::BlocksCompleted(
                            previous.unwrap_or(env.block.height - interval) + interval,
                        )
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
                    exchanger_contract: msg.exchanger_contract.clone(),
                    swap_amount: msg.swap_amount.clone(),
                    minimum_receive_amount: msg.minimum_receive_amount.clone(),
                    maximum_slippage_bps: msg.maximum_slippage_bps,
                    route: msg.route.clone(),
                },
            ]
        );
    }

    #[test]
    fn publishes_strategy_instantiated_event() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let msg = default_instantiate_msg();
        let manager = deps.api.addr_make("manager");
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

        let response = instantiate(
            deps.as_mut(),
            env.clone(),
            info,
            StrategyInstantiateMsg {
                fee_collector: Addr::unchecked("fee-collector"),
                config: CreateStrategyConfig::Twap(msg.clone()),
            },
        )
        .unwrap();

        let config = CONFIG.load(deps.as_ref().storage).unwrap();

        assert_eq!(
            response.events[0],
            Event::from(DomainEvent::TwapStrategyCreated {
                contract_address: env.contract.address,
                config
            })
        );
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;

    use calc_rs::core::Schedule;
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr, Coin, Timestamp,
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("not-owner"), &[]),
                StrategyExecuteMsg::Update(StrategyConfig::Twap(config.clone()))
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
                StrategyExecuteMsg::Update(StrategyConfig::Twap(config.clone()))
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let new_config = StrategyExecuteMsg::Update(StrategyConfig::Twap(TwapConfig {
            manager_contract: Addr::unchecked("new-manager"),
            ..config.clone()
        }));

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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let new_config = StrategyExecuteMsg::Update(StrategyConfig::Twap(TwapConfig {
            distributor_contract: Addr::unchecked("new-distributor"),
            ..config.clone()
        }));

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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let new_config = StrategyExecuteMsg::Update(StrategyConfig::Twap(TwapConfig {
            swap_amount: Coin::new(0u128, "new-denom"),
            ..config.clone()
        }));

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.owner, &[]),
                new_config
            )
            .unwrap_err(),
            ContractError::generic_err(format!(
                "Cannot change the swap amount denomination from {} to {}",
                config.swap_amount.denom, "new-denom"
            ))
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let new_config = StrategyExecuteMsg::Update(StrategyConfig::Twap(TwapConfig {
            minimum_receive_amount: Coin::new(0u128, "new-denom"),
            ..config.clone()
        }));

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.owner, &[]),
                new_config
            )
            .unwrap_err(),
            ContractError::generic_err(format!(
                "Cannot change the minimum receive amount denomination from {} to {}",
                config.minimum_receive_amount.denom, "new-denom"
            ))
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let new_config = StrategyExecuteMsg::Update(StrategyConfig::Twap(TwapConfig {
            maximum_slippage_bps: 10_001,
            ..config.clone()
        }));

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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
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
            StrategyExecuteMsg::Update(StrategyConfig::Twap(new_config.clone())),
        )
        .unwrap();

        assert_eq!(
            CONFIG.load(deps.as_ref().storage).unwrap(),
            TwapConfig {
                swap_conditions: vec![
                    match new_config.swap_cadence {
                        Schedule::Blocks { interval, previous } => Condition::BlocksCompleted(
                            previous.unwrap_or(env.block.height.saturating_sub(interval))
                                + interval,
                        ),
                        Schedule::Time { duration, previous } => Condition::TimestampElapsed(
                            previous
                                .unwrap_or(Timestamp::from_seconds(
                                    env.block.time.seconds().saturating_sub(duration.as_secs()),
                                ))
                                .plus_seconds(duration.as_secs()),
                        ),
                    },
                    Condition::BalanceAvailable {
                        address: env.contract.address.clone(),
                        amount: Coin::new(1u128, new_config.swap_amount.denom.clone()),
                    },
                    Condition::ExchangeLiquidityProvided {
                        exchanger_contract: config.exchanger_contract.clone(),
                        swap_amount: new_config.swap_amount.clone(),
                        minimum_receive_amount: new_config.minimum_receive_amount.clone(),
                        maximum_slippage_bps: new_config.maximum_slippage_bps,
                        route: new_config.route.clone(),
                    },
                ],
                schedule_conditions: vec![
                    Condition::BalanceAvailable {
                        address: env.contract.address.clone(),
                        amount: Coin::new(1u128, new_config.swap_amount.denom.clone()),
                    },
                    Condition::StrategyStatus {
                        manager_contract: config.manager_contract.clone(),
                        contract_address: env.contract.address.clone(),
                        status: StrategyStatus::Active
                    }
                ],
                ..new_config
            }
        );
    }
}

#[cfg(test)]
mod execute_tests {
    use super::*;

    use calc_rs::{core::Schedule, exchanger::ExpectedReceiveAmount, manager::Strategy};
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        to_json_binary, Addr, Coin, ContractResult, Event, SubMsg, SystemResult, WasmMsg,
        WasmQuery,
    };

    use crate::{
        contract::{default_config, execute, EXECUTE_REPLY_ID, SCHEDULE_REPLY_ID},
        state::{CONFIG, STATE, STATS},
        types::DomainEvent,
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&config.manager_contract, &[]),
            StrategyExecuteMsg::Execute {},
        )
        .unwrap();

        assert!(response.messages.contains(&SubMsg::new(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&StrategyExecuteMsg::Clear {}).unwrap(),
            funds: vec![]
        })));

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                StrategyExecuteMsg::Execute {}
            )
            .unwrap_err(),
            ContractError::generic_err("Contract is already in the requested state")
        );

        assert_eq!(
            STATE.load(deps.as_ref().storage).unwrap(),
            StrategyExecuteMsg::Execute {}
        );
    }

    #[test]
    fn clears_state() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        STATE
            .save(deps.as_mut().storage, &StrategyExecuteMsg::Execute {})
            .unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&env.contract.address, &[]),
            StrategyExecuteMsg::Clear {},
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("not-manager"), &[]),
                StrategyExecuteMsg::Execute {}
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
                StrategyExecuteMsg::Execute {}
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
                StrategyExecuteMsg::Execute {}
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
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
                StrategyExecuteMsg::Execute {}
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
                StrategyExecuteMsg::Execute {}
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
                StrategyExecuteMsg::Execute {}
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                StrategyExecuteMsg::Execute {}
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![config.swap_amount.clone()],
        );

        deps.querier.update_wasm(move |_| {
            let minimum_receive_amount = default_config().minimum_receive_amount;
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&ExpectedReceiveAmount {
                    receive_amount: Coin::new(
                        minimum_receive_amount.amount.u128() + 1,
                        minimum_receive_amount.denom.clone(),
                    ),
                    slippage_bps: config.maximum_slippage_bps - 1,
                })
                .unwrap(),
            ))
        });

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                StrategyExecuteMsg::Execute {}
            )
            .unwrap()
            .messages[0],
            SubMsg::reply_always(
                WasmMsg::Execute {
                    contract_addr: config.exchanger_contract.to_string(),
                    msg: to_json_binary(&ExchangeExecuteMsg::Swap {
                        minimum_receive_amount: config.minimum_receive_amount.clone(),
                        maximum_slippage_bps: config.maximum_slippage_bps,
                        route: config.route.clone(),
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
    fn skips_execution_if_all_conditions_not_met() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        deps.querier.update_wasm(|_| {
            let config = default_config();
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&ExpectedReceiveAmount {
                    receive_amount: config.minimum_receive_amount,
                    slippage_bps: config.maximum_slippage_bps + 1,
                })
                .unwrap(),
            ))
        });

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                StrategyExecuteMsg::Execute {}
            )
            .unwrap()
            .events[0],
            Event::from(DomainEvent::TwapExecutionSkipped {
                contract_address: env.contract.address.clone(),
                reason: format!(
                    "Execution skipped due to the following reasons:\n* {}",
                    vec![
                        Condition::BalanceAvailable {
                            address: env.contract.address.clone(),
                            amount: Coin::new(1u128, config.swap_amount.denom.clone())
                        }
                        .check(deps.as_ref(), &env)
                        .unwrap_err(),
                        Condition::ExchangeLiquidityProvided {
                            exchanger_contract: config.exchanger_contract,
                            swap_amount: config.swap_amount,
                            minimum_receive_amount: config.minimum_receive_amount,
                            maximum_slippage_bps: config.maximum_slippage_bps,
                            route: config.route.clone(),
                        }
                        .check(deps.as_ref(), &env)
                        .unwrap_err(),
                    ]
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("\n* ")
                )
            })
        );
    }

    #[test]
    fn skips_execution_if_any_condition_not_met() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        STATS
            .save(
                deps.as_mut().storage,
                &TwapStatistics {
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        deps.querier.update_wasm(|_| {
            let config = default_config();
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&ExpectedReceiveAmount {
                    receive_amount: config.minimum_receive_amount,
                    slippage_bps: config.maximum_slippage_bps + 1,
                })
                .unwrap(),
            ))
        });

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![config.swap_amount.clone()],
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                StrategyExecuteMsg::Execute {}
            )
            .unwrap()
            .events[0],
            Event::from(DomainEvent::TwapExecutionSkipped {
                contract_address: env.contract.address.clone(),
                reason: format!(
                    "Execution skipped due to the following reasons:\n* {}",
                    vec![Condition::ExchangeLiquidityProvided {
                        exchanger_contract: config.exchanger_contract,
                        swap_amount: config.swap_amount,
                        minimum_receive_amount: config.minimum_receive_amount,
                        maximum_slippage_bps: config.maximum_slippage_bps,
                        route: config.route.clone(),
                    }
                    .check(deps.as_ref(), &env)
                    .unwrap_err(),]
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("\n* ")
                )
            })
        );
    }

    #[test]
    fn schedules_next_execution_if_conditions_met() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let exchanger_contract = Addr::unchecked("exchanger");
        let manager_contract = Addr::unchecked("manager");

        let config = TwapConfig {
            exchanger_contract: exchanger_contract.clone(),
            manager_contract: manager_contract.clone(),
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![config.swap_amount.clone()],
        );

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { contract_addr, .. } => {
                    if contract_addr == &exchanger_contract.to_string() {
                        let minimum_receive_amount = default_config().minimum_receive_amount;
                        to_json_binary(&ExpectedReceiveAmount {
                            receive_amount: Coin::new(
                                minimum_receive_amount.amount.u128() + 1,
                                minimum_receive_amount.denom.clone(),
                            ),
                            slippage_bps: config.maximum_slippage_bps - 1,
                        })
                        .unwrap()
                    } else {
                        to_json_binary(&Strategy {
                            owner: Addr::unchecked("owner"),
                            contract_address: Addr::unchecked("contract"),
                            created_at: 0,
                            updated_at: 0,
                            label: "test".to_string(),
                            status: StrategyStatus::Active,
                            affiliates: vec![],
                        })
                        .unwrap()
                    }
                }
                _ => panic!("Unexpected query type"),
            }))
        });

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                StrategyExecuteMsg::Execute {}
            )
            .unwrap()
            .messages[1],
            SubMsg::reply_always(
                WasmMsg::Execute {
                    contract_addr: config.scheduler_contract.to_string(),
                    msg: to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
                        conditions: vec![config.swap_cadence.next(&env).into_condition(&env)],
                        threshold: TriggerConditionsThreshold::All,
                        to: config.manager_contract.clone(),
                        msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                            contract_address: env.contract.address.clone(),
                            msg: Some(to_json_binary(&StrategyExecuteMsg::Execute {}).unwrap()),
                        })
                        .unwrap(),
                    }]))
                    .unwrap(),
                    funds: config.execution_rebate.map_or(vec![], |c| vec![c])
                },
                SCHEDULE_REPLY_ID
            )
        );
    }

    #[test]
    fn progresses_swap_cadence_if_conditions_are_met() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let exchanger_contract = Addr::unchecked("exchanger");
        let manager_contract = Addr::unchecked("manager");

        let config = TwapConfig {
            exchanger_contract: exchanger_contract.clone(),
            manager_contract: manager_contract.clone(),
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![config.swap_amount.clone()],
        );

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { contract_addr, .. } => {
                    if contract_addr == &exchanger_contract.to_string() {
                        let minimum_receive_amount = default_config().minimum_receive_amount;
                        to_json_binary(&ExpectedReceiveAmount {
                            receive_amount: Coin::new(
                                minimum_receive_amount.amount.u128() + 1,
                                minimum_receive_amount.denom.clone(),
                            ),
                            slippage_bps: config.maximum_slippage_bps - 1,
                        })
                        .unwrap()
                    } else {
                        to_json_binary(&Strategy {
                            owner: Addr::unchecked("owner"),
                            contract_address: Addr::unchecked("contract"),
                            created_at: 0,
                            updated_at: 0,
                            label: "test".to_string(),
                            status: StrategyStatus::Active,
                            affiliates: vec![],
                        })
                        .unwrap()
                    }
                }
                _ => panic!("Unexpected query type"),
            }))
        });

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&config.manager_contract, &[]),
            StrategyExecuteMsg::Execute {},
        )
        .unwrap();

        assert_ne!(
            CONFIG.load(deps.as_ref().storage).unwrap().swap_cadence,
            config.swap_cadence
        );

        assert_eq!(
            CONFIG.load(deps.as_ref().storage).unwrap().swap_cadence,
            config.swap_cadence.next(&env)
        );
    }

    #[test]
    fn skips_scheduling_if_all_conditions_not_met() {
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&Strategy {
                    owner: Addr::unchecked("owner"),
                    contract_address: Addr::unchecked("contract"),
                    created_at: 0,
                    updated_at: 0,
                    label: "test".to_string(),
                    status: StrategyStatus::Paused,
                    affiliates: vec![],
                })
                .unwrap(),
            ))
        });

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                StrategyExecuteMsg::Execute {}
            )
            .unwrap()
            .events[1],
            Event::from(DomainEvent::TwapSchedulingSkipped {
                contract_address: env.contract.address.clone(),
                reason: format!(
                    "Scheduling skipped due to the following reasons:\n* {}",
                    vec![
                        Condition::BalanceAvailable {
                            address: env.contract.address.clone(),
                            amount: Coin::new(1u128, config.swap_amount.denom.clone())
                        }
                        .check(deps.as_ref(), &env)
                        .unwrap_err(),
                        Condition::StrategyStatus {
                            manager_contract: env.contract.address.clone(),
                            contract_address: env.contract.address.clone(),
                            status: StrategyStatus::Active
                        }
                        .check(deps.as_ref(), &env)
                        .unwrap_err(),
                    ]
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("\n* ")
                )
            })
        );
    }

    #[test]
    fn skips_scheduling_if_any_condition_not_met() {
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![config.swap_amount.clone()],
        );

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&Strategy {
                    owner: Addr::unchecked("owner"),
                    contract_address: Addr::unchecked("contract"),
                    created_at: 0,
                    updated_at: 0,
                    label: "test".to_string(),
                    status: StrategyStatus::Paused,
                    affiliates: vec![],
                })
                .unwrap(),
            ))
        });

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                StrategyExecuteMsg::Execute {}
            )
            .unwrap()
            .events[1],
            Event::from(DomainEvent::TwapSchedulingSkipped {
                contract_address: env.contract.address.clone(),
                reason: format!(
                    "Scheduling skipped due to the following reasons:\n* {}",
                    vec![Condition::StrategyStatus {
                        manager_contract: env.contract.address.clone(),
                        contract_address: env.contract.address.clone(),
                        status: StrategyStatus::Active
                    }
                    .check(deps.as_ref(), &env)
                    .unwrap_err(),]
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("\n* ")
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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![config.swap_amount.clone()],
        );

        deps.querier.update_wasm(move |_| {
            let minimum_receive_amount = default_config().minimum_receive_amount;
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&ExpectedReceiveAmount {
                    receive_amount: Coin::new(
                        minimum_receive_amount.amount.u128() + 1,
                        minimum_receive_amount.denom.clone(),
                    ),
                    slippage_bps: config.maximum_slippage_bps - 1,
                })
                .unwrap(),
            ))
        });

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&config.manager_contract, &[]),
            StrategyExecuteMsg::Execute {},
        )
        .unwrap();

        let stats = STATS.load(deps.as_ref().storage).unwrap();
        assert_eq!(stats.swapped, config.swap_amount);
    }
}

#[cfg(test)]
mod withdraw_tests {
    use super::*;

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
                    swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&config.manager_contract, &[]),
                StrategyExecuteMsg::Withdraw { amounts: vec![] }
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
                StrategyExecuteMsg::Withdraw { amounts: vec![] }
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
                StrategyExecuteMsg::Withdraw { amounts: vec![] }
            )
            .is_ok(),
            true
        );
    }
}
