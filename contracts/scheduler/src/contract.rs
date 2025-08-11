use std::vec;

use calc_rs::{
    conditions::condition::Condition,
    core::{Contract, ContractError, ContractResult},
    scheduler::{SchedulerExecuteMsg, SchedulerInstantiateMsg, SchedulerQueryMsg, Trigger},
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Coin, Coins, Deps, DepsMut, Env, MessageInfo, Reply, Response,
    StdResult, SubMsg, SubMsgResult,
};
use rujira_rs::fin::{ConfigResponse, ExecuteMsg, OrderResponse, Price, QueryMsg};

use crate::state::TRIGGERS;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: SchedulerInstantiateMsg,
) -> ContractResult {
    Ok(Response::default())
}

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> ContractResult {
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: SchedulerExecuteMsg,
) -> ContractResult {
    match msg {
        SchedulerExecuteMsg::Create(create_command) => {
            let mut sub_messages = Vec::with_capacity(2);
            let trigger_id = create_command.id()?;

            if let Ok(existing_trigger) = TRIGGERS.load(deps.storage, trigger_id) {
                TRIGGERS.delete(deps.storage, existing_trigger.id.into())?;

                if !existing_trigger.execution_rebate.is_empty() {
                    sub_messages.push(SubMsg::reply_never(BankMsg::Send {
                        to_address: info.sender.to_string(),
                        amount: existing_trigger.execution_rebate,
                    }));
                }
            }

            let mut execution_rebate = Coins::try_from(info.funds)?;

            if let Condition::FinLimitOrderFilled {
                pair_address,
                side,
                price,
                ..
            } = &create_command.condition
            {
                let pair = deps.querier.query_wasm_smart::<ConfigResponse>(
                    pair_address.clone(),
                    &QueryMsg::Config {},
                )?;

                let bid_denom = pair.denoms.bid(&side);
                let bid_amount = Coin::new(execution_rebate.amount_of(bid_denom), bid_denom);

                if bid_amount.amount.is_zero() {
                    return Err(ContractError::generic_err("No funds sent for limit order"));
                }

                execution_rebate.sub(bid_amount.clone())?;

                let set_order_msg = SubMsg::reply_never(Contract(pair_address.clone()).call(
                    to_json_binary(&ExecuteMsg::Order((
                        vec![(side.clone(), Price::Fixed(*price), Some(bid_amount.amount))],
                        None,
                    )))?,
                    vec![bid_amount],
                ));

                sub_messages.push(set_order_msg);
            }

            TRIGGERS.save(
                deps.storage,
                &Trigger {
                    id: trigger_id,
                    condition: create_command.condition,
                    msg: create_command.msg,
                    contract_address: create_command.contract_address,
                    executors: create_command.executors,
                    execution_rebate: execution_rebate.to_vec(),
                    jitter: create_command.jitter,
                },
            )?;

            Ok(Response::default().add_submessages(sub_messages))
        }
        SchedulerExecuteMsg::Execute(ids) => {
            let mut sub_messages = Vec::with_capacity(ids.len() * 3);

            for id in ids {
                let trigger = match TRIGGERS.load(deps.storage, id) {
                    Ok(trigger) => trigger,
                    Err(_) => continue,
                };

                if !trigger.executors.is_empty() && !trigger.executors.contains(&info.sender) {
                    continue;
                }

                if let Ok(trigger_is_satisfied) =
                    trigger.condition.is_satisfied(deps.as_ref(), &env)
                {
                    if !trigger_is_satisfied {
                        continue;
                    }

                    if let Condition::FinLimitOrderFilled {
                        pair_address,
                        side,
                        price,
                        ..
                    } = trigger.condition
                    {
                        let order = deps.querier.query_wasm_smart::<OrderResponse>(
                            &pair_address,
                            &QueryMsg::Order((
                                env.contract.address.to_string(),
                                side.clone(),
                                Price::Fixed(price),
                            )),
                        )?;

                        let withdraw_order_msg =
                            SubMsg::reply_never(Contract(pair_address.clone()).call(
                                to_json_binary(&ExecuteMsg::Order((
                                    vec![(side.clone(), Price::Fixed(price), None)],
                                    None,
                                )))?,
                                vec![],
                            ));

                        sub_messages.push(withdraw_order_msg);

                        let pair = deps.querier.query_wasm_smart::<ConfigResponse>(
                            pair_address,
                            &QueryMsg::Config {},
                        )?;

                        let rebate_msg = SubMsg::reply_never(BankMsg::Send {
                            to_address: info.sender.to_string(),
                            amount: vec![Coin::new(order.filled, pair.denoms.ask(&side))],
                        });

                        sub_messages.push(rebate_msg);
                    }
                }

                TRIGGERS.delete(deps.storage, trigger.id.into())?;

                let execute_msg = SubMsg::reply_on_error(
                    Contract(trigger.contract_address).call(trigger.msg, vec![]),
                    0,
                );

                sub_messages.push(execute_msg);

                if !trigger.execution_rebate.is_empty() {
                    let rebate_msg = SubMsg::reply_never(BankMsg::Send {
                        to_address: info.sender.to_string(),
                        amount: trigger.execution_rebate,
                    });

                    sub_messages.push(rebate_msg);
                }
            }

            Ok(Response::default().add_submessages(sub_messages))
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: SchedulerQueryMsg) -> StdResult<Binary> {
    match msg {
        SchedulerQueryMsg::Filtered { filter, limit } => {
            to_json_binary(&TRIGGERS.filtered(deps.storage, filter, limit)?)
        }
        SchedulerQueryMsg::CanExecute(id) => to_json_binary(
            &TRIGGERS
                .load(deps.storage, id)?
                .condition
                .is_satisfied(deps, &env)?,
        ),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, reply: Reply) -> ContractResult {
    match reply.result {
        SubMsgResult::Ok(_) => Ok(Response::default()),
        SubMsgResult::Err(err) => Ok(Response::default().add_attribute("msg_error", err)),
    }
}

#[cfg(test)]
mod create_trigger_tests {
    use super::*;
    use calc_rs::{
        conditions::condition::Condition,
        scheduler::{ConditionFilter, CreateTriggerMsg, Trigger},
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr, Coin, ContractResult as ContractQueryResult, Decimal, SystemResult,
    };
    use rujira_rs::fin::{Denoms, Price, Side, Tick};

    #[test]
    fn creates_block_trigger_correctly() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let info = message_info(&owner.clone(), &[Coin::new(3123_u128, "rune")]);

        let condition = Condition::BlocksCompleted(env.block.height + 10);

        let create_trigger_msg = CreateTriggerMsg {
            condition: condition.clone(),
            msg: Binary::default(),
            contract_address: owner.clone(),
            executors: vec![],
            jitter: None,
        };

        execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            SchedulerExecuteMsg::Create(Box::new(create_trigger_msg.clone())),
        )
        .unwrap();

        let triggers = TRIGGERS
            .filtered(
                deps.as_ref().storage,
                ConditionFilter::BlockHeight {
                    start: None,
                    end: None,
                },
                None,
            )
            .unwrap();

        assert_eq!(
            triggers,
            vec![Trigger {
                id: create_trigger_msg.id().unwrap(),
                contract_address: owner,
                msg: Binary::default(),
                condition: condition.clone(),
                execution_rebate: info.funds.clone(),
                executors: vec![],
                jitter: None
            }]
        );
    }

    #[test]
    fn updates_existing_block_trigger_correctly() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let info = message_info(&owner.clone(), &[Coin::new(3123_u128, "rune")]);

        let condition = Condition::BlocksCompleted(env.block.height + 10);

        let create_trigger_msg = CreateTriggerMsg {
            condition: condition.clone(),
            msg: Binary::default(),
            contract_address: owner.clone(),
            executors: vec![],
            jitter: None,
        };

        execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            SchedulerExecuteMsg::Create(Box::new(create_trigger_msg.clone())),
        )
        .unwrap();

        let triggers = TRIGGERS
            .filtered(
                deps.as_ref().storage,
                ConditionFilter::BlockHeight {
                    start: None,
                    end: None,
                },
                None,
            )
            .unwrap();

        assert_eq!(
            triggers,
            vec![Trigger {
                id: create_trigger_msg.id().unwrap(),
                contract_address: owner.clone(),
                msg: Binary::default(),
                executors: vec![],
                jitter: None,
                condition: condition.clone(),
                execution_rebate: info.funds.clone(),
            }]
        );

        let updated_info = message_info(&owner.clone(), &[Coin::new(1234_u128, "rune")]);

        let create_trigger_msg = CreateTriggerMsg {
            condition: condition.clone(),
            msg: Binary::default(),
            contract_address: owner.clone(),
            executors: vec![],
            jitter: None,
        };

        execute(
            deps.as_mut(),
            env.clone(),
            updated_info.clone(),
            SchedulerExecuteMsg::Create(Box::new(create_trigger_msg.clone())),
        )
        .unwrap();

        let updated_triggers = TRIGGERS
            .filtered(
                deps.as_ref().storage,
                ConditionFilter::Timestamp {
                    start: None,
                    end: None,
                },
                None,
            )
            .unwrap();

        assert_eq!(
            updated_triggers,
            vec![Trigger {
                id: create_trigger_msg.id().unwrap(),
                contract_address: owner,
                msg: Binary::default(),
                executors: vec![],
                jitter: None,
                condition: condition.clone(),
                execution_rebate: updated_info.funds.clone(),
            }]
        );
    }

    #[test]
    fn fails_to_create_limit_order_trigger_if_no_bid_amount_sent() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        let pair_address = Addr::unchecked("pair-0");

        let condition = Condition::FinLimitOrderFilled {
            owner: Some(Addr::unchecked("owner")),
            pair_address,
            side: Side::Base,
            price: Decimal::percent(100),
        };

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractQueryResult::Ok(
                to_json_binary(&ConfigResponse {
                    denoms: Denoms::new("btc-btc", "rune"),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(1),
                    fee_taker: Decimal::percent(10),
                    fee_maker: Decimal::percent(5),
                    fee_address: Addr::unchecked("fee_address").to_string(),
                })
                .unwrap(),
            ))
        });

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[]),
            SchedulerExecuteMsg::Create(Box::new(CreateTriggerMsg {
                condition: condition.clone(),
                msg: Binary::default(),
                contract_address: owner.clone(),
                executors: vec![],
                jitter: None
            })),
        )
        .unwrap_err()
        .to_string()
        .contains("No funds sent for limit order"));

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[Coin::new(1213_u128, "random-denom")]),
            SchedulerExecuteMsg::Create(Box::new(CreateTriggerMsg {
                condition: condition.clone(),
                msg: Binary::default(),
                contract_address: owner.clone(),
                executors: vec![],
                jitter: None
            })),
        )
        .unwrap_err()
        .to_string()
        .contains("No funds sent for limit order"));
    }

    #[test]
    fn fails_to_create_limit_order_trigger_if_pair_does_not_exist() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let info = message_info(&owner, &[Coin::new(1234_u128, "rune")]);

        let pair_address = Addr::unchecked("pair-0");

        let condition = Condition::FinLimitOrderFilled {
            owner: Some(Addr::unchecked("owner")),
            pair_address: pair_address.clone(),
            side: Side::Base,
            price: Decimal::percent(100),
        };

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            info,
            SchedulerExecuteMsg::Create(Box::new(CreateTriggerMsg {
                condition: condition.clone(),
                msg: Binary::default(),
                contract_address: owner.clone(),
                executors: vec![],
                jitter: None
            })),
        )
        .unwrap_err()
        .to_string()
        .contains(format!("Querier system error: No such contract: {pair_address}").as_str()));
    }

    #[test]
    fn sets_limit_order_with_provided_bid_denom_funds() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let bid_amount = Coin::new(1234_u128, "btc-btc");
        let info = message_info(&owner, &[bid_amount.clone()]);

        let pair_address = Addr::unchecked("pair-0");
        let side = Side::Base;
        let price = Decimal::percent(100);

        let condition = Condition::FinLimitOrderFilled {
            owner: Some(Addr::unchecked("owner")),
            pair_address: pair_address.clone(),
            side: side.clone(),
            price: price,
        };

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractQueryResult::Ok(
                to_json_binary(&ConfigResponse {
                    denoms: Denoms::new("btc-btc", "rune"),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(1),
                    fee_taker: Decimal::percent(10),
                    fee_maker: Decimal::percent(5),
                    fee_address: Addr::unchecked("fee_address").to_string(),
                })
                .unwrap(),
            ))
        });

        let response = execute(
            deps.as_mut(),
            env.clone(),
            info,
            SchedulerExecuteMsg::Create(Box::new(CreateTriggerMsg {
                condition: condition.clone(),
                msg: Binary::default(),
                contract_address: owner.clone(),
                executors: vec![],
                jitter: None,
            })),
        )
        .unwrap();

        assert_eq!(
            response.messages,
            vec![SubMsg::reply_never(
                Contract(pair_address.clone()).call(
                    to_json_binary(&ExecuteMsg::Order((
                        vec![(side, Price::Fixed(price), Some(bid_amount.amount),)],
                        None,
                    )))
                    .unwrap(),
                    vec![bid_amount],
                ),
            )]
        )
    }

    #[test]
    fn saves_remaining_funds_as_limit_order_trigger_execution_rebate() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let info = message_info(
            &owner,
            &[
                Coin::new(1234_u128, "rune"),
                Coin::new(1234_u128, "eth-eth"),
            ],
        );

        let pair_address = Addr::unchecked("pair-0");

        let condition = Condition::FinLimitOrderFilled {
            owner: Some(Addr::unchecked("owner")),
            pair_address: pair_address.clone(),
            side: Side::Quote,
            price: Decimal::percent(100),
        };

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractQueryResult::Ok(
                to_json_binary(&ConfigResponse {
                    denoms: Denoms::new("btc-btc", "rune"),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(1),
                    fee_taker: Decimal::percent(10),
                    fee_maker: Decimal::percent(5),
                    fee_address: Addr::unchecked("fee_address").to_string(),
                })
                .unwrap(),
            ))
        });

        let create_trigger_msg = CreateTriggerMsg {
            condition: condition.clone(),
            msg: Binary::default(),
            contract_address: owner.clone(),
            executors: vec![],
            jitter: None,
        };

        execute(
            deps.as_mut(),
            env.clone(),
            info,
            SchedulerExecuteMsg::Create(Box::new(create_trigger_msg.clone())),
        )
        .unwrap();

        let triggers = TRIGGERS
            .filtered(
                deps.as_ref().storage,
                ConditionFilter::LimitOrder {
                    pair_address: pair_address.clone(),
                    price_range: None,
                },
                None,
            )
            .unwrap();

        assert_eq!(
            triggers,
            vec![Trigger {
                id: create_trigger_msg.id().unwrap(),
                contract_address: owner,
                msg: Binary::default(),
                executors: vec![],
                jitter: None,
                condition: condition.clone(),
                execution_rebate: vec![Coin::new(1234_u128, "eth-eth")],
            }]
        );
    }
}

#[cfg(test)]
mod execute_trigger_tests {
    use super::*;
    use calc_rs::conditions::condition::Condition;
    use calc_rs::manager::ManagerExecuteMsg;
    use calc_rs::scheduler::{ConditionFilter, CreateTriggerMsg};
    use cosmwasm_std::testing::message_info;
    use cosmwasm_std::{from_json, Addr, Decimal, Uint128, Uint64, WasmMsg, WasmQuery};
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Coin, ContractResult as CosmosContractResult, SubMsg, SystemResult,
    };
    use rujira_rs::fin::{ConfigResponse, Denoms, OrderResponse, Price, QueryMsg, Side, Tick};

    #[test]
    fn fails_silently_if_trigger_does_not_exist() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let execution_info = MessageInfo {
            sender: deps.api.addr_make("executor"),
            funds: vec![],
        };

        let response = execute(
            deps.as_mut(),
            env.clone(),
            execution_info.clone(),
            SchedulerExecuteMsg::Execute(vec![Uint64::one()]),
        )
        .unwrap();

        assert!(response.messages.is_empty());
    }

    #[test]
    fn fails_silently_if_trigger_cannot_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let condition =
            Condition::BlocksCompleted(cosmwasm_std::testing::mock_env().block.height + 10);

        let create_trigger_msg = CreateTriggerMsg {
            condition: condition.clone(),
            msg: Binary::default(),
            contract_address: owner.clone(),
            executors: vec![],
            jitter: None,
        };

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[Coin::new(327612u128, "rune")]),
            SchedulerExecuteMsg::Create(Box::new(create_trigger_msg.clone())),
        )
        .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![create_trigger_msg.id().unwrap()]),
        )
        .unwrap();

        assert!(response.messages.is_empty());
    }

    #[test]
    fn withdraws_limit_order_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let manager = deps.api.addr_make("manager");

        let side = Side::Base;
        let price = Decimal::percent(100);

        let condition = Condition::FinLimitOrderFilled {
            owner: Some(Addr::unchecked("owner")),
            pair_address: Addr::unchecked("pair-0"),
            side: side.clone(),
            price,
        };

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(CosmosContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new("btc-btc", "rune"),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(1),
                        fee_taker: Decimal::percent(10),
                        fee_maker: Decimal::percent(5),
                        fee_address: Addr::unchecked("fee_address").to_string(),
                    })
                    .unwrap(),
                    QueryMsg::Order(_) => to_json_binary(&OrderResponse {
                        filled: Uint128::new(21312),
                        owner: Addr::unchecked("contract").to_string(),
                        rate: Decimal::percent(10),
                        updated_at: env.block.time,
                        offer: Uint128::new(12321),
                        remaining: Uint128::zero(),
                        side: Side::Base,
                        price: Price::Fixed(Decimal::percent(100)),
                    })
                    .unwrap(),
                    _ => panic!("Unexpected query type"),
                },
                _ => panic!("Unexpected query type"),
            }))
        });

        let remaining_rebate = Coin::new(12312u128, "other");

        TRIGGERS
            .save(
                deps.as_mut().storage,
                &Trigger {
                    id: Uint64::new(156434254),
                    contract_address: manager.clone(),
                    msg: to_json_binary(&ManagerExecuteMsg::Execute {
                        contract_address: owner.clone(),
                    })
                    .unwrap(),
                    condition: condition.clone(),
                    execution_rebate: vec![remaining_rebate.clone()],
                    executors: vec![],
                    jitter: None,
                },
            )
            .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![Uint64::new(156434254)]),
        )
        .unwrap();

        assert_eq!(
            response.messages,
            vec![
                SubMsg::reply_never(
                    Contract(Addr::unchecked("pair-0")).call(
                        to_json_binary(&ExecuteMsg::Order((
                            vec![(side.clone(), Price::Fixed(price), None)],
                            None,
                        )))
                        .unwrap(),
                        vec![],
                    )
                ),
                SubMsg::reply_never(BankMsg::Send {
                    to_address: executor.to_string(),
                    amount: vec![Coin::new(21312u128, "rune")],
                }),
                SubMsg::reply_on_error(
                    WasmMsg::Execute {
                        contract_addr: manager.to_string(),
                        msg: to_json_binary(&ManagerExecuteMsg::Execute {
                            contract_address: owner.clone(),
                        })
                        .unwrap(),
                        funds: vec![],
                    },
                    0,
                ),
                SubMsg::reply_never(BankMsg::Send {
                    to_address: executor.to_string(),
                    amount: vec![remaining_rebate],
                }),
            ]
        )
    }

    #[test]
    fn adds_execute_message_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let manager = deps.api.addr_make("creator");
        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        let condition = Condition::BlocksCompleted(env.block.height - 10);

        let create_trigger_msg = CreateTriggerMsg {
            condition: condition.clone(),
            msg: to_json_binary(&ManagerExecuteMsg::Execute {
                contract_address: owner.clone(),
            })
            .unwrap(),
            contract_address: manager.clone(),
            executors: vec![],
            jitter: None,
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::Create(Box::new(create_trigger_msg.clone())),
        )
        .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![create_trigger_msg.id().unwrap()]),
        )
        .unwrap();

        assert!(response.messages.contains(&SubMsg::reply_on_error(
            WasmMsg::Execute {
                contract_addr: manager.to_string(),
                msg: to_json_binary(&ManagerExecuteMsg::Execute {
                    contract_address: owner.clone(),
                })
                .unwrap(),
                funds: vec![]
            },
            0
        )));
    }

    #[test]
    fn adds_send_rebate_msg_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let manager = deps.api.addr_make("creator");
        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        let condition = Condition::BlocksCompleted(env.block.height - 10);

        let create_trigger_msg = CreateTriggerMsg {
            condition: condition.clone(),
            contract_address: manager.clone(),
            msg: to_json_binary(&ManagerExecuteMsg::Execute {
                contract_address: owner.clone(),
            })
            .unwrap(),
            executors: vec![],
            jitter: None,
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::Create(Box::new(create_trigger_msg.clone())),
        )
        .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![create_trigger_msg.id().unwrap()]),
        )
        .unwrap();

        assert!(response.messages.contains(&SubMsg::reply_on_error(
            WasmMsg::Execute {
                contract_addr: manager.to_string(),
                msg: to_json_binary(&ManagerExecuteMsg::Execute {
                    contract_address: owner.clone(),
                })
                .unwrap(),
                funds: vec![]
            },
            0
        )));

        assert!(response
            .messages
            .contains(&SubMsg::reply_never(BankMsg::Send {
                to_address: executor.to_string(),
                amount: create_trigger_info.funds.clone(),
            })));
    }

    #[test]
    fn deletes_trigger_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        let condition = Condition::BlocksCompleted(env.block.height - 10);

        let create_trigger_msg = CreateTriggerMsg {
            condition: condition.clone(),
            msg: Binary::default(),
            contract_address: owner.clone(),
            executors: vec![],
            jitter: None,
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::Create(Box::new(create_trigger_msg.clone())),
        )
        .unwrap();

        let triggers = TRIGGERS
            .filtered(
                deps.as_ref().storage,
                ConditionFilter::Timestamp {
                    start: None,
                    end: None,
                },
                None,
            )
            .unwrap();

        assert!(!triggers.is_empty());

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![create_trigger_msg.id().unwrap()]),
        )
        .unwrap();

        let triggers = TRIGGERS
            .filtered(
                deps.as_ref().storage,
                ConditionFilter::Timestamp {
                    start: None,
                    end: None,
                },
                None,
            )
            .unwrap();

        assert!(triggers.is_empty());
    }
}

#[cfg(test)]
mod filtered_triggers_tests {
    use std::str::FromStr;

    use super::*;

    use calc_rs::{
        conditions::condition::Condition,
        scheduler::{ConditionFilter, Trigger},
    };
    use cosmwasm_std::{
        from_json,
        testing::{mock_dependencies, mock_env},
        Addr, Decimal, Uint64,
    };
    use rujira_rs::fin::Side;

    #[test]
    fn fetches_triggers_with_timestamp_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: Uint64::from(i),
                        contract_address: Addr::unchecked("manager"),
                        msg: Binary::default(),
                        condition: Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10)),
                        execution_rebate: vec![],
                        executors: vec![],
                        jitter: None,
                    },
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::Timestamp {
                        start: Some(env.block.time.plus_seconds(25)),
                        end: Some(env.block.time.plus_seconds(50)),
                    },
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (3..=5)
                .map(|i| Trigger {
                    id: Uint64::from(i),
                    contract_address: Addr::unchecked("manager"),
                    msg: Binary::default(),
                    condition: Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10),),
                    execution_rebate: vec![],
                    executors: vec![],
                    jitter: None
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_timestamp_filter_and_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: Uint64::from(i),
                        condition: Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10)),
                        contract_address: Addr::unchecked("manager"),
                        msg: Binary::default(),
                        execution_rebate: vec![],
                        executors: vec![],
                        jitter: None,
                    },
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::Timestamp {
                        start: Some(env.block.time),
                        end: Some(env.block.time.plus_seconds(50)),
                    },
                    limit: Some(3),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (1..=3)
                .map(|i| Trigger {
                    id: Uint64::from(i),
                    condition: Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10),),
                    contract_address: Addr::unchecked("manager"),
                    msg: Binary::default(),
                    execution_rebate: vec![],
                    executors: vec![],
                    jitter: None
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_block_height_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: Uint64::from(i),
                        contract_address: Addr::unchecked("manager"),
                        msg: Binary::default(),
                        condition: Condition::BlocksCompleted(env.block.height + i * 10),
                        execution_rebate: vec![],
                        executors: vec![],
                        jitter: None,
                    },
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::BlockHeight {
                        start: Some(env.block.height + 25),
                        end: Some(env.block.height + 50),
                    },
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (3..=5)
                .map(|i| Trigger {
                    id: Uint64::from(i),
                    contract_address: Addr::unchecked("manager"),
                    msg: Binary::default(),
                    condition: Condition::BlocksCompleted(env.block.height + i * 10,),
                    execution_rebate: vec![],
                    executors: vec![],
                    jitter: None
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_block_height_filter_and_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: Uint64::from(i),
                        condition: Condition::BlocksCompleted(env.block.height + i * 10),
                        contract_address: Addr::unchecked("manager"),
                        msg: Binary::default(),
                        execution_rebate: vec![],
                        executors: vec![],
                        jitter: None,
                    },
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::BlockHeight {
                        start: Some(env.block.height + 15),
                        end: Some(env.block.height + 50),
                    },
                    limit: Some(3),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (2..=4)
                .map(|i| Trigger {
                    id: Uint64::from(i),
                    condition: Condition::BlocksCompleted(env.block.height + i * 10,),
                    contract_address: Addr::unchecked("manager"),
                    msg: Binary::default(),
                    execution_rebate: vec![],
                    executors: vec![],
                    jitter: None
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_pair_only_limit_order_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=10 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: Uint64::from(i as u64),
                        contract_address: Addr::unchecked("manager"),
                        msg: Binary::default(),
                        condition: Condition::FinLimitOrderFilled {
                            owner: Some(Addr::unchecked("owner")),
                            pair_address: Addr::unchecked(format!("pair-{}", i % 2)),
                            side: Side::Base,
                            price: Decimal::from_str(&i.to_string()).unwrap(),
                        },
                        execution_rebate: vec![],
                        executors: vec![],
                        jitter: None,
                    },
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::LimitOrder {
                        pair_address: Addr::unchecked("pair-0"),
                        price_range: None,
                    },
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        (2..=5)
            .map(|i| {
                let j = i * 2;
                Trigger {
                    id: Uint64::from(j as u64),
                    contract_address: Addr::unchecked("manager"),
                    msg: Binary::default(),
                    condition: Condition::FinLimitOrderFilled {
                        owner: Some(Addr::unchecked("owner")),
                        pair_address: Addr::unchecked("pair-0"),
                        side: Side::Base,
                        price: Decimal::from_str(&j.to_string()).unwrap(),
                    },
                    execution_rebate: vec![],
                    executors: vec![],
                    jitter: None,
                }
            })
            .collect::<Vec<_>>()
            .iter()
            .for_each(|t| assert!(response.contains(t)));
    }

    #[test]
    fn fetches_triggers_with_pair_and_price_range_limit_order_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=10 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: Uint64::from(i as u64),
                        msg: Binary::default(),
                        contract_address: Addr::unchecked("manager"),
                        condition: Condition::FinLimitOrderFilled {
                            owner: Some(Addr::unchecked("owner")),
                            pair_address: Addr::unchecked(format!("pair-{}", i % 2)),
                            side: Side::Base,
                            price: Decimal::from_str(&i.to_string()).unwrap(),
                        },
                        execution_rebate: vec![],
                        executors: vec![],
                        jitter: None,
                    },
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::LimitOrder {
                        pair_address: Addr::unchecked("pair-0"),
                        price_range: Some((
                            Decimal::from_str("7.0").unwrap(),
                            Decimal::from_str("9.0").unwrap(),
                        )),
                    },
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (4..=4)
                .map(|i| {
                    let j = i * 2;
                    Trigger {
                        id: Uint64::from(j as u64),
                        contract_address: Addr::unchecked("manager"),
                        msg: Binary::default(),
                        condition: Condition::FinLimitOrderFilled {
                            owner: Some(Addr::unchecked("owner")),
                            pair_address: Addr::unchecked("pair-0"),
                            side: Side::Base,
                            price: Decimal::from_str(&j.to_string()).unwrap(),
                        },
                        execution_rebate: vec![],
                        executors: vec![],
                        jitter: None,
                    }
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_limit_order_filter_and_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=10 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: Uint64::from(i as u64),
                        contract_address: Addr::unchecked("manager"),
                        msg: Binary::default(),
                        condition: Condition::FinLimitOrderFilled {
                            owner: Some(Addr::unchecked("owner")),
                            pair_address: Addr::unchecked(format!("pair-{}", i % 2)),
                            side: Side::Base,
                            price: Decimal::from_str(&i.to_string()).unwrap(),
                        },
                        execution_rebate: vec![],
                        executors: vec![],
                        jitter: None,
                    },
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::LimitOrder {
                        pair_address: Addr::unchecked("pair-0"),
                        price_range: Some((
                            Decimal::from_str("1.0").unwrap(),
                            Decimal::from_str("9.0").unwrap(),
                        )),
                    },
                    limit: Some(1),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (1..=1)
                .map(|i| {
                    let j = i * 2;
                    Trigger {
                        id: Uint64::from(j as u64),
                        contract_address: Addr::unchecked("manager"),
                        msg: Binary::default(),
                        condition: Condition::FinLimitOrderFilled {
                            owner: Some(Addr::unchecked("owner")),
                            pair_address: Addr::unchecked("pair-0"),
                            side: Side::Base,
                            price: Decimal::from_str(&j.to_string()).unwrap(),
                        },
                        execution_rebate: vec![],
                        executors: vec![],
                        jitter: None,
                    }
                })
                .collect::<Vec<_>>()
        );
    }
}
