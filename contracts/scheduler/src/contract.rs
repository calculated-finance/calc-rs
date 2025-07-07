use std::vec;

use calc_rs::{
    conditions::Satisfiable,
    core::{Contract, ContractError, ContractResult},
    scheduler::{SchedulerExecuteMsg, SchedulerInstantiateMsg, SchedulerQueryMsg},
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError,
    StdResult, SubMsg,
};

use crate::state::{CONDITION_COUNTER, TRIGGERS, TRIGGER_COUNTER};

const EXECUTE_REPLY_ID: u64 = 1;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: SchedulerInstantiateMsg,
) -> ContractResult {
    TRIGGER_COUNTER.save(_deps.storage, &0)?;
    CONDITION_COUNTER.save(_deps.storage, &0)?;

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
        SchedulerExecuteMsg::CreateTrigger(command) => {
            TRIGGERS.save(
                deps.storage,
                info.sender.clone(),
                command,
                info.funds.clone(),
            )?;
        }
        SchedulerExecuteMsg::SetTriggers(commands) => {
            if !info.funds.is_empty() && info.funds.len() != commands.len() {
                return Err(ContractError::Std(StdError::generic_err(
                    "Must provide either no execution rebates, or 1 execution rebate per trigger",
                )));
            }

            for (i, command) in commands.into_iter().enumerate() {
                TRIGGERS.save(
                    deps.storage,
                    info.sender.clone(),
                    command,
                    info.funds.get(i).map_or_else(Vec::new, |f| vec![f.clone()]),
                )?;
            }
        }
        SchedulerExecuteMsg::ExecuteTrigger(id) => {
            let trigger = TRIGGERS
                .load(deps.storage, id)
                .map_err(|e| ContractError::generic_err(format!("Failed to load trigger: {e}")))?;

            if !trigger.condition.is_satisfied(deps.as_ref(), &env) {
                return Err(ContractError::generic_err(format!(
                    "Trigger condition not met: {:?}",
                    trigger.condition
                )));
            }

            TRIGGERS.delete(deps.storage, trigger.id)?;

            // swallow errors from execute trigger replies to prevent
            // hanging triggers due to a misconfigured downstream contract
            let execute_msg = SubMsg::reply_on_error(
                Contract(trigger.to.clone()).call(trigger.msg.clone(), vec![]),
                EXECUTE_REPLY_ID,
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
        } => to_json_binary(&TRIGGERS.owner(deps.storage, owner, limit, start_after)),
        SchedulerQueryMsg::Filtered { filter, limit } => {
            to_json_binary(&TRIGGERS.filter(deps.storage, filter, limit)?)
        }
        SchedulerQueryMsg::CanExecute { id } => to_json_binary(
            &TRIGGERS
                .load(deps.storage, id)?
                .condition
                .is_satisfied(deps, &env),
        ),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, _reply: Reply) -> ContractResult {
    // swallow errors from execute trigger replies to prevent
    // hanging triggers due to a misconfigured downstream contract
    Ok(Response::default())
}

#[cfg(test)]
fn default_create_trigger_command() -> calc_rs::scheduler::CreateTrigger {
    calc_rs::scheduler::CreateTrigger {
        condition: calc_rs::conditions::Condition::BlocksCompleted(
            cosmwasm_std::testing::mock_env().block.height + 10,
        ),
        threshold: calc_rs::conditions::Threshold::All,
        to: cosmwasm_std::Addr::unchecked("recipient"),
        msg: to_json_binary(&"test message").unwrap(),
    }
}

#[cfg(test)]
mod create_trigger_tests {
    use super::*;
    use calc_rs::scheduler::Trigger;
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Coin,
    };

    #[test]
    fn creates_trigger() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let info = message_info(&owner.clone(), &[Coin::new(3123_u128, "rune")]);

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_command = default_create_trigger_command();

        execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            SchedulerExecuteMsg::CreateTrigger(create_command.clone()),
        )
        .unwrap();

        assert_eq!(TRIGGER_COUNTER.load(deps.as_ref().storage).unwrap(), 1);

        let triggers = TRIGGERS.owner(deps.as_ref().storage, owner.clone(), None, None);

        assert_eq!(
            triggers,
            vec![Trigger {
                id: 1,
                owner: owner.clone(),
                condition: create_command.condition.clone(),
                threshold: create_command.threshold.clone(),
                to: create_command.to.clone(),
                msg: create_command.msg.clone(),
                execution_rebate: info.funds.clone(),
            }]
        );
    }
}

#[cfg(test)]
mod set_triggers_tests {
    use std::vec;

    use super::*;
    use calc_rs::{
        conditions::{Condition, Conditions, Threshold},
        scheduler::{ConditionFilter, CreateTrigger, Trigger},
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr, Coin, Decimal, Timestamp,
    };
    use rujira_rs::fin::{Price, Side};

    #[test]
    fn saves_trigger_with_multiple_conditions() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_trigger_info = message_info(&owner.clone(), &[]);

        let create_command = CreateTrigger {
            condition: Condition::Compose(Conditions {
                conditions: vec![
                    Condition::BlocksCompleted(env.block.height + 10),
                    Condition::TimestampElapsed(Timestamp::from_seconds(
                        env.block.time.seconds() + 10,
                    )),
                    Condition::LimitOrderFilled {
                        pair_address: Addr::unchecked("pair_address"),
                        owner: Addr::unchecked("owner"),
                        side: Side::Base,
                        price: Price::Fixed(Decimal::one()),
                    },
                    Condition::BalanceAvailable {
                        address: env.contract.address.clone(),
                        amount: Coin::new(1000_u128, "rune"),
                    },
                ],
                threshold: Threshold::All,
            }),
            ..default_create_trigger_command()
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
        )
        .unwrap();

        let trigger = Trigger {
            id: 1,
            ..Trigger::from_command(&create_trigger_info, create_command.clone(), vec![])
        };

        assert_eq!(
            TRIGGERS.owner(deps.as_ref().storage, owner.clone(), None, None),
            vec![trigger.clone()]
        );

        assert_eq!(
            TRIGGERS
                .filter(
                    deps.as_ref().storage,
                    ConditionFilter::BlockHeight {
                        start: Some(env.block.height + 5),
                        end: Some(env.block.height + 15)
                    },
                    None
                )
                .unwrap(),
            vec![trigger.clone()]
        );

        assert_eq!(
            TRIGGERS
                .filter(
                    deps.as_ref().storage,
                    ConditionFilter::Timestamp {
                        start: Some(env.block.time.plus_seconds(5)),
                        end: Some(env.block.time.plus_seconds(15))
                    },
                    None
                )
                .unwrap(),
            vec![trigger.clone()]
        );

        assert_eq!(
            TRIGGERS
                .filter(
                    deps.as_ref().storage,
                    ConditionFilter::LimitOrder { start_after: None },
                    None
                )
                .unwrap(),
            vec![trigger.clone()]
        );
    }

    #[test]
    fn saves_triggers_without_rebates() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_trigger_info = message_info(&owner.clone(), &[]);

        let create_command = default_create_trigger_command();
        let create_commands = vec![create_command.clone(), create_command.clone()];

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::SetTriggers(create_commands.clone()),
        )
        .unwrap();

        assert_eq!(TRIGGER_COUNTER.load(deps.as_ref().storage).unwrap(), 2);

        let triggers = TRIGGERS.owner(deps.as_ref().storage, owner.clone(), None, None);

        assert_eq!(
            triggers,
            vec![
                Trigger {
                    id: 1,
                    ..Trigger::from_command(
                        &create_trigger_info,
                        create_commands[0].clone(),
                        vec![]
                    )
                },
                Trigger {
                    id: 2,
                    ..Trigger::from_command(
                        &create_trigger_info,
                        create_commands[1].clone(),
                        vec![]
                    )
                },
            ]
        );
    }

    #[test]
    fn saves_triggers_with_rebates() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_trigger_info = message_info(
            &owner.clone(),
            &[Coin::new(1000_u128, "rune"), Coin::new(2000_u128, "uruji")],
        );

        let create_command = default_create_trigger_command();
        let create_commands = vec![create_command.clone(), create_command.clone()];

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::SetTriggers(create_commands.clone()),
        )
        .unwrap();

        assert_eq!(TRIGGER_COUNTER.load(deps.as_ref().storage).unwrap(), 2);

        let triggers = TRIGGERS.owner(deps.as_ref().storage, owner.clone(), None, None);

        assert_eq!(
            triggers,
            vec![
                Trigger {
                    id: 1,
                    ..Trigger::from_command(
                        &create_trigger_info,
                        create_commands[0].clone(),
                        vec![create_trigger_info.funds[0].clone()]
                    )
                },
                Trigger {
                    id: 2,
                    ..Trigger::from_command(
                        &create_trigger_info,
                        create_commands[1].clone(),
                        vec![create_trigger_info.funds[1].clone()]
                    )
                },
            ]
        );
    }

    // #[test]
    // fn deletes_existing_triggers() {
    //     let mut deps = mock_dependencies();
    //     let env = mock_env();
    //     let owner = deps.api.addr_make("creator");

    //     TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
    //     CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

    //     let create_trigger_info = message_info(
    //         &owner.clone(),
    //         &[Coin::new(1000_u128, "rune"), Coin::new(2000_u128, "uruji")],
    //     );

    //     let create_command = default_create_trigger_command();
    //     let create_commands = vec![create_command.clone(), create_command.clone()];

    //     execute(
    //         deps.as_mut(),
    //         env.clone(),
    //         create_trigger_info.clone(),
    //         SchedulerExecuteMsg::SetTriggers(create_commands.clone()),
    //     )
    //     .unwrap();

    //     assert_eq!(TRIGGER_COUNTER.load(deps.as_ref().storage).unwrap(), 2);

    //     execute(
    //         deps.as_mut(),
    //         env.clone(),
    //         MessageInfo {
    //             sender: owner.clone(),
    //             funds: vec![],
    //         },
    //         SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
    //     )
    //     .unwrap();

    //     let triggers = TRIGGERS.owner(deps.as_ref().storage, owner.clone(), None, None);

    //     assert_eq!(
    //         triggers,
    //         vec![Trigger {
    //             id: 3,
    //             owner,
    //             condition: create_command.condition,
    //             threshold: create_command.threshold,
    //             to: create_command.to,
    //             msg: create_command.msg,
    //             execution_rebate: vec![],
    //         }]
    //     );
    // }

    // #[test]
    // fn refunds_existing_execution_rebates() {
    //     let mut deps = mock_dependencies();
    //     let env = mock_env();
    //     let owner = deps.api.addr_make("creator");

    //     TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
    //     CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

    //     let create_trigger_info = message_info(
    //         &owner.clone(),
    //         &[Coin::new(1000_u128, "rune"), Coin::new(2000_u128, "uruji")],
    //     );

    //     let create_command = default_create_trigger_command();
    //     let create_commands = vec![create_command.clone(), create_command.clone()];

    //     execute(
    //         deps.as_mut(),
    //         env.clone(),
    //         create_trigger_info.clone(),
    //         SchedulerExecuteMsg::SetTriggers(create_commands.clone()),
    //     )
    //     .unwrap();

    //     let response = execute(
    //         deps.as_mut(),
    //         env.clone(),
    //         MessageInfo {
    //             sender: owner.clone(),
    //             funds: vec![],
    //         },
    //         SchedulerExecuteMsg::SetTriggers(vec![]),
    //     )
    //     .unwrap();

    //     assert_eq!(
    //         response.messages,
    //         vec![SubMsg::reply_never(BankMsg::Send {
    //             to_address: owner.to_string(),
    //             amount: create_trigger_info.funds.clone(),
    //         })]
    //     );
    // }
}

#[cfg(test)]
mod execute_trigger_tests {
    use super::*;
    use calc_rs::conditions::Condition;
    use calc_rs::constants::LOG_ERRORS_REPLY_ID;
    use calc_rs::scheduler::CreateTrigger;
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
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let execution_info = MessageInfo {
            sender: deps.api.addr_make("executor"),
            funds: vec![],
        };

        let err = execute(
            deps.as_mut(),
            env.clone(),
            execution_info.clone(),
            SchedulerExecuteMsg::ExecuteTrigger(1),
        )
        .unwrap_err();

        assert!(err.to_string().contains("Failed to load trigger"));
    }

    #[test]
    fn returns_error_if_trigger_cannot_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_command = default_create_trigger_command();

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[Coin::new(327612u128, "rune")]),
            SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::ExecuteTrigger(1),
        )
        .unwrap_err();

        assert!(err.to_string().contains("Trigger condition not met"));
    }

    #[test]
    fn adds_execute_message_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        let create_command = CreateTrigger {
            condition: Condition::BlocksCompleted(env.block.height - 10),
            ..default_create_trigger_command()
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
        )
        .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::ExecuteTrigger(1),
        )
        .unwrap();

        assert!(response.messages.contains(&SubMsg::reply_on_error(
            WasmMsg::Execute {
                contract_addr: create_command.to.to_string(),
                msg: create_command.msg,
                funds: vec![]
            },
            EXECUTE_REPLY_ID
        )));
    }

    #[test]
    fn adds_send_rebate_msg_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_command = CreateTrigger {
            condition: Condition::BlocksCompleted(env.block.height - 10),
            ..default_create_trigger_command()
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
        )
        .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::ExecuteTrigger(1),
        )
        .unwrap();

        assert!(response.messages.contains(&SubMsg::reply_on_error(
            WasmMsg::Execute {
                contract_addr: create_command.to.to_string(),
                msg: create_command.msg,
                funds: vec![]
            },
            EXECUTE_REPLY_ID
        )));

        assert!(response.messages.contains(&SubMsg::reply_always(
            BankMsg::Send {
                to_address: executor.to_string(),
                amount: create_trigger_info.funds.clone(),
            },
            LOG_ERRORS_REPLY_ID
        )));
    }

    #[test]
    fn deletes_trigger_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        let create_command = CreateTrigger {
            condition: Condition::BlocksCompleted(env.block.height - 10),
            ..default_create_trigger_command()
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
        )
        .unwrap();

        let triggers = TRIGGERS.owner(deps.as_ref().storage, owner.clone(), None, None);
        assert!(!triggers.is_empty());

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::ExecuteTrigger(1),
        )
        .unwrap();

        let triggers = TRIGGERS.owner(deps.as_ref().storage, owner.clone(), None, None);
        assert!(triggers.is_empty());
    }
}

#[cfg(test)]
mod owned_triggers_tests {
    use calc_rs::{
        conditions::{Condition, Threshold},
        scheduler::{CreateTrigger, Trigger},
    };
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
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    CreateTrigger {
                        condition: Condition::BlocksCompleted(env.block.height),
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                    },
                    vec![],
                )
                .unwrap();

            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked(format!("other-owner-{i}")),
                    CreateTrigger {
                        condition: Condition::BlocksCompleted(env.block.height),
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                    },
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
                    threshold: Threshold::All,
                    to: Addr::unchecked("recipient"),
                    msg: to_json_binary(&"test message").unwrap(),
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
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for _ in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    CreateTrigger {
                        condition: Condition::BlocksCompleted(env.block.height),
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                    },
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
                    threshold: Threshold::All,
                    to: Addr::unchecked("recipient"),
                    msg: to_json_binary(&"test message").unwrap(),
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
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for _ in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    CreateTrigger {
                        condition: Condition::BlocksCompleted(env.block.height),
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                    },
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
                    threshold: Threshold::All,
                    to: Addr::unchecked("recipient"),
                    msg: to_json_binary(&"test message").unwrap(),
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
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for _ in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    CreateTrigger {
                        condition: Condition::BlocksCompleted(env.block.height),
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                    },
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
                    threshold: Threshold::All,
                    to: Addr::unchecked("recipient"),
                    msg: to_json_binary(&"test message").unwrap(),
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
        conditions::{Condition, Threshold},
        scheduler::{ConditionFilter, CreateTrigger, Trigger},
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
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    CreateTrigger {
                        condition: Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10)),
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
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
                    threshold: Threshold::All,
                    to: Addr::unchecked("recipient"),
                    msg: to_json_binary(&"test message").unwrap(),
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
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    CreateTrigger {
                        condition: Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10)),
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
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
                    threshold: Threshold::All,
                    to: Addr::unchecked("recipient"),
                    msg: to_json_binary(&"test message").unwrap(),
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
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    CreateTrigger {
                        condition: Condition::BlocksCompleted(env.block.height + i * 10),
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
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
                    threshold: Threshold::All,
                    to: Addr::unchecked("recipient"),
                    msg: to_json_binary(&"test message").unwrap(),
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
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    CreateTrigger {
                        condition: Condition::BlocksCompleted(env.block.height + i * 10),
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
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
                    threshold: Threshold::All,
                    to: Addr::unchecked("recipient"),
                    msg: to_json_binary(&"test message").unwrap(),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_limit_order_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    CreateTrigger {
                        condition: Condition::LimitOrderFilled {
                            pair_address: Addr::unchecked("pair"),
                            owner: Addr::unchecked("owner"),
                            side: Side::Base,
                            price: Price::Fixed(Decimal::from_str(&i.to_string()).unwrap()),
                        },
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
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
                        start_after: Some(2),
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
                    Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        condition: Condition::LimitOrderFilled {
                            pair_address: Addr::unchecked("pair"),
                            owner: Addr::unchecked("owner"),
                            side: Side::Base,
                            price: Price::Fixed(Decimal::from_str(&i.to_string()).unwrap()),
                        },
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
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
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        for i in 1..=10 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    Addr::unchecked("owner"),
                    CreateTrigger {
                        condition: Condition::LimitOrderFilled {
                            pair_address: Addr::unchecked("pair"),
                            owner: Addr::unchecked("owner"),
                            side: Side::Base,
                            price: Price::Fixed(Decimal::from_str(&i.to_string()).unwrap()),
                        },
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
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
                        start_after: Some(2),
                    },
                    limit: Some(5),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (3..=7)
                .map(|i| {
                    Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        condition: Condition::LimitOrderFilled {
                            pair_address: Addr::unchecked("pair"),
                            owner: Addr::unchecked("owner"),
                            side: Side::Base,
                            price: Price::Fixed(Decimal::from_str(&i.to_string()).unwrap()),
                        },
                        threshold: Threshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
                    }
                })
                .collect::<Vec<_>>()
        );
    }
}
