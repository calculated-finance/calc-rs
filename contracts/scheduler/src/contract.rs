use std::vec;

use calc_rs::{
    scheduler::{ConditionFilter, SchedulerExecuteMsg, SchedulerQueryMsg, Trigger},
    types::{Contract, ContractError, ContractResult},
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Reply, Response,
    StdError, StdResult, SubMsg,
};

use crate::state::{delete_trigger, fetch_trigger, fetch_triggers, save_trigger, TRIGGER_COUNTER};

const EXECUTE_REPLY_ID: u64 = 1;

#[cw_serde]
pub struct InstantiateMsg {}

#[entry_point]
pub fn instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: InstantiateMsg,
) -> ContractResult {
    TRIGGER_COUNTER.save(_deps.storage, &0)?;
    Ok(Response::default().add_attribute("initialized", "true"))
}

#[cw_serde]
pub struct MigrateMsg {}

#[entry_point]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, StdError> {
    Ok(Response::default().add_attribute("migrated", "true"))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: SchedulerExecuteMsg,
) -> ContractResult {
    match msg {
        SchedulerExecuteMsg::CreateTrigger(command) => {
            let existing_triggers = fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Owner {
                    address: info.sender.clone(),
                },
                None,
                None,
            )?;

            if existing_triggers.len() >= 5 {
                return Err(ContractError::Std(StdError::generic_err(
                    "Cannot have more than 5 active triggers at once",
                )));
            }

            save_trigger(
                deps.storage,
                Trigger::from_command(&info, command, info.funds.clone()),
            )?;

            Ok(Response::default())
        }
        SchedulerExecuteMsg::SetTriggers(commands) => {
            if commands.len() > 5 {
                return Err(ContractError::Std(StdError::generic_err(
                    "Cannot have more than 5 active triggers at once",
                )));
            }

            if !info.funds.is_empty() && info.funds.len() != commands.len() {
                return Err(ContractError::Std(StdError::generic_err(
                    "Must provide either no execution rebates, or 1 execution rebate per trigger",
                )));
            }

            let triggers_to_delete = fetch_triggers(
                deps.as_ref(),
                &env,
                ConditionFilter::Owner {
                    address: info.sender.clone(),
                },
                None,
                None,
            )?;

            let mut rebates_to_refund: Vec<Coin> = vec![];

            for trigger in triggers_to_delete {
                delete_trigger(deps.storage, trigger.id)?;
                rebates_to_refund.extend(trigger.execution_rebate);
            }

            for (i, command) in commands.iter().enumerate() {
                save_trigger(
                    deps.storage,
                    Trigger::from_command(
                        &info,
                        command.clone(),
                        info.funds.get(i).map_or_else(Vec::new, |f| vec![f.clone()]),
                    ),
                )?;
            }

            if rebates_to_refund.is_empty() {
                return Ok(Response::default());
            }

            Ok(Response::default().add_message(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: rebates_to_refund.into(),
            }))
        }
        SchedulerExecuteMsg::ExecuteTrigger(id) => {
            let trigger = fetch_trigger(deps.storage, id)?;

            if !trigger.can_execute(deps.as_ref(), &env)? {
                return Err(ContractError::Std(StdError::generic_err(format!(
                    "Trigger conditions not met: {:?}",
                    trigger.conditions
                ))));
            }

            delete_trigger(deps.storage, id)?;

            // swallow errors from execute trigger replies to prevent
            // hanging triggers due to a misconfigured downstream contract
            let execute_msg = SubMsg::reply_on_error(
                Contract(trigger.to.clone()).call(trigger.msg.clone(), vec![]),
                EXECUTE_REPLY_ID,
            );

            let rebate_msg = BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: trigger.execution_rebate,
            };

            Ok(Response::default()
                .add_submessage(execute_msg)
                .add_message(rebate_msg))
        }
    }
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: SchedulerQueryMsg) -> StdResult<Binary> {
    match msg {
        SchedulerQueryMsg::Triggers {
            filter,
            limit,
            can_execute,
        } => to_json_binary(&fetch_triggers(deps, &env, filter, limit, can_execute)?),
        SchedulerQueryMsg::CanExecute { id } => {
            to_json_binary(&fetch_trigger(deps.storage, id)?.can_execute(deps, &env)?)
        }
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
        conditions: vec![calc_rs::types::Condition::BlocksCompleted(
            cosmwasm_std::testing::mock_env().block.height + 10,
        )],
        threshold: calc_rs::scheduler::TriggerConditionsThreshold::All,
        to: cosmwasm_std::Addr::unchecked("recipient"),
        msg: to_json_binary(&"test message").unwrap(),
    }
}

#[cfg(test)]
mod create_trigger_tests {
    use super::*;
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Coin,
    };

    #[test]
    fn fails_if_too_many_triggers() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let info = message_info(&owner.clone(), &[Coin::new(3123_u128, "rune")]);

        (0..5).into_iter().for_each(|_| {
            execute(
                deps.as_mut(),
                env.clone(),
                info.clone(),
                SchedulerExecuteMsg::CreateTrigger(default_create_trigger_command()),
            )
            .unwrap();
        });

        let create_command = default_create_trigger_command();

        let err = execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            SchedulerExecuteMsg::CreateTrigger(create_command),
        )
        .unwrap_err();

        assert_eq!(
            err,
            ContractError::Std(StdError::generic_err(
                "Cannot have more than 5 active triggers at once"
            ))
        );
    }

    #[test]
    fn creates_trigger() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let info = message_info(&owner.clone(), &[Coin::new(3123_u128, "rune")]);

        let create_command = default_create_trigger_command();

        execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            SchedulerExecuteMsg::CreateTrigger(create_command.clone()),
        )
        .unwrap();

        assert_eq!(TRIGGER_COUNTER.load(deps.as_ref().storage).unwrap(), 1);

        let triggers = fetch_triggers(
            deps.as_ref(),
            &env,
            ConditionFilter::Owner {
                address: owner.clone(),
            },
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            triggers,
            vec![Trigger {
                id: 1,
                owner: owner.clone(),
                conditions: create_command.conditions.clone(),
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
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Coin, SubMsg,
    };

    #[test]
    fn saves_triggers_without_rebates() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
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

        let triggers = fetch_triggers(
            deps.as_ref(),
            &env,
            ConditionFilter::Owner {
                address: owner.clone(),
            },
            None,
            None,
        )
        .unwrap();

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

        let triggers = fetch_triggers(
            deps.as_ref(),
            &env,
            ConditionFilter::Owner {
                address: owner.clone(),
            },
            None,
            None,
        )
        .unwrap();

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

    #[test]
    fn deletes_existing_triggers() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
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

        execute(
            deps.as_mut(),
            env.clone(),
            MessageInfo {
                sender: owner.clone(),
                funds: vec![],
            },
            SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
        )
        .unwrap();

        let triggers = fetch_triggers(
            deps.as_ref(),
            &env,
            ConditionFilter::Owner {
                address: owner.clone(),
            },
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            triggers,
            vec![Trigger {
                id: 3,
                owner: owner,
                conditions: create_command.conditions,
                threshold: create_command.threshold,
                to: create_command.to,
                msg: create_command.msg,
                execution_rebate: vec![],
            }]
        );
    }

    #[test]
    fn refunds_existing_execution_rebates() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
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

        let response = execute(
            deps.as_mut(),
            env.clone(),
            MessageInfo {
                sender: owner.clone(),
                funds: vec![],
            },
            SchedulerExecuteMsg::SetTriggers(vec![]),
        )
        .unwrap();

        assert_eq!(
            response.messages,
            vec![SubMsg::reply_never(BankMsg::Send {
                to_address: owner.to_string(),
                amount: create_trigger_info.funds.clone(),
            })]
        );
    }
}

#[cfg(test)]
mod execute_trigger_tests {
    use super::*;
    use calc_rs::scheduler::CreateTrigger;
    use calc_rs::types::Condition;
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

        assert_eq!(
            err,
            ContractError::Std(StdError::not_found("Trigger with id 1 not found"))
        );
    }

    #[test]
    fn returns_error_if_trigger_cannot_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
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

        assert_eq!(
            err,
            ContractError::Std(StdError::generic_err(format!(
                "Trigger conditions not met: {:?}",
                create_command.conditions
            )))
        );
    }

    #[test]
    fn adds_execute_message_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        let create_command = CreateTrigger {
            conditions: vec![Condition::BlocksCompleted(env.block.height - 10)],
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
        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_command = CreateTrigger {
            conditions: vec![Condition::BlocksCompleted(env.block.height - 10)],
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

        let create_command = CreateTrigger {
            conditions: vec![Condition::BlocksCompleted(env.block.height - 10)],
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

        let triggers = fetch_triggers(
            deps.as_ref(),
            &env,
            ConditionFilter::Owner {
                address: owner.clone(),
            },
            None,
            None,
        )
        .unwrap();

        assert!(triggers.is_empty());
    }
}
