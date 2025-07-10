use std::vec;

use calc_rs::{
    constants::LOG_ERRORS_REPLY_ID,
    core::{Contract, ContractError, ContractResult},
    manager::ManagerExecuteMsg,
    scheduler::{SchedulerExecuteMsg, SchedulerInstantiateMsg, SchedulerQueryMsg},
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError,
    StdResult, SubMsg, SubMsgResult,
};

use crate::state::{MANAGER, TRIGGERS, TRIGGER_COUNTER};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: SchedulerInstantiateMsg,
) -> ContractResult {
    MANAGER.save(_deps.storage, &msg.manager)?;
    TRIGGER_COUNTER.save(_deps.storage, &0)?;

    Ok(Response::default())
}

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, StdError> {
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: SchedulerExecuteMsg,
) -> ContractResult {
    let mut messages = vec![];
    let mut sub_messages = vec![];

    match msg {
        SchedulerExecuteMsg::Create(condition) => {
            TRIGGERS.save(
                deps.storage,
                info.sender.clone(),
                condition,
                info.funds.clone(),
            )?;
        }
        SchedulerExecuteMsg::Execute(ids) => {
            for id in ids {
                let trigger = TRIGGERS.load(deps.storage, id).map_err(|e| {
                    ContractError::generic_err(format!("Failed to load trigger: {e}"))
                })?;

                let check_result = trigger.condition.is_satisfied(deps.as_ref(), &env);

                if let Ok(check_result) = check_result {
                    if !check_result {
                        continue;
                    }
                } else {
                    // The condition itself is invalid, so we
                    // just delete the associated trigger.
                    TRIGGERS.delete(deps.storage, trigger.id)?;
                    continue;
                }

                TRIGGERS.delete(deps.storage, trigger.id)?;

                // swallow errors from execute trigger replies to prevent
                // hanging triggers due to a misconfigured downstream contract.
                let execute_msg = SubMsg::reply_on_error(
                    Contract(MANAGER.load(deps.storage)?).call(
                        to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                            contract_address: trigger.owner,
                        })?,
                        vec![],
                    ),
                    LOG_ERRORS_REPLY_ID,
                );

                sub_messages.push(execute_msg);

                if !trigger.execution_rebate.is_empty() {
                    let rebate_msg = BankMsg::Send {
                        to_address: info.sender.to_string(),
                        amount: trigger.execution_rebate,
                    };

                    messages.push(rebate_msg);
                }
            }
        }
    };

    Ok(Response::default()
        .add_messages(messages)
        .add_submessages(sub_messages))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: SchedulerQueryMsg) -> StdResult<Binary> {
    match msg {
        SchedulerQueryMsg::Owned {
            owner,
            limit,
            start_after,
        } => to_json_binary(&TRIGGERS.owned(deps.storage, owner, limit, start_after)),
        SchedulerQueryMsg::Filtered { filter, limit } => {
            let filtered = TRIGGERS.filtered(deps.storage, filter, limit)?;
            to_json_binary(&filtered)
        }
        SchedulerQueryMsg::CanExecute(id) => to_json_binary(
            &TRIGGERS
                .load(deps.storage, id)?
                .condition
                .is_satisfied(deps, &env)
                .is_ok(),
        ),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, reply: Reply) -> ContractResult {
    match reply.result {
        SubMsgResult::Ok(_) => Ok(Response::default()),
        SubMsgResult::Err(err) => Ok(Response::default()
            .add_attribute("msg_error", err)
            .add_attribute("msg_payload", reply.payload.to_string())
            .add_attribute("reply_id", reply.id.to_string())),
    }
}

#[cfg(test)]
mod create_trigger_tests {
    use super::*;
    use calc_rs::{conditions::Condition, scheduler::Trigger};
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Coin,
    };

    #[test]
    fn creates_trigger_correctly() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let info = message_info(&owner.clone(), &[Coin::new(3123_u128, "rune")]);

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let condition =
            Condition::BlocksCompleted(cosmwasm_std::testing::mock_env().block.height + 10);

        execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        assert_eq!(TRIGGER_COUNTER.load(deps.as_ref().storage).unwrap(), 1);

        let triggers = TRIGGERS.owned(deps.as_ref().storage, owner.clone(), None, None);

        assert_eq!(
            triggers,
            vec![Trigger {
                id: 1,
                owner: owner.clone(),
                condition: condition.clone(),
                execution_rebate: info.funds.clone(),
            }]
        );
    }
}

#[cfg(test)]
mod execute_trigger_tests {
    use super::*;
    use calc_rs::conditions::Condition;
    use cosmwasm_std::testing::message_info;
    use cosmwasm_std::WasmMsg;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Coin, SubMsg,
    };

    #[test]
    fn returns_error_if_trigger_does_not_exist() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let execution_info = MessageInfo {
            sender: deps.api.addr_make("executor"),
            funds: vec![],
        };

        let err = execute(
            deps.as_mut(),
            env.clone(),
            execution_info.clone(),
            SchedulerExecuteMsg::Execute(vec![1]),
        )
        .unwrap_err();

        assert!(err.to_string().contains("Failed to load trigger"));
    }

    #[test]
    fn fails_silently_if_trigger_cannot_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let manager = deps.api.addr_make("creator");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();
        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let condition =
            Condition::BlocksCompleted(cosmwasm_std::testing::mock_env().block.height + 10);

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[Coin::new(327612u128, "rune")]),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![1]),
        )
        .unwrap();

        assert!(response.messages.is_empty());
    }

    #[test]
    fn adds_execute_message_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let manager = deps.api.addr_make("creator");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();
        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        let condition = Condition::BlocksCompleted(env.block.height - 10);

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![1]),
        )
        .unwrap();

        assert!(response.messages.contains(&SubMsg::reply_on_error(
            WasmMsg::Execute {
                contract_addr: manager.to_string(),
                msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                    contract_address: owner.clone(),
                })
                .unwrap(),
                funds: vec![]
            },
            LOG_ERRORS_REPLY_ID
        )));
    }

    #[test]
    fn adds_send_rebate_msg_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let manager = deps.api.addr_make("creator");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();
        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let condition = Condition::BlocksCompleted(env.block.height - 10);

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![1]),
        )
        .unwrap();

        assert!(response.messages.contains(&SubMsg::reply_on_error(
            WasmMsg::Execute {
                contract_addr: manager.to_string(),
                msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                    contract_address: owner.clone(),
                })
                .unwrap(),
                funds: vec![]
            },
            LOG_ERRORS_REPLY_ID
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
        let manager = deps.api.addr_make("manager");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();
        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        let condition = Condition::BlocksCompleted(env.block.height - 10);

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        let triggers = TRIGGERS.owned(deps.as_ref().storage, owner.clone(), None, None);
        assert!(!triggers.is_empty());

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![1]),
        )
        .unwrap();

        let triggers = TRIGGERS.owned(deps.as_ref().storage, owner.clone(), None, None);
        assert!(triggers.is_empty());
    }
}

#[cfg(test)]
mod owned_triggers_tests {
    use calc_rs::{conditions::Condition, scheduler::Trigger};
    use cosmwasm_std::{
        from_json,
        testing::{mock_dependencies, mock_env},
        Addr,
    };

    use super::*;

    #[test]
    fn fetches_owned_triggers() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    Condition::BlocksCompleted(env.block.height),
                    vec![],
                )
                .unwrap();

            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked(format!("other-owner-{i}")),
                    Condition::BlocksCompleted(env.block.height),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Owned {
                    owner: Addr::unchecked("owner"),
                    limit: None,
                    start_after: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (0..5)
                .map(|i| Trigger {
                    id: i * 2 + 1, // odd ids for the owner
                    owner: Addr::unchecked("owner"),
                    condition: Condition::BlocksCompleted(env.block.height),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_owned_triggers_after_start_after() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for _ in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    Condition::BlocksCompleted(env.block.height),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Owned {
                    owner: Addr::unchecked("owner"),
                    limit: None,
                    start_after: Some(2),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (3..=5)
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::BlocksCompleted(env.block.height),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_owned_triggers_with_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for _ in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    Condition::BlocksCompleted(env.block.height),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Owned {
                    owner: Addr::unchecked("owner"),
                    limit: Some(3),
                    start_after: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (1..=3)
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::BlocksCompleted(env.block.height),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_owned_triggers_with_limit_and_start_after() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for _ in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    Condition::BlocksCompleted(env.block.height),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Owned {
                    owner: Addr::unchecked("owner"),
                    limit: Some(3),
                    start_after: Some(1),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (2..=4)
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::BlocksCompleted(env.block.height),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }
}

#[cfg(test)]
mod filtered_triggers_tests {
    use std::str::FromStr;

    use super::*;

    use calc_rs::{
        conditions::Condition,
        scheduler::{ConditionFilter, Trigger},
    };
    use cosmwasm_std::{
        from_json,
        testing::{mock_dependencies, mock_env},
        Addr, Decimal,
    };
    use rujira_rs::fin::{Price, Side};

    #[test]
    fn fetches_triggers_with_timestamp_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10)),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::Timestamp {
                        start: Some(env.block.time.plus_seconds(20)),
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
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10),),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_timestamp_filter_and_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10)),
                    vec![],
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
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10),),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_block_height_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    Condition::BlocksCompleted(env.block.height + i * 10),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::BlockHeight {
                        start: Some(env.block.height + 20),
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
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::BlocksCompleted(env.block.height + i * 10,),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_block_height_filter_and_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    Condition::BlocksCompleted(env.block.height + i * 10),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::BlockHeight {
                        start: Some(env.block.height + 10),
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
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::BlocksCompleted(env.block.height + i * 10,),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_pair_only_limit_order_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=10 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    Condition::LimitOrderFilled {
                        pair_address: Addr::unchecked(format!("pair-{}", i % 2)),
                        owner: Addr::unchecked("owner"),
                        side: Side::Base,
                        price: Price::Fixed(Decimal::from_str(&i.to_string()).unwrap()),
                        minimum_filled_amount: None,
                    },
                    vec![],
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
                        start_after: Some(4),
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
                .map(|i| {
                    let j = i * 2;
                    Trigger {
                        id: j,
                        owner: Addr::unchecked("owner"),
                        condition: Condition::LimitOrderFilled {
                            pair_address: Addr::unchecked("pair-0"),
                            owner: Addr::unchecked("owner"),
                            side: Side::Base,
                            price: Price::Fixed(Decimal::from_str(&j.to_string()).unwrap()),
                            minimum_filled_amount: None,
                        },
                        execution_rebate: vec![],
                    }
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_pair_and_price_range_limit_order_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=10 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    Condition::LimitOrderFilled {
                        pair_address: Addr::unchecked(format!("pair-{}", i % 2)),
                        owner: Addr::unchecked("owner"),
                        side: Side::Base,
                        price: Price::Fixed(Decimal::from_str(&i.to_string()).unwrap()),
                        minimum_filled_amount: None,
                    },
                    vec![],
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
                        start_after: None,
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
                        id: j,
                        owner: Addr::unchecked("owner"),
                        condition: Condition::LimitOrderFilled {
                            pair_address: Addr::unchecked("pair-0"),
                            owner: Addr::unchecked("owner"),
                            side: Side::Base,
                            price: Price::Fixed(Decimal::from_str(&j.to_string()).unwrap()),
                            minimum_filled_amount: None,
                        },
                        execution_rebate: vec![],
                    }
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_pair_and_price_range_and_start_after_limit_order_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=10 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    Condition::LimitOrderFilled {
                        pair_address: Addr::unchecked(format!("pair-{}", i % 2)),
                        owner: Addr::unchecked("owner"),
                        side: Side::Base,
                        price: Price::Fixed(Decimal::from_str(&i.to_string()).unwrap()),
                        minimum_filled_amount: None,
                    },
                    vec![],
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
                        start_after: Some(4),
                    },
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (1..=4)
                .map(|i| {
                    let j = i * 2;
                    Trigger {
                        id: j,
                        owner: Addr::unchecked("owner"),
                        condition: Condition::LimitOrderFilled {
                            pair_address: Addr::unchecked("pair-0"),
                            owner: Addr::unchecked("owner"),
                            side: Side::Base,
                            price: Price::Fixed(Decimal::from_str(&j.to_string()).unwrap()),
                            minimum_filled_amount: None,
                        },
                        execution_rebate: vec![],
                    }
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_limit_order_filter_and_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=10 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    Condition::LimitOrderFilled {
                        pair_address: Addr::unchecked(format!("pair-{}", i % 2)),
                        owner: Addr::unchecked("owner"),
                        side: Side::Base,
                        price: Price::Fixed(Decimal::from_str(&i.to_string()).unwrap()),
                        minimum_filled_amount: None,
                    },
                    vec![],
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
                        start_after: None,
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
                        id: j,
                        owner: Addr::unchecked("owner"),
                        condition: Condition::LimitOrderFilled {
                            pair_address: Addr::unchecked("pair-0"),
                            owner: Addr::unchecked("owner"),
                            side: Side::Base,
                            price: Price::Fixed(Decimal::from_str(&j.to_string()).unwrap()),
                            minimum_filled_amount: None,
                        },
                        execution_rebate: vec![],
                    }
                })
                .collect::<Vec<_>>()
        );
    }
}
