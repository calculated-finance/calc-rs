use std::vec;

use calc_rs::{
    core::{Contract, ContractError, ContractResult},
    scheduler::{SchedulerExecuteMsg, SchedulerQueryMsg, Trigger},
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Coin, Deps, DepsMut, Env, MessageInfo, Reply, Response,
    StdError, StdResult, SubMsg,
};

use crate::state::{CONDITION_COUNTER, TRIGGERS, TRIGGER_COUNTER};

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
    CONDITION_COUNTER.save(_deps.storage, &0)?;
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
            TRIGGERS.save(
                deps.storage,
                &Trigger::from_command(&info, command, info.funds.clone()),
            )?;

            Ok(Response::default())
        }
        SchedulerExecuteMsg::SetTriggers(commands) => {
            if !info.funds.is_empty() && info.funds.len() != commands.len() {
                return Err(ContractError::Std(StdError::generic_err(
                    "Must provide either no execution rebates, or 1 execution rebate per trigger",
                )));
            }

            let mut triggers_to_delete: Vec<Trigger> = Vec::new();

            loop {
                let triggers = TRIGGERS.owner(
                    deps.as_ref().storage,
                    info.sender.clone(),
                    Some(30),
                    triggers_to_delete.last().map(|t| t.id),
                );

                triggers_to_delete.extend(triggers.clone());

                if triggers.len() < 30 {
                    break;
                }
            }

            let mut rebates_to_refund: Vec<Coin> = vec![];

            for trigger in triggers_to_delete {
                TRIGGERS.delete(deps.storage, trigger.id)?;
                rebates_to_refund.extend(trigger.execution_rebate);
            }

            for (i, command) in commands.iter().enumerate() {
                TRIGGERS.save(
                    deps.storage,
                    &Trigger::from_command(
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
            let trigger = TRIGGERS.load(deps.storage, id).map_err(|e| {
                ContractError::generic_err(format!("Failed to load trigger: {}", e))
            })?;

            if !trigger.can_execute(deps.as_ref(), &env)? {
                return Err(ContractError::generic_err(format!(
                    "Trigger conditions not met: {:?}",
                    trigger.conditions
                )));
            }

            TRIGGERS.delete(deps.storage, trigger.id)?;

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
        SchedulerQueryMsg::Owned {
            owner,
            limit,
            start_after,
        } => to_json_binary(&TRIGGERS.owner(deps.storage, owner, limit, start_after)),
        SchedulerQueryMsg::Filtered { filter, limit } => {
            to_json_binary(&TRIGGERS.filter(deps.storage, filter, limit)?)
        }
        SchedulerQueryMsg::CanExecute { id } => {
            to_json_binary(&TRIGGERS.load(deps.storage, id)?.can_execute(deps, &env)?)
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
        conditions: vec![calc_rs::core::Condition::BlocksCompleted(
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

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

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
    use calc_rs::{
        core::Condition,
        scheduler::{ConditionFilter, CreateTrigger},
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr, Coin, Decimal, SubMsg, Timestamp,
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
            conditions: vec![
                Condition::BlocksCompleted(env.block.height + 10),
                Condition::TimestampElapsed(Timestamp::from_seconds(env.block.time.seconds() + 10)),
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

    #[test]
    fn deletes_existing_triggers() {
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

        let triggers = TRIGGERS.owner(deps.as_ref().storage, owner.clone(), None, None);

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
    use calc_rs::core::Condition;
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

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

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

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

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

        TRIGGER_COUNTER.save(deps.as_mut().storage, &0).unwrap();
        CONDITION_COUNTER.save(deps.as_mut().storage, &0).unwrap();

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
    use calc_rs::scheduler::TriggerConditionsThreshold;
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
                    &Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        conditions: vec![],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
                    },
                )
                .unwrap();

            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: i,
                        owner: Addr::unchecked(format!("other-owner-{}", i)),
                        conditions: vec![],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
                    },
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env,
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
                .into_iter()
                .map(|i| Trigger {
                    id: i * 2 + 1, // odd ids for the owner
                    owner: Addr::unchecked("owner"),
                    conditions: vec![],
                    threshold: TriggerConditionsThreshold::All,
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

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        conditions: vec![],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
                    },
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env,
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
                .into_iter()
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    conditions: vec![],
                    threshold: TriggerConditionsThreshold::All,
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

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        conditions: vec![],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
                    },
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env,
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
                .into_iter()
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    conditions: vec![],
                    threshold: TriggerConditionsThreshold::All,
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

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    &Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        conditions: vec![],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
                    },
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env,
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
                .into_iter()
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    conditions: vec![],
                    threshold: TriggerConditionsThreshold::All,
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
        core::Condition,
        scheduler::{ConditionFilter, TriggerConditionsThreshold},
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
                    &Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        conditions: vec![Condition::TimestampElapsed(
                            env.block.time.plus_seconds(i as u64 * 10),
                        )],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
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
                .into_iter()
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    conditions: vec![Condition::TimestampElapsed(
                        env.block.time.plus_seconds(i as u64 * 10),
                    )],
                    threshold: TriggerConditionsThreshold::All,
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
                    &Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        conditions: vec![Condition::TimestampElapsed(
                            env.block.time.plus_seconds(i as u64 * 10),
                        )],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
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
                .into_iter()
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    conditions: vec![Condition::TimestampElapsed(
                        env.block.time.plus_seconds(i as u64 * 10),
                    )],
                    threshold: TriggerConditionsThreshold::All,
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
                    &Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        conditions: vec![Condition::BlocksCompleted(
                            env.block.height + i as u64 * 10,
                        )],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
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
                .into_iter()
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    conditions: vec![Condition::BlocksCompleted(env.block.height + i as u64 * 10,)],
                    threshold: TriggerConditionsThreshold::All,
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
                    &Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        conditions: vec![Condition::BlocksCompleted(
                            env.block.height + i as u64 * 10,
                        )],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
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
                .into_iter()
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    conditions: vec![Condition::BlocksCompleted(env.block.height + i as u64 * 10,)],
                    threshold: TriggerConditionsThreshold::All,
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
                    &Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        conditions: vec![Condition::LimitOrderFilled {
                            pair_address: Addr::unchecked("pair"),
                            owner: Addr::unchecked("owner"),
                            side: Side::Base,
                            price: Price::Fixed(Decimal::from_str(&i.to_string()).unwrap()),
                        }],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
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
                .into_iter()
                .map(|i| {
                    Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        conditions: vec![Condition::LimitOrderFilled {
                            pair_address: Addr::unchecked("pair"),
                            owner: Addr::unchecked("owner"),
                            side: Side::Base,
                            price: Price::Fixed(Decimal::from_str(&i.to_string()).unwrap()),
                        }],
                        threshold: TriggerConditionsThreshold::All,
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
                    &Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        conditions: vec![Condition::LimitOrderFilled {
                            pair_address: Addr::unchecked("pair"),
                            owner: Addr::unchecked("owner"),
                            side: Side::Base,
                            price: Price::Fixed(Decimal::from_str(&i.to_string()).unwrap()),
                        }],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
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
                .into_iter()
                .map(|i| {
                    Trigger {
                        id: i,
                        owner: Addr::unchecked("owner"),
                        conditions: vec![Condition::LimitOrderFilled {
                            pair_address: Addr::unchecked("pair"),
                            owner: Addr::unchecked("owner"),
                            side: Side::Base,
                            price: Price::Fixed(Decimal::from_str(&i.to_string()).unwrap()),
                        }],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("recipient"),
                        msg: to_json_binary(&"test message").unwrap(),
                        execution_rebate: vec![],
                    }
                })
                .collect::<Vec<_>>()
        );
    }
}
