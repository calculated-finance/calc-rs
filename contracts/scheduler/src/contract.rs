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
    to_json_binary, BankMsg, Binary, Coins, Deps, DepsMut, Env, MessageInfo, Reply, Response,
    StdResult, SubMsg, SubMsgResult,
};

use crate::state::TRIGGERS;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: SchedulerInstantiateMsg,
) -> ContractResult {
    Ok(Response::new())
}

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> ContractResult {
    Ok(Response::new())
}

const MAX_EXECUTORS: usize = 10;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: SchedulerExecuteMsg,
) -> ContractResult {
    match msg {
        SchedulerExecuteMsg::Create(create_command) => {
            if create_command.executors.len() > MAX_EXECUTORS {
                return Err(ContractError::generic_err(format!(
                    "Cannot specify more than {MAX_EXECUTORS} executors"
                )));
            }

            match create_command.condition {
                Condition::BlocksCompleted(_) | Condition::TimestampElapsed(_) => {}
                _ => {
                    return Err(ContractError::generic_err(format!(
                        "Unsupported condition type for trigger: {:#?}",
                        create_command.condition
                    )));
                }
            }

            let mut sub_messages = Vec::with_capacity(2);
            let trigger_id = create_command.id(&info.sender)?;

            if let Ok(existing_trigger) = TRIGGERS.load(deps.storage, trigger_id) {
                if info.sender != existing_trigger.owner {
                    return Err(ContractError::generic_err(
                        "Only the owner can update an existing trigger",
                    ));
                }

                TRIGGERS.delete(deps.storage, existing_trigger.id.into())?;

                if !existing_trigger.execution_rebate.is_empty() {
                    sub_messages.push(SubMsg::reply_never(BankMsg::Send {
                        to_address: existing_trigger.owner.to_string(),
                        amount: existing_trigger.execution_rebate,
                    }));
                }
            }

            TRIGGERS.save(
                deps.storage,
                &Trigger {
                    id: trigger_id,
                    owner: info.sender,
                    condition: create_command.condition,
                    msg: create_command.msg,
                    contract_address: create_command.contract_address,
                    executors: create_command.executors,
                    execution_rebate: Coins::try_from(info.funds)?.to_vec(),
                    jitter: create_command.jitter,
                },
            )?;

            Ok(Response::new().add_submessages(sub_messages))
        }
        SchedulerExecuteMsg::Execute(ids) => {
            let mut sub_messages = Vec::with_capacity(ids.len() * 2);

            for id in ids {
                let trigger = match TRIGGERS.load(deps.storage, id) {
                    Ok(trigger) => trigger,
                    Err(_) => continue,
                };

                if !trigger.executors.is_empty() && !trigger.executors.contains(&info.sender) {
                    continue;
                }

                match trigger.condition.is_satisfied(deps.as_ref(), &env) {
                    Ok(true) => {}
                    _ => continue,
                }

                TRIGGERS.delete(deps.storage, trigger.id.into())?;

                let execute_trigger_msg = SubMsg::reply_on_error(
                    Contract(trigger.contract_address).call(trigger.msg, vec![]),
                    0,
                );

                sub_messages.push(execute_trigger_msg);

                if !trigger.execution_rebate.is_empty() {
                    let rebate_msg = SubMsg::reply_never(BankMsg::Send {
                        to_address: info.sender.to_string(),
                        amount: trigger.execution_rebate,
                    });

                    sub_messages.push(rebate_msg);
                }
            }

            Ok(Response::new().add_submessages(sub_messages))
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
        SubMsgResult::Ok(_) => Ok(Response::new()),
        SubMsgResult::Err(err) => Ok(Response::new().add_attribute("msg_error", err)),
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
        Addr, Coin,
    };

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
                id: create_trigger_msg.id(&owner).unwrap(),
                owner: owner.clone(),
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
                id: create_trigger_msg.id(&owner).unwrap(),
                owner: owner.clone(),
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
                id: create_trigger_msg.id(&owner).unwrap(),
                owner: owner.clone(),
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
    fn cannot_overwrite_trigger_with_different_owner() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let caller = deps.api.addr_make("caller");
        let owner = deps.api.addr_make("owner");
        let info = message_info(&caller, &[]);

        let create_trigger_msg = CreateTriggerMsg {
            contract_address: Addr::unchecked("manager"),
            msg: Binary::default(),
            condition: Condition::BlocksCompleted(100),
            executors: vec![],
            jitter: None,
        };

        let id = create_trigger_msg.id(&caller).unwrap();

        TRIGGERS
            .save(
                deps.as_mut().storage,
                &Trigger {
                    id,
                    owner,
                    contract_address: create_trigger_msg.contract_address.clone(),
                    msg: create_trigger_msg.msg.clone(),
                    condition: create_trigger_msg.condition.clone(),
                    execution_rebate: vec![],
                    executors: create_trigger_msg.executors.clone(),
                    jitter: create_trigger_msg.jitter,
                },
            )
            .unwrap();

        let err = execute(
            deps.as_mut(),
            env,
            info,
            SchedulerExecuteMsg::Create(Box::new(create_trigger_msg)),
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("Only the owner can update an existing trigger"));
    }
}

#[cfg(test)]
mod execute_trigger_tests {
    use super::*;
    use calc_rs::conditions::condition::Condition;
    use calc_rs::manager::ManagerExecuteMsg;
    use calc_rs::scheduler::{ConditionFilter, CreateTriggerMsg};
    use cosmwasm_std::testing::message_info;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Coin, SubMsg,
    };
    use cosmwasm_std::{Uint64, WasmMsg};

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
            SchedulerExecuteMsg::Execute(vec![create_trigger_msg.id(&owner).unwrap()]),
        )
        .unwrap();

        assert!(response.messages.is_empty());
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
            SchedulerExecuteMsg::Execute(vec![create_trigger_msg.id(&owner).unwrap()]),
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
            SchedulerExecuteMsg::Execute(vec![create_trigger_msg.id(&owner).unwrap()]),
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
            SchedulerExecuteMsg::Execute(vec![create_trigger_msg.id(&owner).unwrap()]),
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
    use super::*;

    use calc_rs::{
        conditions::condition::Condition,
        scheduler::{ConditionFilter, Trigger},
    };
    use cosmwasm_std::{
        from_json,
        testing::{mock_dependencies, mock_env},
        Addr, Uint64,
    };

    #[test]
    fn fetches_triggers_with_timestamp_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: Uint64::from(i),
                        owner: owner.clone(),
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
                    owner: owner.clone(),
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
        let owner = deps.api.addr_make("creator");

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: Uint64::from(i),
                        owner: owner.clone(),
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
                    owner: owner.clone(),
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
        let owner = deps.api.addr_make("creator");

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: Uint64::from(i),
                        owner: owner.clone(),
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
                    owner: owner.clone(),
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
        let owner = deps.api.addr_make("creator");

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: Uint64::from(i),
                        owner: owner.clone(),
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
                    owner: owner.clone(),
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
}
