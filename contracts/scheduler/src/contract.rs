use calc_rs::types::{
    ConditionFilter, ContractError, ContractResult, SchedulerExecuteMsg, SchedulerQueryMsg, Trigger,
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Coins, Deps, DepsMut, Env, MessageInfo, Response, StdError,
    StdResult,
};

use crate::state::{fetch_triggers, triggers, TRIGGER_COUNTER};
use crate::types::Executable;

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
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, StdError> {
    if TRIGGER_COUNTER.load(deps.storage).is_err() {
        TRIGGER_COUNTER.save(deps.storage, &0)?;
    }
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
        SchedulerExecuteMsg::CreateTrigger(trigger) => {
            let id = TRIGGER_COUNTER.update(deps.storage, |id| Ok::<u64, StdError>(id + 1))?;

            triggers().save(
                deps.storage,
                id,
                &Trigger {
                    id,
                    owner: info.sender.clone(),
                    condition: trigger.condition,
                    msg: trigger.msg,
                    to: trigger.to,
                    execution_rebate: info.funds.clone(),
                },
            )?;

            Ok(Response::default().add_attribute("trigger_id", id.to_string()))
        }
        SchedulerExecuteMsg::SetTriggers(triggers_to_create) => {
            if !info.funds.is_empty() && info.funds.len() != triggers_to_create.len() {
                return Err(ContractError::Std(StdError::generic_err(
                    "Number of funds must match number of triggers to create",
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

            let mut rebates_to_refund = Coins::default();

            for trigger in triggers_to_delete {
                triggers().remove(deps.storage, trigger.id)?;

                for coin in &trigger.execution_rebate {
                    rebates_to_refund.add(coin.clone())?;
                }
            }

            for (i, trigger_to_create) in triggers_to_create.iter().enumerate() {
                let id = TRIGGER_COUNTER.update(deps.storage, |id| Ok::<u64, StdError>(id + 1))?;

                triggers().save(
                    deps.storage,
                    id,
                    &Trigger {
                        id,
                        owner: info.sender.clone(),
                        condition: trigger_to_create.condition.clone(),
                        msg: trigger_to_create.msg.clone(),
                        to: trigger_to_create.to.clone(),
                        execution_rebate: info
                            .funds
                            .get(i)
                            .map_or_else(|| vec![], |rebate| vec![rebate.clone()]),
                    },
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
            let trigger = triggers().load(deps.storage, id).map_err(|_| {
                ContractError::Std(StdError::generic_err(format!(
                    "Trigger with id {} does not exist",
                    id
                )))
            })?;

            let response = trigger.execute(&env)?;

            triggers().remove(deps.storage, id)?;

            Ok(response.add_message(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: trigger.execution_rebate,
            }))
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
            to_json_binary(&triggers().load(deps.storage, id)?.can_execute(&env))
        }
    }
}

#[cfg(test)]
mod create_trigger_tests {
    use super::*;
    use calc_rs::types::{Condition, CreateTrigger};
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Coin, Uint128,
    };

    #[test]
    fn creates_trigger() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        let create_trigger_info = MessageInfo {
            sender: owner.clone(),
            funds: vec![Coin {
                denom: "rune".to_string(),
                amount: Uint128::new(235463),
            }],
        };

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_command = CreateTrigger {
            condition: Condition::BlockHeight {
                height: env.block.height + 10,
            },
            to: deps.api.addr_make("recipient"),
            msg: to_json_binary(&"test message").unwrap(),
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
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
                condition: create_command.condition.clone(),
                to: create_command.to.clone(),
                msg: create_command.msg.clone(),
                execution_rebate: create_trigger_info.funds.clone(),
            }]
        );
    }
}

#[cfg(test)]
mod set_triggers_tests {
    use super::*;
    use calc_rs::types::{Condition, CreateTrigger};
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Coin, SubMsg, Uint128,
    };

    #[test]
    fn saves_triggers_without_rebates() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        let create_trigger_info = MessageInfo {
            sender: owner.clone(),
            funds: vec![],
        };

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_commands = vec![
            CreateTrigger {
                condition: Condition::BlockHeight {
                    height: env.block.height + 10,
                },
                to: deps.api.addr_make("recipient1"),
                msg: to_json_binary(&"test message 1").unwrap(),
            },
            CreateTrigger {
                condition: Condition::Timestamp {
                    timestamp: env.block.time.plus_seconds(100),
                },
                to: deps.api.addr_make("recipient2"),
                msg: to_json_binary(&"test message 2").unwrap(),
            },
        ];

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
                    owner: owner.clone(),
                    condition: create_commands[0].condition.clone(),
                    to: create_commands[0].to.clone(),
                    msg: create_commands[0].msg.clone(),
                    execution_rebate: vec![],
                },
                Trigger {
                    id: 2,
                    owner: owner.clone(),
                    condition: create_commands[1].condition.clone(),
                    to: create_commands[1].to.clone(),
                    msg: create_commands[1].msg.clone(),
                    execution_rebate: vec![],
                },
            ]
        );
    }

    #[test]
    fn saves_triggers_with_rebates() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        let create_trigger_info = MessageInfo {
            sender: owner.clone(),
            funds: vec![
                Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(235463),
                },
                Coin {
                    denom: "uruji".to_string(),
                    amount: Uint128::new(542365),
                },
            ],
        };

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_commands = vec![
            CreateTrigger {
                condition: Condition::BlockHeight {
                    height: env.block.height + 10,
                },
                to: deps.api.addr_make("recipient1"),
                msg: to_json_binary(&"test message 1").unwrap(),
            },
            CreateTrigger {
                condition: Condition::Timestamp {
                    timestamp: env.block.time.plus_seconds(100),
                },
                to: deps.api.addr_make("recipient2"),
                msg: to_json_binary(&"test message 2").unwrap(),
            },
        ];

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
                    owner: owner.clone(),
                    condition: create_commands[0].condition.clone(),
                    to: create_commands[0].to.clone(),
                    msg: create_commands[0].msg.clone(),
                    execution_rebate: vec![create_trigger_info.funds[0].clone()],
                },
                Trigger {
                    id: 2,
                    owner: owner.clone(),
                    condition: create_commands[1].condition.clone(),
                    to: create_commands[1].to.clone(),
                    msg: create_commands[1].msg.clone(),
                    execution_rebate: vec![create_trigger_info.funds[1].clone()],
                },
            ]
        );
    }

    #[test]
    fn deletes_existing_triggers() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        let create_trigger_info = MessageInfo {
            sender: owner.clone(),
            funds: vec![Coin {
                denom: "rune".to_string(),
                amount: Uint128::new(235463),
            }],
        };

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_command = CreateTrigger {
            condition: Condition::BlockHeight {
                height: env.block.height + 10,
            },
            to: deps.api.addr_make("recipient"),
            msg: to_json_binary(&"test message").unwrap(),
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
        )
        .unwrap();

        assert_eq!(TRIGGER_COUNTER.load(deps.as_ref().storage).unwrap(), 1);

        execute(
            deps.as_mut(),
            env.clone(),
            MessageInfo {
                sender: owner.clone(),
                funds: vec![],
            }
            .clone(),
            SchedulerExecuteMsg::SetTriggers(vec![]),
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

        assert!(triggers.is_empty());
    }

    #[test]
    fn refunds_existing_execution_rebates() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        let create_trigger_info = MessageInfo {
            sender: owner.clone(),
            funds: vec![
                Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(235463),
                },
                Coin {
                    denom: "uruji".to_string(),
                    amount: Uint128::new(736328),
                },
            ],
        };

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_commands = vec![
            CreateTrigger {
                condition: Condition::BlockHeight {
                    height: env.block.height + 10,
                },
                to: deps.api.addr_make("recipient1"),
                msg: to_json_binary(&"test message 1").unwrap(),
            },
            CreateTrigger {
                condition: Condition::Timestamp {
                    timestamp: env.block.time.plus_seconds(100),
                },
                to: deps.api.addr_make("recipient2"),
                msg: to_json_binary(&"test message 2").unwrap(),
            },
        ];

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
            }
            .clone(),
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

    #[test]
    fn sets_multiple_triggers() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        let create_trigger_info = MessageInfo {
            sender: owner.clone(),
            funds: vec![Coin {
                denom: "rune".to_string(),
                amount: Uint128::new(235463),
            }],
        };

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_command = CreateTrigger {
            condition: Condition::BlockHeight {
                height: env.block.height + 10,
            },
            to: deps.api.addr_make("recipient"),
            msg: to_json_binary(&"test message").unwrap(),
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
        )
        .unwrap();

        assert_eq!(TRIGGER_COUNTER.load(deps.as_ref().storage).unwrap(), 1);

        let set_triggers_info = MessageInfo {
            sender: owner.clone(),
            funds: vec![
                Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(345345),
                },
                Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(876749),
                },
            ],
        };

        let set_triggers_commands = vec![
            CreateTrigger {
                condition: Condition::BlockHeight {
                    height: env.block.height + 10,
                },
                to: deps.api.addr_make("recipient1"),
                msg: to_json_binary(&"test message 1").unwrap(),
            },
            CreateTrigger {
                condition: Condition::BlockHeight {
                    height: env.block.height + 20,
                },
                to: deps.api.addr_make("recipient2"),
                msg: to_json_binary(&"test message 2").unwrap(),
            },
        ];

        execute(
            deps.as_mut(),
            env.clone(),
            set_triggers_info.clone(),
            SchedulerExecuteMsg::SetTriggers(set_triggers_commands.clone()),
        )
        .unwrap();

        assert_eq!(TRIGGER_COUNTER.load(deps.as_ref().storage).unwrap(), 3);

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
                    id: 2,
                    owner: owner.clone(),
                    condition: set_triggers_commands[0].condition.clone(),
                    to: set_triggers_commands[0].to.clone(),
                    msg: set_triggers_commands[0].msg.clone(),
                    execution_rebate: vec![set_triggers_info.funds[0].clone()],
                },
                Trigger {
                    id: 3,
                    owner: owner.clone(),
                    condition: set_triggers_commands[1].condition.clone(),
                    to: set_triggers_commands[1].to.clone(),
                    msg: set_triggers_commands[1].msg.clone(),
                    execution_rebate: vec![set_triggers_info.funds[1].clone()],
                },
            ]
        );
    }
}

#[cfg(test)]
mod execute_trigger_tests {
    use super::*;
    use calc_rs::types::{Condition, CreateTrigger};
    use cosmwasm_std::WasmMsg;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Coin, SubMsg, Uint128,
    };

    #[test]
    fn returns_error_if_trigger_does_not_exist() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let execution_info = MessageInfo {
            sender: deps.api.addr_make("executor"),
            funds: vec![],
        };

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let err = execute(
            deps.as_mut(),
            env.clone(),
            execution_info.clone(),
            SchedulerExecuteMsg::ExecuteTrigger(1),
        )
        .unwrap_err();

        assert_eq!(
            err,
            ContractError::Std(StdError::generic_err("Trigger with id 1 does not exist"))
        );
    }

    #[test]
    fn returns_error_if_trigger_cannot_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let execution_info = MessageInfo {
            sender: deps.api.addr_make("executor"),
            funds: vec![],
        };

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_command = CreateTrigger {
            condition: Condition::BlockHeight {
                height: env.block.height + 10,
            },
            to: deps.api.addr_make("recipient"),
            msg: to_json_binary(&"test message").unwrap(),
        };

        let owner = deps.api.addr_make("creator");

        execute(
            deps.as_mut(),
            env.clone(),
            MessageInfo {
                sender: owner,
                funds: vec![Coin {
                    denom: "rune".to_string(),
                    amount: Uint128::new(235463),
                }],
            },
            SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
        )
        .unwrap();

        let err = execute(
            deps.as_mut(),
            env.clone(),
            execution_info.clone(),
            SchedulerExecuteMsg::ExecuteTrigger(1),
        )
        .unwrap_err();

        assert_eq!(
            err,
            ContractError::Std(StdError::generic_err(format!(
                "Condition not met: {:?}",
                create_command.condition
            )))
        );
    }

    #[test]
    fn adds_execute_message_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        let create_trigger_info = MessageInfo {
            sender: owner.clone(),
            funds: vec![Coin {
                denom: "rune".to_string(),
                amount: Uint128::new(235463),
            }],
        };

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_command = CreateTrigger {
            condition: Condition::BlockHeight {
                height: env.block.height - 10,
            },
            to: deps.api.addr_make("recipient"),
            msg: to_json_binary(&"test message").unwrap(),
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
        )
        .unwrap();

        assert_eq!(TRIGGER_COUNTER.load(deps.as_ref().storage).unwrap(), 1);

        let execution_info = MessageInfo {
            sender: deps.api.addr_make("executor"),
            funds: vec![],
        };

        let response = execute(
            deps.as_mut(),
            env.clone(),
            execution_info.clone(),
            SchedulerExecuteMsg::ExecuteTrigger(1),
        )
        .unwrap();

        assert!(response
            .messages
            .contains(&SubMsg::reply_never(WasmMsg::Execute {
                contract_addr: create_command.to.to_string(),
                msg: create_command.msg,
                funds: vec![]
            })));
    }

    #[test]
    fn adds_send_rebate_msg_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        let create_trigger_info = MessageInfo {
            sender: owner.clone(),
            funds: vec![Coin {
                denom: "rune".to_string(),
                amount: Uint128::new(235463),
            }],
        };

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_command = CreateTrigger {
            condition: Condition::BlockHeight {
                height: env.block.height - 10,
            },
            to: deps.api.addr_make("recipient"),
            msg: to_json_binary(&"test message").unwrap(),
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
        )
        .unwrap();

        assert_eq!(TRIGGER_COUNTER.load(deps.as_ref().storage).unwrap(), 1);

        let execution_info = MessageInfo {
            sender: deps.api.addr_make("executor"),
            funds: vec![],
        };

        let response = execute(
            deps.as_mut(),
            env.clone(),
            execution_info.clone(),
            SchedulerExecuteMsg::ExecuteTrigger(1),
        )
        .unwrap();

        assert!(response
            .messages
            .contains(&SubMsg::reply_never(BankMsg::Send {
                to_address: execution_info.sender.to_string(),
                amount: create_trigger_info.funds.clone(),
            })));
    }

    #[test]
    fn deletes_trigger_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        let create_trigger_info = MessageInfo {
            sender: owner.clone(),
            funds: vec![Coin {
                denom: "rune".to_string(),
                amount: Uint128::new(235463),
            }],
        };

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();

        let create_command = CreateTrigger {
            condition: Condition::BlockHeight {
                height: env.block.height - 10,
            },
            to: deps.api.addr_make("recipient"),
            msg: to_json_binary(&"test message").unwrap(),
        };

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::SetTriggers(vec![create_command.clone()]),
        )
        .unwrap();

        assert_eq!(TRIGGER_COUNTER.load(deps.as_ref().storage).unwrap(), 1);

        let execution_info = MessageInfo {
            sender: deps.api.addr_make("executor"),
            funds: vec![],
        };

        execute(
            deps.as_mut(),
            env.clone(),
            execution_info.clone(),
            SchedulerExecuteMsg::ExecuteTrigger(1),
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

        assert!(triggers.is_empty());
    }
}
