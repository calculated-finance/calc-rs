use std::cmp::min;

use calc_rs::{
    core::{Condition, Contract, ContractError, ContractResult},
    distributor::{Destination, DistributorConfig, DistributorExecuteMsg, Recipient},
    manager::{
        Affiliate, CreateStrategyConfig, ManagerQueryMsg, StrategyConfig, StrategyExecuteMsg,
        StrategyInstantiateMsg, StrategyQueryMsg, StrategyStatus,
    },
    scheduler::SchedulerExecuteMsg,
    stoploss::{StopLossConfig, StopLossStatistics},
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    instantiate2_address, to_json_binary, BankMsg, Binary, Coin, Coins, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Reply, Response, StdResult, Uint128, WasmMsg,
};
use rujira_rs::{
    fin::{
        BookResponse, ConfigResponse, ExecuteMsg, OrdersResponse, Price, QueryMsg, Side,
        SwapRequest,
    },
    CallbackData,
};

use crate::{
    state::{CONFIG, STATE, STATS},
    types::DomainEvent,
};

const BASE_FEE_BPS: u64 = 15;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    match msg.config {
        CreateStrategyConfig::StopLoss(config) => {
            if info.funds.len() > 1
                || (info.funds.len() == 1 && info.funds[0].denom != config.swap_denom)
            {
                return Err(ContractError::generic_err(format!(
                    "Invalid funds provided, only {} is allowed",
                    config.swap_denom
                )));
            }

            let pair = deps
                .querier
                .query_wasm_smart::<ConfigResponse>(
                    config.pair_address.clone(),
                    &QueryMsg::Config {},
                )
                .map_err(|e| {
                    ContractError::generic_err(format!("Failed to query pair config: {}", e))
                })?;

            let denoms = [
                pair.denoms.base().to_string(),
                pair.denoms.quote().to_string(),
            ];

            if !denoms.contains(&config.swap_denom) {
                return Err(ContractError::generic_err(format!(
                    "Pair at {} does not support swapping from {}",
                    config.pair_address, config.swap_denom
                )));
            }

            if !denoms.contains(&config.target_denom) {
                return Err(ContractError::generic_err(format!(
                    "Pair at {} does not support swapping into {}",
                    config.pair_address, config.target_denom
                )));
            }

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
                    denoms: vec![config.target_denom.clone()],
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
                &StopLossConfig {
                    owner: config.owner,
                    manager_contract: info.sender,
                    scheduler_contract: config.scheduler_contract,
                    distributor_contract: distributor_address.clone(),
                    move_conditions: vec![],
                    distribute_conditions: vec![],
                    execution_rebate: config.execution_rebate,
                    pair_address: config.pair_address,
                    swap_denom: config.swap_denom.clone(),
                    target_denom: config.target_denom.clone(),
                    offset: config.offset,
                },
            )?;

            STATS.save(
                deps.storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, config.swap_denom),
                    withdrawn: vec![],
                    filled: Coin::new(0u128, config.target_denom.clone()),
                    claimed: Coin::new(0u128, config.target_denom),
                },
            )?;

            let execute_msg = Contract(env.contract.address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::Execute { msg: None })?,
                vec![],
            );

            let strategy_instantiated_event = DomainEvent::StrategyCreated {
                contract_address: env.contract.address,
                config: CONFIG.load(deps.storage)?,
            };

            Ok(Response::default()
                .add_message(instantiate_distributor_msg)
                .add_message(execute_msg)
                .add_event(strategy_instantiated_event))
        }
        _ => {
            return Err(ContractError::generic_err(
                "Trying to instantiate a non-TWAP strategy with a TWAP contract implementation",
            ));
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

    if info.funds.len() > 1 || (info.funds.len() == 1 && info.funds[0].denom != config.swap_denom) {
        return Err(ContractError::generic_err(format!(
            "Invalid funds provided, only {} is allowed",
            config.swap_denom
        )));
    }

    let mut stats = STATS.load(deps.storage)?;

    let mut messages: Vec<CosmosMsg> = vec![];
    let mut events: Vec<DomainEvent> = vec![];

    match msg.clone() {
        StrategyExecuteMsg::Update(new_config) => {
            if info.sender != config.manager_contract {
                return Err(ContractError::Unauthorized {});
            }

            match new_config {
                StrategyConfig::StopLoss(new_config) => {
                    if new_config.owner != config.owner {
                        return Err(ContractError::generic_err("Cannot change the owner"));
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

                    if new_config.pair_address != config.pair_address {
                        return Err(ContractError::generic_err("Cannot change the pair address"));
                    }

                    if new_config.swap_denom != config.swap_denom {
                        return Err(ContractError::generic_err(format!(
                            "Cannot change the swap denomination from {} to {}",
                            config.target_denom, new_config.target_denom
                        )));
                    }

                    if new_config.target_denom != config.target_denom {
                        return Err(ContractError::generic_err(format!(
                            "Cannot change the target denomination from {} to {}",
                            config.target_denom, new_config.target_denom
                        )));
                    }

                    let updated_event = DomainEvent::StrategyUpdated {
                        contract_address: env.contract.address.clone(),
                        old_config: config.clone(),
                        new_config: new_config.clone(),
                    };

                    events.push(updated_event);

                    config = new_config;
                }
                _ => {
                    return Err(ContractError::generic_err(
                        "Trying to update a StopLoss strategy with non-StopLoss config",
                    ));
                }
            }
        }
        StrategyExecuteMsg::Execute { .. } => {
            if info.sender != config.manager_contract && info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            let orders_response = deps.querier.query_wasm_smart::<OrdersResponse>(
                config.pair_address.clone(),
                &QueryMsg::Orders {
                    owner: env.contract.address.to_string(),
                    side: None,
                    offset: None,
                    limit: None,
                },
            )?;

            if orders_response.orders.len() > 0 {
                let mut order_targets: Vec<(Side, Price, Option<Uint128>)> = vec![];
                let mut claimed_amount = Uint128::zero();
                let mut remaining_amount = Uint128::zero();

                for order in orders_response.orders {
                    let target = (order.side, order.price, Some(Uint128::zero()));
                    order_targets.push(target);

                    claimed_amount = claimed_amount + order.filled;
                    remaining_amount = remaining_amount + order.remaining;
                }

                let claim_and_withdraw_orders_msg = Contract(config.pair_address.clone()).call(
                    to_json_binary(&ExecuteMsg::Order((order_targets, None)))?,
                    vec![Coin::new(0u128, config.swap_denom.clone())],
                );

                messages.push(claim_and_withdraw_orders_msg);

                let pair = deps.querier.query_wasm_smart::<ConfigResponse>(
                    config.pair_address.clone(),
                    &QueryMsg::Config {},
                )?;

                let order_book = deps.querier.query_wasm_smart::<BookResponse>(
                    config.pair_address.clone(),
                    &QueryMsg::Book {
                        offset: None,
                        limit: None,
                    },
                )?;

                let mut target_amount = Uint128::new(1000);
                let mut swap_amount = Uint128::zero();

                if pair
                    .denoms
                    .ask_side(&Coin::new(1u128, config.swap_denom.clone()))?
                    == Side::Base
                {
                    for order in order_book.base {
                        let order_to_fill = min(target_amount, order.total);
                        let swap_amount_to_fill = order_to_fill.mul_ceil(order.price);
                        swap_amount = swap_amount + swap_amount_to_fill;
                        target_amount = target_amount.saturating_sub(order_to_fill);

                        if target_amount.is_zero() {
                            break;
                        }
                    }
                } else {
                    for order in order_book.quote {
                        let order_to_fill = min(target_amount, order.total);
                        let swap_amount_to_fill =
                            order_to_fill.mul_ceil(Decimal::one() / order.price);
                        swap_amount = swap_amount + swap_amount_to_fill;
                        target_amount = target_amount.saturating_sub(order_to_fill);

                        if target_amount.is_zero() {
                            break;
                        }
                    }
                };

                if remaining_amount > swap_amount && target_amount.eq(&Uint128::zero()) {
                    let swap_msg = Contract(config.pair_address.clone()).call(
                        to_json_binary(&ExecuteMsg::Swap(SwapRequest {
                            min_return: Some(target_amount),
                            to: None,
                            callback: None,
                        }))?,
                        vec![Coin::new(swap_amount, config.swap_denom)],
                    );

                    messages.push(swap_msg);
                }
            } else {
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

            let funds_withdrawn_event = DomainEvent::FundsWithdrawn {
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
                    let execute_msg = Contract(env.contract.address.clone()).call(
                        to_json_binary(&StrategyExecuteMsg::Execute { msg: None })?,
                        vec![],
                    );

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
            if info.sender != env.contract.address && info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            STATE.remove(deps.storage);
        }
    };

    CONFIG.save(deps.storage, &config)?;
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
        .add_messages(messages)
        .add_events(events))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, _reply: Reply) -> ContractResult {
    Ok(Response::default())
    // let mut events: Vec<DomainEvent> = vec![];

    // match reply.id {
    //     EXECUTE_REPLY_ID => match reply.result {
    //         SubMsgResult::Ok(_) => {
    //             let execution_succeeded_event = DomainEvent::TwapExecutionSucceeded {
    //                 contract_address: env.contract.address.clone(),
    //                 statistics: STATS.load(_deps.storage)?,
    //             };

    //             events.push(execution_succeeded_event);
    //         }
    //         SubMsgResult::Err(err) => {
    //             let execution_failed_event = DomainEvent::TwapExecutionFailed {
    //                 contract_address: env.contract.address.clone(),
    //                 reason: err.to_string(),
    //             };

    //             events.push(execution_failed_event);
    //         }
    //     },
    //     SCHEDULE_REPLY_ID => match reply.result {
    //         SubMsgResult::Ok(_) => {
    //             let scheduling_succeeded_event = DomainEvent::TwapSchedulingSucceeded {
    //                 contract_address: env.contract.address.clone(),
    //             };

    //             events.push(scheduling_succeeded_event);
    //         }
    //         SubMsgResult::Err(err) => {
    //             let scheduling_failed_event = DomainEvent::TwapSchedulingFailed {
    //                 contract_address: env.contract.address.clone(),
    //                 reason: err.to_string(),
    //             };

    //             events.push(scheduling_failed_event);
    //         }
    //     },
    //     _ => {}
    // }

    // Ok(Response::default().add_events(events))
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: StrategyQueryMsg) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;
    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&config),
        StrategyQueryMsg::Statistics {} => unimplemented!(),
    }
}

#[cfg(test)]
mod instantiate_tests {
    use calc_rs::{
        core::ContractError,
        distributor::{Destination, DistributorConfig, Recipient},
        manager::{Affiliate, CreateStrategyConfig, StrategyExecuteMsg, StrategyInstantiateMsg},
        stoploss::{InstantiateStopLossCommand, StopLossConfig},
    };
    use calc_rs_test::test::CodeInfoResponse;
    use cosmwasm_std::{
        instantiate2_address,
        testing::{message_info, mock_dependencies, mock_env},
        to_json_binary, Addr, Api, Checksum, Coin, ContractResult, Decimal, Event, SubMsg,
        SystemResult, Uint128, WasmMsg, WasmQuery,
    };
    use rujira_rs::fin::{ConfigResponse, Denoms, Tick};

    use crate::{
        contract::{instantiate, BASE_FEE_BPS},
        state::CONFIG,
        types::DomainEvent,
    };

    fn default_instantiate_msg() -> InstantiateStopLossCommand {
        InstantiateStopLossCommand {
            owner: Addr::unchecked("owner"),
            manager_contract: Addr::unchecked("manager"),
            scheduler_contract: Addr::unchecked("scheduler"),
            distributor_code_id: 1,
            pair_address: Addr::unchecked("pair_address"),
            swap_denom: "swap_denom".to_string(),
            target_denom: "target_denom".to_string(),
            offset: Decimal::percent(10),
            execution_rebate: Some(Coin::new(100u128, "rune")),
            affiliate_code: None,
            minimum_distribute_amount: None,
            mutable_destinations: vec![Destination {
                recipient: Recipient::Bank {
                    address: Addr::unchecked("mutable_destination"),
                },
                shares: Uint128::new(10_000u128),
                label: Some("Mutable Destination".to_string()),
            }],
            immutable_destinations: vec![],
        }
    }

    #[test]
    fn fails_if_non_swap_denom_funds_provided() {
        let mut deps = mock_dependencies();

        let response = instantiate(
            deps.as_mut(),
            mock_env(),
            message_info(
                &Addr::unchecked("manager"),
                &[Coin::new(100u128, "not_swap")],
            ),
            StrategyInstantiateMsg {
                fee_collector: Addr::unchecked("fee_collector"),
                config: CreateStrategyConfig::StopLoss(default_instantiate_msg()),
            },
        )
        .unwrap_err();

        assert_eq!(
            response,
            ContractError::generic_err("Invalid funds provided, only swap_denom is allowed")
        );
    }

    #[test]
    fn fails_if_multiple_funds_provided() {
        let mut deps = mock_dependencies();

        let response = instantiate(
            deps.as_mut(),
            mock_env(),
            message_info(
                &Addr::unchecked("manager"),
                &[Coin::new(100u128, "swap_denom"), Coin::new(50u128, "rune")],
            ),
            StrategyInstantiateMsg {
                fee_collector: Addr::unchecked("fee_collector"),
                config: CreateStrategyConfig::StopLoss(default_instantiate_msg()),
            },
        )
        .unwrap_err();

        assert_eq!(
            response,
            ContractError::generic_err("Invalid funds provided, only swap_denom is allowed")
        );
    }

    #[test]
    fn fails_if_pair_denoms_do_not_contain_swap_and_target_denom() {
        let mut deps = mock_dependencies();
        let config = default_instantiate_msg();

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&ConfigResponse {
                    denoms: Denoms::new("not_swap_denom", "target_denom"),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(6),
                    fee_taker: Decimal::percent(10),
                    fee_maker: Decimal::percent(10),
                    fee_address: "fee_address".to_string(),
                })
                .unwrap(),
            ))
        });

        let response = instantiate(
            deps.as_mut(),
            mock_env(),
            message_info(
                &Addr::unchecked("manager"),
                &[Coin::new(100u128, "swap_denom")],
            ),
            StrategyInstantiateMsg {
                fee_collector: Addr::unchecked("fee_collector"),
                config: CreateStrategyConfig::StopLoss(config.clone()),
            },
        )
        .unwrap_err();

        assert_eq!(
            response,
            ContractError::generic_err(format!(
                "Pair at {} does not support swapping from {}",
                config.pair_address, config.swap_denom
            ))
        );

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&ConfigResponse {
                    denoms: Denoms::new("swap_denom", "not_target_denom"),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(6),
                    fee_taker: Decimal::percent(10),
                    fee_maker: Decimal::percent(10),
                    fee_address: "fee_address".to_string(),
                })
                .unwrap(),
            ))
        });

        let response = instantiate(
            deps.as_mut(),
            mock_env(),
            message_info(
                &Addr::unchecked("manager"),
                &[Coin::new(100u128, "swap_denom")],
            ),
            StrategyInstantiateMsg {
                fee_collector: Addr::unchecked("fee_collector"),
                config: CreateStrategyConfig::StopLoss(config.clone()),
            },
        )
        .unwrap_err();

        assert_eq!(
            response,
            ContractError::generic_err(format!(
                "Pair at {} does not support swapping into {}",
                config.pair_address, config.target_denom
            ))
        );
    }

    #[test]
    fn fails_if_pair_address_not_found() {
        let mut deps = mock_dependencies();

        let response = instantiate(
            deps.as_mut(),
            mock_env(),
            message_info(
                &Addr::unchecked("manager"),
                &[Coin::new(100u128, "swap_denom")],
            ),
            StrategyInstantiateMsg {
                fee_collector: Addr::unchecked("fee_collector"),
                config: CreateStrategyConfig::StopLoss(default_instantiate_msg()),
            },
        )
        .unwrap_err();

        assert!(response.to_string().contains("Failed to query pair config"),);
    }

    #[test]
    fn saves_valid_config() {
        let mut deps = mock_dependencies();
        let config = default_instantiate_msg();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { .. } => to_json_binary(&ConfigResponse {
                    denoms: Denoms::new("swap_denom", "target_denom"),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(6),
                    fee_taker: Decimal::percent(10),
                    fee_maker: Decimal::percent(10),
                    fee_address: "fee_address".to_string(),
                })
                .unwrap(),
                WasmQuery::CodeInfo { .. } => to_json_binary(&CodeInfoResponse {
                    code_id: 1,
                    creator: Addr::unchecked("creator"),
                    checksum: Checksum::from_hex(
                        "9c28fd8b95ead3f54024f04c742b506c9fa0d24024defe47dcf42601710aca08",
                    )
                    .unwrap(),
                })
                .unwrap(),
                _ => panic!("Unexpected query: {:?}", query),
            }))
        });

        let fee_collector = Addr::unchecked("fee_collector");
        let env = mock_env();

        instantiate(
            deps.as_mut(),
            env.clone(),
            message_info(
                &Addr::unchecked("manager"),
                &[Coin::new(100u128, "swap_denom")],
            ),
            StrategyInstantiateMsg {
                fee_collector: fee_collector.clone(),
                config: CreateStrategyConfig::StopLoss(config.clone()),
            },
        )
        .unwrap();

        let distributor_address = instantiate2_address(
            Checksum::from_hex("9c28fd8b95ead3f54024f04c742b506c9fa0d24024defe47dcf42601710aca08")
                .unwrap()
                .as_slice(),
            &deps
                .api
                .addr_canonicalize(env.contract.address.as_str())
                .unwrap(),
            to_json_binary(&(
                config.owner.to_string().truncate(16),
                env.block.time.seconds(),
                1,
            ))
            .unwrap()
            .as_slice(),
        )
        .unwrap();

        assert_eq!(
            CONFIG.load(deps.as_ref().storage).unwrap(),
            StopLossConfig {
                owner: config.owner,
                manager_contract: config.manager_contract,
                scheduler_contract: config.scheduler_contract,
                distributor_contract: deps.api.addr_humanize(&distributor_address).unwrap(),
                pair_address: config.pair_address,
                swap_denom: config.swap_denom,
                target_denom: config.target_denom,
                offset: config.offset,
                move_conditions: vec![],
                distribute_conditions: vec![],
                execution_rebate: config.execution_rebate
            }
        );
    }

    #[test]
    fn sends_distributor_instantiate_msg() {
        let mut deps = mock_dependencies();
        let config = default_instantiate_msg();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { .. } => to_json_binary(&ConfigResponse {
                    denoms: Denoms::new("swap_denom", "target_denom"),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(6),
                    fee_taker: Decimal::percent(10),
                    fee_maker: Decimal::percent(10),
                    fee_address: "fee_address".to_string(),
                })
                .unwrap(),
                WasmQuery::CodeInfo { .. } => to_json_binary(&CodeInfoResponse {
                    code_id: 1,
                    creator: Addr::unchecked("creator"),
                    checksum: Checksum::from_hex(
                        "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
                    )
                    .unwrap(),
                })
                .unwrap(),
                _ => panic!("Unexpected query: {:?}", query),
            }))
        });

        let fee_collector = Addr::unchecked("fee_collector");
        let env = mock_env();

        let response = instantiate(
            deps.as_mut(),
            env.clone(),
            message_info(
                &Addr::unchecked("manager"),
                &[Coin::new(100u128, "swap_denom")],
            ),
            StrategyInstantiateMsg {
                fee_collector: fee_collector.clone(),
                config: CreateStrategyConfig::StopLoss(config.clone()),
            },
        )
        .unwrap();

        let destinations = config
            .mutable_destinations
            .iter()
            .chain(config.immutable_destinations.iter())
            .collect::<Vec<_>>();

        let total_shares_with_fees = destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares)
            .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

        assert_eq!(
            response.messages[0],
            SubMsg::new(WasmMsg::Instantiate2 {
                admin: Some(env.contract.address.to_string()),
                code_id: 1,
                label: "Distributor".to_string(),
                msg: to_json_binary(&DistributorConfig {
                    owner: config.owner.clone(),
                    denoms: vec![config.target_denom.clone()],
                    mutable_destinations: config.mutable_destinations,
                    immutable_destinations: [
                        config.immutable_destinations,
                        vec![Destination {
                            recipient: Recipient::Bank {
                                address: fee_collector.clone(),
                            },
                            shares: total_shares_with_fees.mul_floor(Decimal::bps(BASE_FEE_BPS)),
                            label: Some("CALC".to_string()),
                        }]
                    ]
                    .concat(),
                    conditions: vec![],
                })
                .unwrap(),
                funds: vec![],
                salt: to_json_binary(&(
                    "owner".to_string().truncate(16),
                    env.block.time.seconds(),
                    1
                ))
                .unwrap(),
            })
        );
    }

    #[test]
    fn sends_distributor_instantiate_msg_with_affiliate_destination() {
        let mut deps = mock_dependencies();
        let config = InstantiateStopLossCommand {
            affiliate_code: Some("affiliate_code".to_string()),
            ..default_instantiate_msg()
        };

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { contract_addr, .. } => {
                    if contract_addr == "manager" {
                        to_json_binary(&Affiliate {
                            code: "affiliate_code".to_string(),
                            address: Addr::unchecked("affiliate_address"),
                            bps: 5,
                        })
                        .unwrap()
                    } else {
                        to_json_binary(&ConfigResponse {
                            denoms: Denoms::new("swap_denom", "target_denom"),
                            oracles: None,
                            market_maker: None,
                            tick: Tick::new(6),
                            fee_taker: Decimal::percent(10),
                            fee_maker: Decimal::percent(10),
                            fee_address: "fee_address".to_string(),
                        })
                        .unwrap()
                    }
                }
                WasmQuery::CodeInfo { .. } => to_json_binary(&CodeInfoResponse {
                    code_id: 1,
                    creator: Addr::unchecked("creator"),
                    checksum: Checksum::from_hex(
                        "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
                    )
                    .unwrap(),
                })
                .unwrap(),
                _ => panic!("Unexpected query: {:?}", query),
            }))
        });

        let fee_collector = Addr::unchecked("fee_collector");
        let env = mock_env();

        let response = instantiate(
            deps.as_mut(),
            env.clone(),
            message_info(
                &Addr::unchecked("manager"),
                &[Coin::new(100u128, "swap_denom")],
            ),
            StrategyInstantiateMsg {
                fee_collector: fee_collector.clone(),
                config: CreateStrategyConfig::StopLoss(config.clone()),
            },
        )
        .unwrap();

        let destinations = config
            .mutable_destinations
            .iter()
            .chain(config.immutable_destinations.iter())
            .collect::<Vec<_>>();

        let total_shares_with_fees = destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares)
            .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

        assert_eq!(
            response.messages[0],
            SubMsg::new(WasmMsg::Instantiate2 {
                admin: Some(env.contract.address.to_string()),
                code_id: 1,
                label: "Distributor".to_string(),
                msg: to_json_binary(&DistributorConfig {
                    owner: config.owner.clone(),
                    denoms: vec![config.target_denom.clone()],
                    mutable_destinations: config.mutable_destinations,
                    immutable_destinations: [
                        config.immutable_destinations,
                        vec![
                            Destination {
                                recipient: Recipient::Bank {
                                    address: fee_collector.clone(),
                                },
                                shares: total_shares_with_fees
                                    .mul_floor(Decimal::bps(BASE_FEE_BPS - 5)),
                                label: Some("CALC".to_string()),
                            },
                            Destination {
                                recipient: Recipient::Bank {
                                    address: Addr::unchecked("affiliate_address"),
                                },
                                shares: total_shares_with_fees.mul_floor(Decimal::bps(5)),
                                label: Some("Affiliate: affiliate_code".to_string()),
                            }
                        ],
                    ]
                    .concat(),
                    conditions: vec![],
                })
                .unwrap(),
                funds: vec![],
                salt: to_json_binary(&(
                    "owner".to_string().truncate(16),
                    env.block.time.seconds(),
                    1
                ))
                .unwrap(),
            })
        );
    }

    #[test]
    fn sends_execute_msg_to_self() {
        let mut deps = mock_dependencies();
        let config = default_instantiate_msg();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { .. } => to_json_binary(&ConfigResponse {
                    denoms: Denoms::new("swap_denom", "target_denom"),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(6),
                    fee_taker: Decimal::percent(10),
                    fee_maker: Decimal::percent(10),
                    fee_address: "fee_address".to_string(),
                })
                .unwrap(),
                WasmQuery::CodeInfo { .. } => to_json_binary(&CodeInfoResponse {
                    code_id: 1,
                    creator: Addr::unchecked("creator"),
                    checksum: Checksum::from_hex(
                        "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
                    )
                    .unwrap(),
                })
                .unwrap(),
                _ => panic!("Unexpected query: {:?}", query),
            }))
        });

        let fee_collector = Addr::unchecked("fee_collector");
        let env = mock_env();

        let response = instantiate(
            deps.as_mut(),
            env.clone(),
            message_info(
                &Addr::unchecked("manager"),
                &[Coin::new(100u128, "swap_denom")],
            ),
            StrategyInstantiateMsg {
                fee_collector: fee_collector.clone(),
                config: CreateStrategyConfig::StopLoss(config.clone()),
            },
        )
        .unwrap();

        assert_eq!(
            response.messages[1],
            SubMsg::new(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_json_binary(&StrategyExecuteMsg::Execute { msg: None }).unwrap(),
                funds: vec![],
            })
        );
    }

    #[test]
    fn publishes_strategy_instantiated_event() {
        let mut deps = mock_dependencies();
        let config = default_instantiate_msg();

        deps.querier.update_wasm(|query| {
            SystemResult::Ok(ContractResult::Ok(match query {
                WasmQuery::Smart { .. } => to_json_binary(&ConfigResponse {
                    denoms: Denoms::new("swap_denom", "target_denom"),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(6),
                    fee_taker: Decimal::percent(10),
                    fee_maker: Decimal::percent(10),
                    fee_address: "fee_address".to_string(),
                })
                .unwrap(),
                WasmQuery::CodeInfo { .. } => to_json_binary(&CodeInfoResponse {
                    code_id: 1,
                    creator: Addr::unchecked("creator"),
                    checksum: Checksum::from_hex(
                        "9c28fd8b95ead3f54024f04c742b506c9fa0d24024defe47dcf42601710aca08",
                    )
                    .unwrap(),
                })
                .unwrap(),
                _ => panic!("Unexpected query: {:?}", query),
            }))
        });

        let fee_collector = Addr::unchecked("fee_collector");
        let env = mock_env();

        let response = instantiate(
            deps.as_mut(),
            env.clone(),
            message_info(
                &Addr::unchecked("manager"),
                &[Coin::new(100u128, "swap_denom")],
            ),
            StrategyInstantiateMsg {
                fee_collector: fee_collector.clone(),
                config: CreateStrategyConfig::StopLoss(config.clone()),
            },
        )
        .unwrap();

        let distributor_address = instantiate2_address(
            Checksum::from_hex("9c28fd8b95ead3f54024f04c742b506c9fa0d24024defe47dcf42601710aca08")
                .unwrap()
                .as_slice(),
            &deps
                .api
                .addr_canonicalize(env.contract.address.as_str())
                .unwrap(),
            to_json_binary(&(
                config.owner.to_string().truncate(16),
                env.block.time.seconds(),
                1,
            ))
            .unwrap()
            .as_slice(),
        )
        .unwrap();

        assert_eq!(
            response.events[0],
            Event::from(DomainEvent::StrategyCreated {
                contract_address: env.contract.address,
                config: StopLossConfig {
                    owner: config.owner,
                    manager_contract: config.manager_contract,
                    scheduler_contract: config.scheduler_contract,
                    distributor_contract: deps.api.addr_humanize(&distributor_address).unwrap(),
                    pair_address: config.pair_address,
                    swap_denom: config.swap_denom,
                    target_denom: config.target_denom,
                    offset: config.offset,
                    move_conditions: vec![],
                    distribute_conditions: vec![],
                    execution_rebate: config.execution_rebate,
                }
            })
        );
    }
}

#[cfg(test)]
fn default_config() -> StopLossConfig {
    use cosmwasm_std::Addr;
    StopLossConfig {
        owner: Addr::unchecked("owner"),
        manager_contract: Addr::unchecked("manager"),
        scheduler_contract: Addr::unchecked("scheduler"),
        distributor_contract: Addr::unchecked("distributor"),
        pair_address: Addr::unchecked("pair_address"),
        swap_denom: "swap_denom".to_string(),
        target_denom: "target_denom".to_string(),
        offset: Decimal::percent(10),
        move_conditions: vec![],
        distribute_conditions: vec![],
        execution_rebate: Some(Coin::new(100u128, "rune")),
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;

    use calc_rs::{
        core::ContractError,
        manager::{StrategyConfig, StrategyExecuteMsg},
        stoploss::{StopLossConfig, StopLossStatistics},
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr, Coin, Decimal, Event,
    };

    use crate::{
        contract::execute,
        state::{CONFIG, STATS},
        types::DomainEvent,
    };

    #[test]
    fn fails_if_sender_not_manager() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = message_info(&deps.api.addr_make("owner"), &[]);

        CONFIG
            .save(deps.as_mut().storage, &default_config())
            .unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(0u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env,
                info,
                StrategyExecuteMsg::Update(StrategyConfig::StopLoss(StopLossConfig {
                    owner: Addr::unchecked("new-owner"),
                    ..default_config()
                })),
            )
            .unwrap_err(),
            ContractError::Unauthorized {}
        );
    }

    #[test]
    fn fails_to_update_owner() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let existing_config = default_config();
        let info = message_info(&existing_config.manager_contract, &[]);

        CONFIG
            .save(deps.as_mut().storage, &existing_config)
            .unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(0u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env,
                info,
                StrategyExecuteMsg::Update(StrategyConfig::StopLoss(StopLossConfig {
                    owner: Addr::unchecked("new-owner"),
                    ..existing_config
                })),
            )
            .unwrap_err(),
            ContractError::generic_err("Cannot change the owner")
        );
    }

    #[test]
    fn fails_to_update_manager_contract() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let existing_config = default_config();
        let info = message_info(&existing_config.manager_contract, &[]);

        CONFIG
            .save(deps.as_mut().storage, &existing_config)
            .unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(0u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env,
                info,
                StrategyExecuteMsg::Update(StrategyConfig::StopLoss(StopLossConfig {
                    manager_contract: Addr::unchecked("new-manager"),
                    ..existing_config
                })),
            )
            .unwrap_err(),
            ContractError::generic_err("Cannot change the manager contract")
        );
    }

    #[test]
    fn fails_to_update_distributor_contract() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let existing_config = default_config();
        let info = message_info(&existing_config.manager_contract, &[]);

        CONFIG
            .save(deps.as_mut().storage, &existing_config)
            .unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(0u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env,
                info,
                StrategyExecuteMsg::Update(StrategyConfig::StopLoss(StopLossConfig {
                    distributor_contract: Addr::unchecked("new-distributor"),
                    ..existing_config
                })),
            )
            .unwrap_err(),
            ContractError::generic_err("Cannot change the distributor contract")
        );
    }

    #[test]
    fn fails_to_update_pair_address() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let existing_config = default_config();
        let info = message_info(&existing_config.manager_contract, &[]);

        CONFIG
            .save(deps.as_mut().storage, &existing_config)
            .unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(0u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env,
                info,
                StrategyExecuteMsg::Update(StrategyConfig::StopLoss(StopLossConfig {
                    pair_address: Addr::unchecked("new-pair-address"),
                    ..existing_config
                })),
            )
            .unwrap_err(),
            ContractError::generic_err("Cannot change the pair address")
        );
    }

    #[test]
    fn fails_to_update_swap_denom() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let existing_config = default_config();
        let info = message_info(&existing_config.manager_contract, &[]);

        CONFIG
            .save(deps.as_mut().storage, &existing_config)
            .unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(0u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env,
                info,
                StrategyExecuteMsg::Update(StrategyConfig::StopLoss(StopLossConfig {
                    swap_denom: "new-swap-denom".to_string(),
                    ..existing_config
                })),
            )
            .unwrap_err(),
            ContractError::generic_err("Cannot change the swap denom")
        );
    }

    #[test]
    fn fails_to_update_target_denom() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let existing_config = default_config();
        let info = message_info(&existing_config.manager_contract, &[]);

        CONFIG
            .save(deps.as_mut().storage, &existing_config)
            .unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(0u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env,
                info,
                StrategyExecuteMsg::Update(StrategyConfig::StopLoss(StopLossConfig {
                    target_denom: "new-target-denom".to_string(),
                    ..existing_config
                })),
            )
            .unwrap_err(),
            ContractError::generic_err("Cannot change the target denom")
        );
    }

    #[test]
    fn updates_mutable_config() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let existing_config = default_config();
        let info = message_info(&existing_config.manager_contract, &[]);

        CONFIG
            .save(deps.as_mut().storage, &existing_config)
            .unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(0u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let new_config = StopLossConfig {
            offset: Decimal::percent(20),
            execution_rebate: Some(Coin::new(2u128, "rune")),
            ..existing_config
        };

        execute(
            deps.as_mut(),
            env,
            info,
            StrategyExecuteMsg::Update(StrategyConfig::StopLoss(new_config.clone())),
        )
        .unwrap();

        assert_eq!(CONFIG.load(deps.as_ref().storage).unwrap(), new_config);
    }

    #[test]
    fn publishes_strategy_updated_event() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let existing_config = default_config();
        let info = message_info(&existing_config.manager_contract, &[]);

        CONFIG
            .save(deps.as_mut().storage, &existing_config)
            .unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(0u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        let new_config = StopLossConfig {
            offset: Decimal::percent(20),
            execution_rebate: Some(Coin::new(2u128, "rune")),
            ..existing_config.clone()
        };

        let response = execute(
            deps.as_mut(),
            env.clone(),
            info,
            StrategyExecuteMsg::Update(StrategyConfig::StopLoss(new_config.clone())),
        )
        .unwrap();

        assert_eq!(
            response.events[0],
            Event::from(DomainEvent::StrategyUpdated {
                contract_address: env.contract.address,
                old_config: existing_config,
                new_config: new_config,
            })
        );
    }
}

#[cfg(test)]
mod execute_tests {
    use super::*;
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr,
    };

    use crate::state::CONFIG;

    #[test]
    fn fails_if_sender_not_manager_or_contract() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(0u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("anyone"), &[]),
                StrategyExecuteMsg::Execute { msg: None },
            )
            .unwrap_err(),
            ContractError::Unauthorized {}
        );

        STATE.remove(deps.as_mut().storage);

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&config.manager_contract, &[]),
            StrategyExecuteMsg::Execute { msg: None },
        )
        .is_ok());

        STATE.remove(deps.as_mut().storage);

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&env.contract.address, &[]),
            StrategyExecuteMsg::Execute { msg: None },
        )
        .is_ok());
    }

    #[test]
    fn fails_if_already_in_executing_state() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(0u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&config.manager_contract, &[]),
            StrategyExecuteMsg::Execute { msg: None },
        )
        .is_ok());

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&env.contract.address, &[]),
                StrategyExecuteMsg::Execute { msg: None },
            )
            .unwrap_err(),
            ContractError::generic_err("Already in executing state")
        );
    }

    #[test]
    fn claims_partially_filled_stop_loss_order() {}

    #[test]
    fn claims_and_withdraws_all_existing_orders_if_any_exist() {}

    #[test]
    fn resets_trigger_order_if_funds_remaining() {}

    #[test]
    fn resets_stop_loss_order_if_funds_remaining() {}

    #[test]
    fn resets_check_schedule_if_past_due() {}

    #[test]
    fn distributes_claimed_funds_minus_trigger_order_amount_if_swap_funds_remaining() {}

    #[test]
    fn distributes_all_claimed_funds_if_no_swap_funds_remaining() {}

    #[test]
    fn publishes_execution_attempted_event() {}

    #[test]
    fn publishes_execution_completed_event() {}
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
    fn allows_owner_to_withdraw() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(0u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
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

    #[test]
    fn sends_bank_msg() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(0u128, "swap_denom"),
                    filled: Coin::new(1000u128, "target_denom"),
                    claimed: Coin::new(0u128, "target_denom"),
                    withdrawn: vec![],
                },
            )
            .unwrap();

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![
                Coin::new(1000u128, "target_denom"),
                Coin::new(122u128, "any_other_denom"),
            ],
        );

        let response = execute(
            deps.as_mut(),
            env,
            message_info(&config.owner, &[]),
            StrategyExecuteMsg::Withdraw {
                amounts: vec![
                    Coin::new(1000u128, "target_denom"),
                    Coin::new(1000u128, "any_other_denom"),
                ],
            },
        )
        .unwrap();

        assert_eq!(
            response.messages[0].msg,
            CosmosMsg::Bank(BankMsg::Send {
                to_address: config.owner.to_string(),
                amount: vec![
                    Coin::new(122u128, "any_other_denom"),
                    Coin::new(1000u128, "target_denom"),
                ],
            })
        );
    }

    #[test]
    fn updates_statistics() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let config = default_config();

        CONFIG.save(deps.as_mut().storage, &config).unwrap();

        STATS
            .save(
                deps.as_mut().storage,
                &StopLossStatistics {
                    remaining: Coin::new(100u128, "swap_denom"),
                    filled: Coin::new(1000u128, "target_denom"),
                    claimed: Coin::new(100u128, "target_denom"),
                    withdrawn: vec![
                        Coin::new(100u128, "target_denom"),
                        Coin::new(100u128, "swap_denom"),
                    ],
                },
            )
            .unwrap();

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![
                Coin::new(1000u128, "target_denom"),
                Coin::new(122u128, "any_other_denom"),
            ],
        );

        execute(
            deps.as_mut(),
            env,
            message_info(&config.owner, &[]),
            StrategyExecuteMsg::Withdraw {
                amounts: vec![
                    Coin::new(1000u128, "target_denom"),
                    Coin::new(1000u128, "any_other_denom"),
                ],
            },
        )
        .unwrap();

        assert_eq!(
            STATS.load(deps.as_ref().storage).unwrap(),
            StopLossStatistics {
                remaining: Coin::new(100u128, "swap_denom"),
                filled: Coin::new(1000u128, "target_denom"),
                claimed: Coin::new(100u128, "target_denom"),
                withdrawn: vec![
                    Coin::new(122u128, "any_other_denom"),
                    Coin::new(100u128, "swap_denom"),
                    Coin::new(1100u128, "target_denom"),
                ],
            }
        );
    }
}

// #[cfg(test)]
// fn default_config() -> TwapConfig {
//     let deps = cosmwasm_std::testing::mock_dependencies();
//     TwapConfig {
//         owner: deps.api.addr_make("owner"),
//         manager_contract: deps.api.addr_make("manager"),
//         exchanger_contract: deps.api.addr_make("exchanger"),
//         scheduler_contract: deps.api.addr_make("scheduler"),
//         distributor_contract: deps.api.addr_make("distributor"),
//         swap_amount: Coin::new(1000u128, "rune"),
//         minimum_receive_amount: Coin::new(900u128, "uruji"),
//         maximum_slippage_bps: 100,
//         route: None,
//         swap_cadence: calc_rs::core::Schedule::Blocks {
//             interval: 100,
//             previous: None,
//         },
//         swap_conditions: vec![],
//         schedule_conditions: vec![],
//         execution_rebate: None,
//     }
// }

// #[cfg(test)]
// mod instantiate_tests {
//     use super::*;

//     use calc_rs::{
//         core::{Condition, Schedule},
//         twap::InstantiateTwapCommand,
//     };
//     use calc_rs_test::test::CodeInfoResponse;
//     use cosmwasm_std::{
//         testing::{message_info, mock_dependencies, mock_env},
//         to_json_binary, Addr, Checksum, Coin, ContractResult, Decimal, Event, SubMsg, SystemResult,
//         Uint128, WasmMsg, WasmQuery,
//     };

//     use crate::{
//         contract::{instantiate, BASE_FEE_BPS},
//         state::CONFIG,
//         types::DomainEvent,
//     };

//     fn default_instantiate_msg() -> InstantiateTwapCommand {
//         let deps = mock_dependencies();
//         InstantiateTwapCommand {
//             owner: Addr::unchecked("owner"),
//             exchanger_contract: deps.api.addr_make("exchange"),
//             scheduler_contract: deps.api.addr_make("scheduler"),
//             swap_amount: Coin::new(1000u128, "rune"),
//             minimum_receive_amount: Coin::new(900u128, "uruji"),
//             maximum_slippage_bps: 100,
//             route: None,
//             swap_cadence: Schedule::Blocks {
//                 interval: 100,
//                 previous: None,
//             },
//             distributor_code_id: 1,
//             minimum_distribute_amount: None,
//             affiliate_code: None,
//             mutable_destinations: vec![Destination {
//                 recipient: Recipient::Bank {
//                     address: deps.api.addr_make("destination"),
//                 },
//                 shares: Uint128::new(10000),
//                 label: Some("Mutable Destination".to_string()),
//             }],
//             immutable_destinations: vec![],
//             execution_rebate: None,
//         }
//     }

//     #[test]
//     fn adds_calc_fee_collector_destination() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let msg = default_instantiate_msg();
//         let info = message_info(&deps.api.addr_make("manager"), &[]);

//         deps.querier.update_wasm(move |_| {
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&CodeInfoResponse {
//                     code_id: msg.distributor_code_id,
//                     creator: Addr::unchecked("creator"),
//                     checksum: Checksum::from_hex(
//                         "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
//                     )
//                     .unwrap(),
//                 })
//                 .unwrap(),
//             ))
//         });

//         let fee_collector = Addr::unchecked("fee-collector");

//         let response = instantiate(
//             deps.as_mut(),
//             env.clone(),
//             info,
//             StrategyInstantiateMsg {
//                 fee_collector: fee_collector.clone(),
//                 config: CreateStrategyConfig::Twap(msg.clone()),
//             },
//         )
//         .unwrap();

//         let total_shares_with_fees = msg
//             .mutable_destinations
//             .iter()
//             .chain(msg.immutable_destinations.iter())
//             .fold(Uint128::zero(), |acc, d| acc + d.shares)
//             .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

//         let calc_fee_collector_destination = Destination {
//             recipient: Recipient::Bank {
//                 address: fee_collector.clone(),
//             },
//             shares: total_shares_with_fees.mul_floor(Decimal::bps(BASE_FEE_BPS)),
//             label: Some("CALC".to_string()),
//         };

//         assert_eq!(
//             response.messages[0],
//             SubMsg::new(WasmMsg::Instantiate2 {
//                 admin: Some(env.contract.address.to_string()),
//                 code_id: msg.distributor_code_id,
//                 label: "Distributor".to_string(),
//                 msg: to_json_binary(&DistributorConfig {
//                     owner: msg.owner.clone(),
//                     denoms: vec![msg.minimum_receive_amount.denom.clone()],
//                     mutable_destinations: msg.mutable_destinations,
//                     immutable_destinations: vec![calc_fee_collector_destination],
//                     conditions: vec![],
//                 })
//                 .unwrap(),
//                 funds: vec![],
//                 salt: to_json_binary(&(
//                     msg.owner.to_string().truncate(16),
//                     env.block.time.seconds(),
//                     msg.distributor_code_id
//                 ))
//                 .unwrap(),
//             })
//         );
//     }

//     #[test]
//     fn adds_affiliate_fee_collector_destination() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let msg = InstantiateTwapCommand {
//             affiliate_code: Some("affiliate_code".to_string()),
//             ..default_instantiate_msg()
//         };
//         let manager = deps.api.addr_make("manager");
//         let info = message_info(&manager, &[]);

//         let affiliate_bps = 5;

//         deps.querier.update_wasm(move |query| {
//             SystemResult::Ok(ContractResult::Ok(match query {
//                 WasmQuery::CodeInfo { code_id } => {
//                     assert_eq!(code_id, &msg.distributor_code_id);
//                     to_json_binary(&CodeInfoResponse {
//                         code_id: msg.distributor_code_id.clone(),
//                         creator: Addr::unchecked("creator"),
//                         checksum: Checksum::from_hex(
//                             "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
//                         )
//                         .unwrap(),
//                     })
//                     .unwrap()
//                 }
//                 WasmQuery::Smart { contract_addr, msg } => {
//                     assert_eq!(contract_addr, manager.as_str());
//                     assert_eq!(
//                         msg,
//                         &to_json_binary(&ManagerQueryMsg::Affiliate {
//                             code: "affiliate_code".to_string(),
//                         })
//                         .unwrap()
//                     );
//                     to_json_binary(&Affiliate {
//                         code: "affiliate_code".to_string(),
//                         address: deps.api.addr_make("affiliate"),
//                         bps: affiliate_bps,
//                     })
//                     .unwrap()
//                 }
//                 _ => panic!("Unexpected query type"),
//             }))
//         });

//         let fee_collector = Addr::unchecked("fee-collector");

//         let response = instantiate(
//             deps.as_mut(),
//             env.clone(),
//             info,
//             StrategyInstantiateMsg {
//                 fee_collector: fee_collector.clone(),
//                 config: CreateStrategyConfig::Twap(msg.clone()),
//             },
//         )
//         .unwrap();

//         let total_shares_with_fees = msg
//             .mutable_destinations
//             .iter()
//             .chain(msg.immutable_destinations.iter())
//             .fold(Uint128::zero(), |acc, d| acc + d.shares)
//             .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

//         let calc_fee_collector_destination = Destination {
//             recipient: Recipient::Bank {
//                 address: fee_collector.clone(),
//             },
//             shares: total_shares_with_fees.mul_floor(Decimal::bps(BASE_FEE_BPS - affiliate_bps)),
//             label: Some("CALC".to_string()),
//         };

//         let affiliate_destination = Destination {
//             recipient: Recipient::Bank {
//                 address: deps.api.addr_make("affiliate"),
//             },
//             shares: total_shares_with_fees.mul_floor(Decimal::bps(affiliate_bps)),
//             label: Some(format!(
//                 "Affiliate: {}",
//                 msg.affiliate_code.clone().unwrap()
//             )),
//         };

//         assert_eq!(
//             response.messages[0],
//             SubMsg::new(WasmMsg::Instantiate2 {
//                 admin: Some(env.contract.address.to_string()),
//                 code_id: msg.distributor_code_id,
//                 label: "Distributor".to_string(),
//                 msg: to_json_binary(&DistributorConfig {
//                     owner: msg.owner.clone(),
//                     denoms: vec![msg.minimum_receive_amount.denom.clone()],
//                     mutable_destinations: msg.mutable_destinations,
//                     immutable_destinations: vec![
//                         calc_fee_collector_destination,
//                         affiliate_destination
//                     ],
//                     conditions: vec![],
//                 })
//                 .unwrap(),
//                 funds: vec![],
//                 salt: to_json_binary(&(
//                     msg.owner.to_string().truncate(16),
//                     env.block.time.seconds(),
//                     msg.distributor_code_id
//                 ))
//                 .unwrap(),
//             })
//         );
//     }

//     #[test]
//     fn adds_minimum_distribution_amount_condition() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let msg = InstantiateTwapCommand {
//             minimum_distribute_amount: Some(Coin::new(1000u128, "rune")),
//             ..default_instantiate_msg()
//         };
//         let manager = deps.api.addr_make("manager");
//         let info = message_info(&manager, &[]);

//         deps.querier.update_wasm(move |_| {
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&CodeInfoResponse {
//                     code_id: msg.distributor_code_id.clone(),
//                     creator: Addr::unchecked("creator"),
//                     checksum: Checksum::from_hex(
//                         "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
//                     )
//                     .unwrap(),
//                 })
//                 .unwrap(),
//             ))
//         });

//         let fee_collector = Addr::unchecked("fee-collector");

//         let response = instantiate(
//             deps.as_mut(),
//             env.clone(),
//             info,
//             StrategyInstantiateMsg {
//                 fee_collector: fee_collector.clone(),
//                 config: CreateStrategyConfig::Twap(msg.clone()),
//             },
//         )
//         .unwrap();

//         let config = CONFIG.load(deps.as_ref().storage).unwrap();

//         let total_shares_with_fees = msg
//             .mutable_destinations
//             .iter()
//             .chain(msg.immutable_destinations.iter())
//             .fold(Uint128::zero(), |acc, d| acc + d.shares)
//             .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

//         let calc_fee_collector_destination = Destination {
//             recipient: Recipient::Bank {
//                 address: fee_collector.clone(),
//             },
//             shares: total_shares_with_fees.mul_floor(Decimal::bps(BASE_FEE_BPS)),
//             label: Some("CALC".to_string()),
//         };

//         assert_eq!(
//             response.messages[0],
//             SubMsg::new(WasmMsg::Instantiate2 {
//                 admin: Some(env.contract.address.to_string()),
//                 code_id: msg.distributor_code_id,
//                 label: "Distributor".to_string(),
//                 msg: to_json_binary(&DistributorConfig {
//                     owner: msg.owner.clone(),
//                     denoms: vec![msg.minimum_receive_amount.denom.clone()],
//                     mutable_destinations: msg.mutable_destinations,
//                     immutable_destinations: vec![calc_fee_collector_destination],
//                     conditions: vec![Condition::BalanceAvailable {
//                         address: config.distributor_contract.clone(),
//                         amount: msg.minimum_distribute_amount.unwrap(),
//                     }],
//                 })
//                 .unwrap(),
//                 funds: vec![],
//                 salt: to_json_binary(&(
//                     msg.owner.to_string().truncate(16),
//                     env.block.time.seconds(),
//                     msg.distributor_code_id
//                 ))
//                 .unwrap(),
//             })
//         );
//     }

//     #[test]
//     fn adds_execute_msg() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let msg = default_instantiate_msg();
//         let manager = deps.api.addr_make("manager");
//         let info = message_info(&manager, &[]);

//         deps.querier.update_wasm(move |_| {
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&CodeInfoResponse {
//                     code_id: msg.distributor_code_id.clone(),
//                     creator: Addr::unchecked("creator"),
//                     checksum: Checksum::from_hex(
//                         "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
//                     )
//                     .unwrap(),
//                 })
//                 .unwrap(),
//             ))
//         });

//         let response = instantiate(
//             deps.as_mut(),
//             env.clone(),
//             info,
//             StrategyInstantiateMsg {
//                 fee_collector: Addr::unchecked("fee-collector"),
//                 config: CreateStrategyConfig::Twap(msg.clone()),
//             },
//         )
//         .unwrap();

//         assert_eq!(
//             response.messages[1],
//             SubMsg::new(WasmMsg::Execute {
//                 contract_addr: env.contract.address.to_string(),
//                 msg: to_json_binary(&StrategyExecuteMsg::Execute { msg: None }).unwrap(),
//                 funds: vec![]
//             })
//         );
//     }

//     #[test]
//     fn adds_swap_conditions() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let msg = InstantiateTwapCommand {
//             minimum_distribute_amount: Some(Coin::new(1000u128, "rune")),
//             ..default_instantiate_msg()
//         };
//         let manager = deps.api.addr_make("manager");
//         let info = message_info(&manager, &[]);

//         deps.querier.update_wasm(move |_| {
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&CodeInfoResponse {
//                     code_id: msg.distributor_code_id.clone(),
//                     creator: Addr::unchecked("creator"),
//                     checksum: Checksum::from_hex(
//                         "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
//                     )
//                     .unwrap(),
//                 })
//                 .unwrap(),
//             ))
//         });

//         instantiate(
//             deps.as_mut(),
//             env.clone(),
//             info,
//             StrategyInstantiateMsg {
//                 fee_collector: Addr::unchecked("fee-collector"),
//                 config: CreateStrategyConfig::Twap(msg.clone()),
//             },
//         )
//         .unwrap();

//         let config = CONFIG.load(deps.as_ref().storage).unwrap();

//         assert_eq!(
//             config.swap_conditions,
//             vec![
//                 match msg.swap_cadence {
//                     Schedule::Blocks { interval, previous } => {
//                         Condition::BlocksCompleted(
//                             previous.unwrap_or(env.block.height - interval) + interval,
//                         )
//                     }
//                     Schedule::Time { duration, previous } => Condition::TimestampElapsed(
//                         previous
//                             .unwrap_or(env.block.time)
//                             .plus_seconds(duration.as_secs())
//                     ),
//                 },
//                 Condition::BalanceAvailable {
//                     address: env.contract.address.clone(),
//                     amount: Coin::new(1u128, msg.swap_amount.denom.clone()),
//                 },
//                 Condition::ExchangeLiquidityProvided {
//                     exchanger_contract: msg.exchanger_contract.clone(),
//                     swap_amount: msg.swap_amount.clone(),
//                     minimum_receive_amount: msg.minimum_receive_amount.clone(),
//                     maximum_slippage_bps: msg.maximum_slippage_bps,
//                     route: msg.route.clone(),
//                 },
//             ]
//         );
//     }

//     #[test]
//     fn publishes_strategy_instantiated_event() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let msg = default_instantiate_msg();
//         let manager = deps.api.addr_make("manager");
//         let info = message_info(&manager, &[]);

//         deps.querier.update_wasm(move |_| {
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&CodeInfoResponse {
//                     code_id: msg.distributor_code_id.clone(),
//                     creator: Addr::unchecked("creator"),
//                     checksum: Checksum::from_hex(
//                         "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
//                     )
//                     .unwrap(),
//                 })
//                 .unwrap(),
//             ))
//         });

//         let response = instantiate(
//             deps.as_mut(),
//             env.clone(),
//             info,
//             StrategyInstantiateMsg {
//                 fee_collector: Addr::unchecked("fee-collector"),
//                 config: CreateStrategyConfig::Twap(msg.clone()),
//             },
//         )
//         .unwrap();

//         let config = CONFIG.load(deps.as_ref().storage).unwrap();

//         assert_eq!(
//             response.events[0],
//             Event::from(DomainEvent::TwapStrategyCreated {
//                 contract_address: env.contract.address,
//                 config
//             })
//         );
//     }
// }

// #[cfg(test)]
// mod update_tests {
//     use super::*;

//     use calc_rs::{core::Schedule, exchanger::Route};
//     use cosmwasm_std::{
//         testing::{message_info, mock_dependencies, mock_env},
//         Addr, Coin, Timestamp,
//     };

//     use crate::{
//         contract::{default_config, execute},
//         state::{CONFIG, STATE, STATS},
//     };

//     #[test]
//     fn only_allows_manager_to_update() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&Addr::unchecked("not-owner"), &[]),
//                 StrategyExecuteMsg::Update(StrategyConfig::Twap(config.clone()))
//             )
//             .unwrap_err(),
//             ContractError::Unauthorized {}
//         );

//         STATE.remove(deps.as_mut().storage);

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 StrategyExecuteMsg::Update(StrategyConfig::Twap(config.clone()))
//             )
//             .is_ok(),
//             true
//         );
//     }

//     #[test]
//     fn cannot_update_manager_contract() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         let new_config = StrategyExecuteMsg::Update(StrategyConfig::Twap(TwapConfig {
//             manager_contract: Addr::unchecked("new-manager"),
//             ..config.clone()
//         }));

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 new_config
//             )
//             .unwrap_err(),
//             ContractError::generic_err("Cannot change the manager contract")
//         );
//     }

//     #[test]
//     fn cannot_update_distributor_contract() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         let new_config = StrategyExecuteMsg::Update(StrategyConfig::Twap(TwapConfig {
//             distributor_contract: Addr::unchecked("new-distributor"),
//             ..config.clone()
//         }));

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 new_config
//             )
//             .unwrap_err(),
//             ContractError::generic_err("Cannot change the distributor contract")
//         );
//     }

//     #[test]
//     fn cannot_update_swap_amount_denomination() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         let new_config = StrategyExecuteMsg::Update(StrategyConfig::Twap(TwapConfig {
//             swap_amount: Coin::new(0u128, "new-denom"),
//             ..config.clone()
//         }));

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 new_config
//             )
//             .unwrap_err(),
//             ContractError::generic_err(format!(
//                 "Cannot change the swap amount denomination from {} to {}",
//                 config.swap_amount.denom, "new-denom"
//             ))
//         );
//     }

//     #[test]
//     fn cannot_update_minimum_receive_amount_denomination() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         let new_config = StrategyExecuteMsg::Update(StrategyConfig::Twap(TwapConfig {
//             minimum_receive_amount: Coin::new(0u128, "new-denom"),
//             ..config.clone()
//         }));

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 new_config
//             )
//             .unwrap_err(),
//             ContractError::generic_err(format!(
//                 "Cannot change the minimum receive amount denomination from {} to {}",
//                 config.minimum_receive_amount.denom, "new-denom"
//             ))
//         );
//     }

//     #[test]
//     fn cannot_set_maximum_slippage_bps_to_more_than_10_000() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         let new_config = StrategyExecuteMsg::Update(StrategyConfig::Twap(TwapConfig {
//             maximum_slippage_bps: 10_001,
//             ..config.clone()
//         }));

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 new_config
//             )
//             .unwrap_err(),
//             ContractError::generic_err("Maximum slippage basis points cannot exceed 10,000 (100%)")
//         );
//     }

//     #[test]
//     fn updates_mutable_config() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         let new_config = TwapConfig {
//             swap_amount: Coin::new(32867423_u128, config.swap_amount.denom.clone()),
//             minimum_receive_amount: Coin::new(
//                 32867423_u128,
//                 config.minimum_receive_amount.denom.clone(),
//             ),
//             maximum_slippage_bps: 8767,
//             swap_cadence: Schedule::Blocks {
//                 interval: 236473,
//                 previous: Some(1265),
//             },
//             route: Some(Route::FinMarket {
//                 address: Addr::unchecked("pair"),
//             }),
//             execution_rebate: Some(Coin::new(2u128, "rune")),
//             ..config.clone()
//         };

//         execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&config.manager_contract, &[]),
//             StrategyExecuteMsg::Update(StrategyConfig::Twap(new_config.clone())),
//         )
//         .unwrap();

//         assert_eq!(
//             CONFIG.load(deps.as_ref().storage).unwrap(),
//             TwapConfig {
//                 swap_conditions: vec![
//                     match new_config.swap_cadence {
//                         Schedule::Blocks { interval, previous } => Condition::BlocksCompleted(
//                             previous.unwrap_or(env.block.height.saturating_sub(interval))
//                                 + interval,
//                         ),
//                         Schedule::Time { duration, previous } => Condition::TimestampElapsed(
//                             previous
//                                 .unwrap_or(Timestamp::from_seconds(
//                                     env.block.time.seconds().saturating_sub(duration.as_secs()),
//                                 ))
//                                 .plus_seconds(duration.as_secs()),
//                         ),
//                     },
//                     Condition::BalanceAvailable {
//                         address: env.contract.address.clone(),
//                         amount: Coin::new(1u128, new_config.swap_amount.denom.clone()),
//                     },
//                     Condition::ExchangeLiquidityProvided {
//                         exchanger_contract: config.exchanger_contract.clone(),
//                         swap_amount: new_config.swap_amount.clone(),
//                         minimum_receive_amount: new_config.minimum_receive_amount.clone(),
//                         maximum_slippage_bps: new_config.maximum_slippage_bps,
//                         route: new_config.route.clone(),
//                     },
//                     Condition::BalanceAvailable {
//                         address: env.contract.address.clone(),
//                         amount: new_config.execution_rebate.clone().unwrap()
//                     }
//                 ],
//                 schedule_conditions: vec![
//                     Condition::BalanceAvailable {
//                         address: env.contract.address.clone(),
//                         amount: Coin::new(1u128, new_config.swap_amount.denom.clone()),
//                     },
//                     Condition::StrategyStatus {
//                         manager_contract: config.manager_contract.clone(),
//                         contract_address: env.contract.address.clone(),
//                         status: StrategyStatus::Active
//                     }
//                 ],
//                 ..new_config
//             }
//         );
//     }
// }

// #[cfg(test)]
// mod execute_tests {
//     use super::*;

//     use calc_rs::{core::Schedule, exchanger::ExpectedReceiveAmount, manager::Strategy};
//     use cosmwasm_std::{
//         testing::{message_info, mock_dependencies, mock_env},
//         to_json_binary, Addr, Coin, ContractResult, Event, SubMsg, SystemResult, WasmMsg,
//         WasmQuery,
//     };

//     use crate::{
//         contract::{default_config, execute, EXECUTE_REPLY_ID, SCHEDULE_REPLY_ID},
//         state::{CONFIG, STATE, STATS},
//         types::DomainEvent,
//     };

//     #[test]
//     fn prevents_recursive_execution() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&config.manager_contract, &[]),
//             StrategyExecuteMsg::Execute { msg: None },
//         )
//         .unwrap();

//         assert!(response.messages.contains(&SubMsg::new(WasmMsg::Execute {
//             contract_addr: env.contract.address.to_string(),
//             msg: to_json_binary(&StrategyExecuteMsg::Clear {}).unwrap(),
//             funds: vec![]
//         })));

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .unwrap_err(),
//             ContractError::generic_err("Contract is already in the requested state")
//         );

//         assert_eq!(
//             STATE.load(deps.as_ref().storage).unwrap(),
//             StrategyExecuteMsg::Execute { msg: None }
//         );
//     }

//     #[test]
//     fn clears_state() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         STATE
//             .save(
//                 deps.as_mut().storage,
//                 &StrategyExecuteMsg::Execute { msg: None },
//             )
//             .unwrap();

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&env.contract.address, &[]),
//             StrategyExecuteMsg::Clear {},
//         )
//         .unwrap();

//         assert!(STATE.may_load(deps.as_ref().storage).unwrap().is_none());
//     }

//     #[test]
//     fn only_allows_manager_and_contract_owner_to_execute() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&Addr::unchecked("not-manager"), &[]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .unwrap_err(),
//             ContractError::Unauthorized {}
//         );

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
//         STATE.remove(deps.as_mut().storage);

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .is_ok(),
//             true
//         );

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
//         STATE.remove(deps.as_mut().storage);

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&env.contract.address, &[]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .is_ok(),
//             true
//         );
//     }

//     #[test]
//     fn only_allows_valid_funds() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(
//                     &config.manager_contract,
//                     &[Coin::new(1000u128, config.swap_amount.denom.clone())]
//                 ),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .is_ok(),
//             true
//         );

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
//         STATE.remove(deps.as_mut().storage);

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&env.contract.address, &[Coin::new(500u128, "random")]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .unwrap_err(),
//             ContractError::generic_err(format!(
//                 "Invalid funds provided, only {} is allowed",
//                 config.swap_amount.denom
//             ))
//         );

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();
//         STATE.remove(deps.as_mut().storage);

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(
//                     &env.contract.address,
//                     &[
//                         Coin::new(1000u128, config.swap_amount.denom.clone()),
//                         Coin::new(500u128, "random")
//                     ]
//                 ),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .unwrap_err(),
//             ContractError::generic_err(format!(
//                 "Invalid funds provided, only {} is allowed",
//                 config.swap_amount.denom
//             ))
//         );
//     }

//     #[test]
//     fn returns_error_if_not_due_for_execution() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = TwapConfig {
//             swap_cadence: Schedule::Blocks {
//                 interval: 100,
//                 previous: Some(env.block.height + 50),
//             },
//             ..default_config()
//         };

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .unwrap_err(),
//             ContractError::generic_err(format!(
//                 "DCA strategy is not due for execution until {:?}",
//                 config.swap_cadence.into_condition(&env).description()
//             ))
//         );
//     }

//     #[test]
//     fn executes_swap_if_conditions_met() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         deps.querier.bank.update_balance(
//             env.contract.address.clone(),
//             vec![config.swap_amount.clone()],
//         );

//         deps.querier.update_wasm(move |_| {
//             let minimum_receive_amount = default_config().minimum_receive_amount;
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&ExpectedReceiveAmount {
//                     receive_amount: Coin::new(
//                         minimum_receive_amount.amount.u128() + 1,
//                         minimum_receive_amount.denom.clone(),
//                     ),
//                     slippage_bps: config.maximum_slippage_bps - 1,
//                 })
//                 .unwrap(),
//             ))
//         });

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .unwrap()
//             .messages[0],
//             SubMsg::reply_always(
//                 WasmMsg::Execute {
//                     contract_addr: config.exchanger_contract.to_string(),
//                     msg: to_json_binary(&ExchangeExecuteMsg::Swap {
//                         minimum_receive_amount: config.minimum_receive_amount.clone(),
//                         maximum_slippage_bps: config.maximum_slippage_bps,
//                         route: config.route.clone(),
//                         recipient: Some(config.distributor_contract.clone()),
//                         on_complete: Some(Callback {
//                             contract: config.distributor_contract.clone(),
//                             msg: to_json_binary(&DistributorExecuteMsg::Distribute {}).unwrap(),
//                             execution_rebate: config
//                                 .execution_rebate
//                                 .clone()
//                                 .map_or(vec![], |f| vec![f]),
//                         }),
//                     })
//                     .unwrap(),
//                     funds: vec![config.swap_amount.clone()],
//                 },
//                 EXECUTE_REPLY_ID
//             )
//         );
//     }

//     #[test]
//     fn includes_execution_rebate_if_it_is_set() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let execution_rebate = Coin::new(2u128, "rune");
//         let config = TwapConfig {
//             execution_rebate: Some(execution_rebate.clone()),
//             ..default_config()
//         };

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         deps.querier.bank.update_balance(
//             env.contract.address.clone(),
//             vec![config.swap_amount.clone(), execution_rebate.clone()],
//         );

//         deps.querier.update_wasm(move |_| {
//             let minimum_receive_amount = default_config().minimum_receive_amount;
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&ExpectedReceiveAmount {
//                     receive_amount: Coin::new(
//                         minimum_receive_amount.amount.u128() + 1,
//                         minimum_receive_amount.denom.clone(),
//                     ),
//                     slippage_bps: config.maximum_slippage_bps - 1,
//                 })
//                 .unwrap(),
//             ))
//         });

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .unwrap()
//             .messages[0],
//             SubMsg::reply_always(
//                 WasmMsg::Execute {
//                     contract_addr: config.exchanger_contract.to_string(),
//                     msg: to_json_binary(&ExchangeExecuteMsg::Swap {
//                         minimum_receive_amount: config.minimum_receive_amount.clone(),
//                         maximum_slippage_bps: config.maximum_slippage_bps,
//                         route: config.route.clone(),
//                         recipient: Some(config.distributor_contract.clone()),
//                         on_complete: Some(Callback {
//                             contract: config.distributor_contract.clone(),
//                             msg: to_json_binary(&DistributorExecuteMsg::Distribute {}).unwrap(),
//                             execution_rebate: vec![execution_rebate.clone()],
//                         }),
//                     })
//                     .unwrap(),
//                     funds: vec![config.swap_amount, execution_rebate],
//                 },
//                 EXECUTE_REPLY_ID
//             )
//         );
//     }

//     #[test]
//     fn skips_execution_if_all_conditions_not_met() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         deps.querier.update_wasm(|_| {
//             let config = default_config();
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&ExpectedReceiveAmount {
//                     receive_amount: config.minimum_receive_amount,
//                     slippage_bps: config.maximum_slippage_bps + 1,
//                 })
//                 .unwrap(),
//             ))
//         });

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .unwrap()
//             .events[0],
//             Event::from(DomainEvent::TwapExecutionSkipped {
//                 contract_address: env.contract.address.clone(),
//                 reason: format!(
//                     "Execution skipped due to the following reasons:\n* {}",
//                     vec![
//                         Condition::BalanceAvailable {
//                             address: env.contract.address.clone(),
//                             amount: Coin::new(1u128, config.swap_amount.denom.clone())
//                         }
//                         .check(deps.as_ref(), &env)
//                         .unwrap_err(),
//                         Condition::ExchangeLiquidityProvided {
//                             exchanger_contract: config.exchanger_contract,
//                             swap_amount: config.swap_amount,
//                             minimum_receive_amount: config.minimum_receive_amount,
//                             maximum_slippage_bps: config.maximum_slippage_bps,
//                             route: config.route.clone(),
//                         }
//                         .check(deps.as_ref(), &env)
//                         .unwrap_err(),
//                     ]
//                     .iter()
//                     .map(|e| e.to_string())
//                     .collect::<Vec<_>>()
//                     .join("\n* ")
//                 )
//             })
//         );
//     }

//     #[test]
//     fn skips_execution_if_any_condition_not_met() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         deps.querier.update_wasm(|_| {
//             let config = default_config();
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&ExpectedReceiveAmount {
//                     receive_amount: config.minimum_receive_amount,
//                     slippage_bps: config.maximum_slippage_bps + 1,
//                 })
//                 .unwrap(),
//             ))
//         });

//         deps.querier.bank.update_balance(
//             env.contract.address.clone(),
//             vec![config.swap_amount.clone()],
//         );

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .unwrap()
//             .events[0],
//             Event::from(DomainEvent::TwapExecutionSkipped {
//                 contract_address: env.contract.address.clone(),
//                 reason: format!(
//                     "Execution skipped due to the following reasons:\n* {}",
//                     vec![Condition::ExchangeLiquidityProvided {
//                         exchanger_contract: config.exchanger_contract,
//                         swap_amount: config.swap_amount,
//                         minimum_receive_amount: config.minimum_receive_amount,
//                         maximum_slippage_bps: config.maximum_slippage_bps,
//                         route: config.route.clone(),
//                     }
//                     .check(deps.as_ref(), &env)
//                     .unwrap_err(),]
//                     .iter()
//                     .map(|e| e.to_string())
//                     .collect::<Vec<_>>()
//                     .join("\n* ")
//                 )
//             })
//         );
//     }

//     #[test]
//     fn schedules_next_execution_if_conditions_met() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let exchanger_contract = Addr::unchecked("exchanger");
//         let manager_contract = Addr::unchecked("manager");

//         let config = TwapConfig {
//             exchanger_contract: exchanger_contract.clone(),
//             manager_contract: manager_contract.clone(),
//             schedule_conditions: vec![Condition::BalanceAvailable {
//                 address: env.contract.address.clone(),
//                 amount: default_config().swap_amount,
//             }],
//             ..default_config()
//         };

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         deps.querier.bank.update_balance(
//             env.contract.address.clone(),
//             vec![config.swap_amount.clone()],
//         );

//         deps.querier.update_wasm(move |query| {
//             SystemResult::Ok(ContractResult::Ok(match query {
//                 WasmQuery::Smart { contract_addr, .. } => {
//                     if contract_addr == &exchanger_contract.to_string() {
//                         let minimum_receive_amount = default_config().minimum_receive_amount;
//                         to_json_binary(&ExpectedReceiveAmount {
//                             receive_amount: Coin::new(
//                                 minimum_receive_amount.amount.u128() + 1,
//                                 minimum_receive_amount.denom.clone(),
//                             ),
//                             slippage_bps: config.maximum_slippage_bps - 1,
//                         })
//                         .unwrap()
//                     } else {
//                         to_json_binary(&Strategy {
//                             id: 1,
//                             owner: Addr::unchecked("owner"),
//                             contract_address: Addr::unchecked("contract"),
//                             created_at: 0,
//                             updated_at: 0,
//                             label: "test".to_string(),
//                             status: StrategyStatus::Active,
//                             affiliates: vec![],
//                         })
//                         .unwrap()
//                     }
//                 }
//                 _ => panic!("Unexpected query type"),
//             }))
//         });

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .unwrap()
//             .messages[1],
//             SubMsg::reply_always(
//                 WasmMsg::Execute {
//                     contract_addr: config.scheduler_contract.to_string(),
//                     msg: to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
//                         conditions: vec![config.swap_cadence.next(&env).into_condition(&env)],
//                         threshold: TriggerConditionsThreshold::All,
//                         to: config.manager_contract.clone(),
//                         msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
//                             contract_address: env.contract.address.clone(),
//                             msg: Some(
//                                 to_json_binary(&StrategyExecuteMsg::Execute { msg: None }).unwrap()
//                             ),
//                         })
//                         .unwrap(),
//                     }]))
//                     .unwrap(),
//                     funds: config.execution_rebate.map_or(vec![], |c| vec![c])
//                 },
//                 SCHEDULE_REPLY_ID
//             )
//         );
//     }

//     #[test]
//     fn progresses_swap_cadence_if_conditions_are_met() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let exchanger_contract = Addr::unchecked("exchanger");
//         let manager_contract = Addr::unchecked("manager");

//         let config = TwapConfig {
//             exchanger_contract: exchanger_contract.clone(),
//             manager_contract: manager_contract.clone(),
//             schedule_conditions: vec![Condition::BalanceAvailable {
//                 address: env.contract.address.clone(),
//                 amount: default_config().swap_amount,
//             }],
//             ..default_config()
//         };

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         deps.querier.bank.update_balance(
//             env.contract.address.clone(),
//             vec![config.swap_amount.clone()],
//         );

//         deps.querier.update_wasm(move |query| {
//             SystemResult::Ok(ContractResult::Ok(match query {
//                 WasmQuery::Smart { contract_addr, .. } => {
//                     if contract_addr == &exchanger_contract.to_string() {
//                         let minimum_receive_amount = default_config().minimum_receive_amount;
//                         to_json_binary(&ExpectedReceiveAmount {
//                             receive_amount: Coin::new(
//                                 minimum_receive_amount.amount.u128() + 1,
//                                 minimum_receive_amount.denom.clone(),
//                             ),
//                             slippage_bps: config.maximum_slippage_bps - 1,
//                         })
//                         .unwrap()
//                     } else {
//                         to_json_binary(&Strategy {
//                             id: 1,
//                             owner: Addr::unchecked("owner"),
//                             contract_address: Addr::unchecked("contract"),
//                             created_at: 0,
//                             updated_at: 0,
//                             label: "test".to_string(),
//                             status: StrategyStatus::Active,
//                             affiliates: vec![],
//                         })
//                         .unwrap()
//                     }
//                 }
//                 _ => panic!("Unexpected query type"),
//             }))
//         });

//         execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&config.manager_contract, &[]),
//             StrategyExecuteMsg::Execute { msg: None },
//         )
//         .unwrap();

//         assert_ne!(
//             CONFIG.load(deps.as_ref().storage).unwrap().swap_cadence,
//             config.swap_cadence
//         );

//         assert_eq!(
//             CONFIG.load(deps.as_ref().storage).unwrap().swap_cadence,
//             config.swap_cadence.next(&env)
//         );
//     }

//     #[test]
//     fn skips_scheduling_if_all_conditions_not_met() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = TwapConfig {
//             schedule_conditions: vec![Condition::BalanceAvailable {
//                 address: env.contract.address.clone(),
//                 amount: default_config().swap_amount,
//             }],
//             ..default_config()
//         };

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         deps.querier.update_wasm(|_| {
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&Strategy {
//                     id: 1,
//                     owner: Addr::unchecked("owner"),
//                     contract_address: Addr::unchecked("contract"),
//                     created_at: 0,
//                     updated_at: 0,
//                     label: "test".to_string(),
//                     status: StrategyStatus::Paused,
//                     affiliates: vec![],
//                 })
//                 .unwrap(),
//             ))
//         });

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .unwrap()
//             .events[1],
//             Event::from(DomainEvent::TwapSchedulingSkipped {
//                 contract_address: env.contract.address.clone(),
//                 reason: format!(
//                     "Scheduling skipped due to the following reasons:\n* {}",
//                     vec![
//                         Condition::BalanceAvailable {
//                             address: env.contract.address.clone(),
//                             amount: Coin::new(1u128, config.swap_amount.denom.clone())
//                         }
//                         .check(deps.as_ref(), &env)
//                         .unwrap_err(),
//                         Condition::StrategyStatus {
//                             manager_contract: env.contract.address.clone(),
//                             contract_address: env.contract.address.clone(),
//                             status: StrategyStatus::Active
//                         }
//                         .check(deps.as_ref(), &env)
//                         .unwrap_err(),
//                     ]
//                     .iter()
//                     .map(|e| e.to_string())
//                     .collect::<Vec<_>>()
//                     .join("\n* ")
//                 )
//             })
//         );
//     }

//     #[test]
//     fn skips_scheduling_if_any_condition_not_met() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = TwapConfig {
//             schedule_conditions: vec![Condition::BalanceAvailable {
//                 address: env.contract.address.clone(),
//                 amount: default_config().swap_amount,
//             }],
//             ..default_config()
//         };

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         deps.querier.bank.update_balance(
//             env.contract.address.clone(),
//             vec![config.swap_amount.clone()],
//         );

//         deps.querier.update_wasm(|_| {
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&Strategy {
//                     id: 1,
//                     owner: Addr::unchecked("owner"),
//                     contract_address: Addr::unchecked("contract"),
//                     created_at: 0,
//                     updated_at: 0,
//                     label: "test".to_string(),
//                     status: StrategyStatus::Paused,
//                     affiliates: vec![],
//                 })
//                 .unwrap(),
//             ))
//         });

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 StrategyExecuteMsg::Execute { msg: None }
//             )
//             .unwrap()
//             .events[1],
//             Event::from(DomainEvent::TwapSchedulingSkipped {
//                 contract_address: env.contract.address.clone(),
//                 reason: format!(
//                     "Scheduling skipped due to the following reasons:\n* {}",
//                     vec![Condition::StrategyStatus {
//                         manager_contract: env.contract.address.clone(),
//                         contract_address: env.contract.address.clone(),
//                         status: StrategyStatus::Active
//                     }
//                     .check(deps.as_ref(), &env)
//                     .unwrap_err(),]
//                     .iter()
//                     .map(|e| e.to_string())
//                     .collect::<Vec<_>>()
//                     .join("\n* ")
//                 )
//             })
//         );
//     }

//     #[test]
//     fn updates_statistics() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         deps.querier.bank.update_balance(
//             env.contract.address.clone(),
//             vec![config.swap_amount.clone()],
//         );

//         deps.querier.update_wasm(move |_| {
//             let minimum_receive_amount = default_config().minimum_receive_amount;
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&ExpectedReceiveAmount {
//                     receive_amount: Coin::new(
//                         minimum_receive_amount.amount.u128() + 1,
//                         minimum_receive_amount.denom.clone(),
//                     ),
//                     slippage_bps: config.maximum_slippage_bps - 1,
//                 })
//                 .unwrap(),
//             ))
//         });

//         execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&config.manager_contract, &[]),
//             StrategyExecuteMsg::Execute { msg: None },
//         )
//         .unwrap();

//         let stats = STATS.load(deps.as_ref().storage).unwrap();
//         assert_eq!(stats.swapped, config.swap_amount);
//     }
// }

// #[cfg(test)]
// mod withdraw_tests {
//     use super::*;

//     use cosmwasm_std::{
//         testing::{message_info, mock_dependencies, mock_env},
//         Coin,
//     };

//     use crate::{
//         contract::{default_config, execute},
//         state::{CONFIG, STATE, STATS},
//     };

//     #[test]
//     fn only_allows_owner_to_withdraw() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();
//         let config = default_config();

//         CONFIG.save(deps.as_mut().storage, &env, &config).unwrap();

//         STATS
//             .save(
//                 deps.as_mut().storage,
//                 &TwapStatistics {
//                     swapped: Coin::new(0u128, config.swap_amount.denom.clone()),
//                     withdrawn: vec![],
//                 },
//             )
//             .unwrap();

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.manager_contract, &[]),
//                 StrategyExecuteMsg::Withdraw { amounts: vec![] }
//             )
//             .unwrap_err(),
//             ContractError::Unauthorized {}
//         );

//         STATE.remove(deps.as_mut().storage);

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&env.contract.address, &[]),
//                 StrategyExecuteMsg::Withdraw { amounts: vec![] }
//             )
//             .unwrap_err(),
//             ContractError::Unauthorized {}
//         );

//         STATE.remove(deps.as_mut().storage);

//         assert_eq!(
//             execute(
//                 deps.as_mut(),
//                 env.clone(),
//                 message_info(&config.owner, &[]),
//                 StrategyExecuteMsg::Withdraw { amounts: vec![] }
//             )
//             .is_ok(),
//             true
//         );
//     }
// }
