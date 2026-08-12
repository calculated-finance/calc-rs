use std::{collections::HashSet, vec};

use calc_rs::{
    conditions::condition::Condition,
    core::{Contract, ContractError, ContractResult},
    scheduler::{
        SchedulerConfig, SchedulerExecuteMsg, SchedulerInstantiateMsg, SchedulerQueryMsg, Trigger,
    },
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Addr, BankMsg, Binary, Coin, Coins, Deps, DepsMut, Env, MessageInfo, Reply,
    Response, StdResult, SubMsg, SubMsgResult, Uint64,
};

use crate::state::{CONFIG, TRIGGERS};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: SchedulerInstantiateMsg,
) -> ContractResult {
    let owner = validate_owner(deps.as_ref(), msg.owner)?;

    CONFIG.save(
        deps.storage,
        &SchedulerConfig {
            owner,
            enforcement_enabled: false,
            accepted_rebate_minimums: vec![],
        },
    )?;

    Ok(Response::new())
}

#[cw_serde]
pub struct MigrateMsg {
    pub owner: Addr,
    pub enforcement_enabled: bool,
    pub accepted_rebate_minimums: Vec<Coin>,
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: MigrateMsg) -> ContractResult {
    let owner = validate_owner(deps.as_ref(), msg.owner)?;
    validate_policy(msg.enforcement_enabled, &msg.accepted_rebate_minimums)?;

    CONFIG.save(
        deps.storage,
        &SchedulerConfig {
            owner,
            enforcement_enabled: msg.enforcement_enabled,
            accepted_rebate_minimums: msg.accepted_rebate_minimums,
        },
    )?;

    Ok(Response::new())
}

const MAX_EXECUTORS: usize = 10;

fn validate_owner(deps: Deps, owner: Addr) -> Result<Addr, ContractError> {
    deps.api
        .addr_validate(owner.as_str())
        .map_err(|_| ContractError::generic_err("Invalid scheduler owner address"))
}

fn validate_policy(
    enforcement_enabled: bool,
    accepted_rebate_minimums: &[Coin],
) -> Result<(), ContractError> {
    if enforcement_enabled && accepted_rebate_minimums.is_empty() {
        return Err(ContractError::generic_err(
            "Rebate enforcement requires at least one accepted minimum",
        ));
    }

    let mut denoms = HashSet::with_capacity(accepted_rebate_minimums.len());

    for minimum in accepted_rebate_minimums {
        if minimum.amount.is_zero() {
            return Err(ContractError::generic_err(format!(
                "Accepted rebate minimum for {} must be greater than zero",
                minimum.denom
            )));
        }

        if !denoms.insert(minimum.denom.as_str()) {
            return Err(ContractError::generic_err(format!(
                "Duplicate accepted rebate denom: {}",
                minimum.denom
            )));
        }
    }

    Ok(())
}

fn normalize_and_validate_rebate(
    config: &SchedulerConfig,
    funds: Vec<Coin>,
) -> Result<Vec<Coin>, ContractError> {
    let funds = Coins::try_from(funds)?.to_vec();

    if config.enforcement_enabled
        && !config.accepted_rebate_minimums.iter().any(|minimum| {
            funds
                .iter()
                .any(|coin| coin.denom == minimum.denom && coin.amount >= minimum.amount)
        })
    {
        return Err(ContractError::generic_err(
            "Attached execution rebate does not meet any configured minimum",
        ));
    }

    Ok(funds)
}

fn execute_triggers(
    deps: DepsMut,
    env: &Env,
    info: &MessageInfo,
    ids: Vec<Uint64>,
    rebate_receiver: Addr,
) -> ContractResult {
    let mut sub_messages = Vec::with_capacity(ids.len() * 2);

    for id in ids {
        let trigger = match TRIGGERS.load(deps.storage, id) {
            Ok(trigger) => trigger,
            Err(_) => continue,
        };

        if !trigger.executors.is_empty() && !trigger.executors.contains(&info.sender) {
            continue;
        }

        match trigger.condition.is_satisfied(deps.as_ref(), env) {
            Ok(true) => {}
            _ => continue,
        }

        TRIGGERS.delete(deps.storage, trigger.id.into())?;

        sub_messages.push(SubMsg::reply_on_error(
            Contract(trigger.contract_address).call(trigger.msg, vec![]),
            0,
        ));

        if !trigger.execution_rebate.is_empty() {
            sub_messages.push(SubMsg::reply_never(BankMsg::Send {
                to_address: rebate_receiver.to_string(),
                amount: trigger.execution_rebate,
            }));
        }
    }

    Ok(Response::new().add_submessages(sub_messages))
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
            let existing_trigger = TRIGGERS.load(deps.storage, trigger_id).ok();

            if let Some(existing_trigger) = &existing_trigger {
                if info.sender != existing_trigger.owner {
                    return Err(ContractError::generic_err(
                        "Only the owner can update an existing trigger",
                    ));
                }
            }

            let config = CONFIG.load(deps.storage)?;
            let execution_rebate = normalize_and_validate_rebate(&config, info.funds.clone())?;

            if let Some(existing_trigger) = existing_trigger {
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
                    execution_rebate,
                    jitter: create_command.jitter,
                },
            )?;

            Ok(Response::new().add_submessages(sub_messages))
        }
        SchedulerExecuteMsg::Execute(ids) => {
            let rebate_receiver = info.sender.clone();
            execute_triggers(deps, &env, &info, ids, rebate_receiver)
        }
        SchedulerExecuteMsg::ExecuteWithRebateReceiver {
            ids,
            rebate_receiver,
        } => {
            let rebate_receiver = deps
                .api
                .addr_validate(rebate_receiver.as_str())
                .map_err(|_| {
                    ContractError::generic_err(format!(
                        "Invalid rebate receiver address: {rebate_receiver}"
                    ))
                })?;

            execute_triggers(deps, &env, &info, ids, rebate_receiver)
        }
        SchedulerExecuteMsg::UpdateConfig {
            enforcement_enabled,
            accepted_rebate_minimums,
        } => {
            let config = CONFIG.load(deps.storage)?;

            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            if !info.funds.is_empty() {
                return Err(ContractError::generic_err(
                    "Cannot attach funds to a scheduler config update",
                ));
            }

            validate_policy(enforcement_enabled, &accepted_rebate_minimums)?;

            CONFIG.save(
                deps.storage,
                &SchedulerConfig {
                    owner: config.owner,
                    enforcement_enabled,
                    accepted_rebate_minimums,
                },
            )?;

            Ok(Response::new())
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: SchedulerQueryMsg) -> StdResult<Binary> {
    match msg {
        SchedulerQueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
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
fn initialize_test_config(deps: DepsMut) {
    CONFIG
        .save(
            deps.storage,
            &SchedulerConfig {
                owner: Addr::unchecked("scheduler-owner"),
                enforcement_enabled: false,
                accepted_rebate_minimums: vec![],
            },
        )
        .unwrap();
}

#[cfg(test)]
mod config_and_rebate_policy_tests {
    use super::*;
    use calc_rs::{
        conditions::condition::Condition,
        scheduler::{CreateTriggerMsg, SchedulerQueryMsg},
    };
    use cosmwasm_std::{
        from_json,
        testing::{message_info, mock_dependencies, mock_env},
    };

    fn create_msg(env: &Env, target: Addr) -> CreateTriggerMsg {
        CreateTriggerMsg {
            condition: Condition::BlocksCompleted(env.block.height.saturating_sub(1)),
            msg: Binary::default(),
            contract_address: target,
            executors: vec![],
            jitter: None,
        }
    }

    fn update_config(
        deps: DepsMut,
        env: &Env,
        owner: &Addr,
        enforcement_enabled: bool,
        accepted_rebate_minimums: Vec<Coin>,
    ) -> ContractResult {
        execute(
            deps,
            env.clone(),
            message_info(owner, &[]),
            SchedulerExecuteMsg::UpdateConfig {
                enforcement_enabled,
                accepted_rebate_minimums,
            },
        )
    }

    #[test]
    fn instantiate_stores_message_owner_and_config_query_returns_it() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let sender = deps.api.addr_make("sender");
        let owner = deps.api.addr_make("explicit-owner");

        instantiate(
            deps.as_mut(),
            env.clone(),
            message_info(&sender, &[]),
            SchedulerInstantiateMsg {
                owner: owner.clone(),
            },
        )
        .unwrap();

        let config: SchedulerConfig =
            from_json(query(deps.as_ref(), env, SchedulerQueryMsg::Config {}).unwrap()).unwrap();

        assert_eq!(
            config,
            SchedulerConfig {
                owner,
                enforcement_enabled: false,
                accepted_rebate_minimums: vec![],
            }
        );
        assert_ne!(config.owner, sender);
    }

    #[test]
    fn migrate_writes_supplied_config() {
        let mut deps = mock_dependencies();
        let owner = deps.api.addr_make("migration-owner");
        let minimums = vec![
            Coin::new(100_000u128, "x/ruji"),
            Coin::new(50_000u128, "rune"),
        ];

        migrate(
            deps.as_mut(),
            mock_env(),
            MigrateMsg {
                owner: owner.clone(),
                enforcement_enabled: true,
                accepted_rebate_minimums: minimums.clone(),
            },
        )
        .unwrap();

        assert_eq!(
            CONFIG.load(deps.as_ref().storage).unwrap(),
            SchedulerConfig {
                owner,
                enforcement_enabled: true,
                accepted_rebate_minimums: minimums,
            }
        );
    }

    #[test]
    fn only_owner_can_update_config_and_updates_cannot_attach_funds() {
        let mut deps = mock_dependencies();
        initialize_test_config(deps.as_mut());
        let env = mock_env();
        let owner = CONFIG.load(deps.as_ref().storage).unwrap().owner;
        let stranger = deps.api.addr_make("stranger");
        let msg = SchedulerExecuteMsg::UpdateConfig {
            enforcement_enabled: true,
            accepted_rebate_minimums: vec![Coin::new(100u128, "x/ruji")],
        };

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&stranger, &[]),
                msg.clone(),
            ),
            Err(ContractError::Unauthorized {})
        );

        let err = execute(
            deps.as_mut(),
            env,
            message_info(&owner, &[Coin::new(1u128, "rune")]),
            msg,
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("Cannot attach funds to a scheduler config update"));
    }

    #[test]
    fn owner_atomically_replaces_policy_and_keeps_owner_immutable() {
        let mut deps = mock_dependencies();
        initialize_test_config(deps.as_mut());
        let env = mock_env();
        let owner = CONFIG.load(deps.as_ref().storage).unwrap().owner;

        let policies = [
            (false, vec![Coin::new(100u128, "x/ruji")]),
            (
                true,
                vec![Coin::new(150u128, "x/ruji"), Coin::new(50u128, "rune")],
            ),
            (true, vec![Coin::new(40u128, "rune")]),
            (false, vec![]),
        ];

        for (enforcement_enabled, accepted_rebate_minimums) in policies {
            update_config(
                deps.as_mut(),
                &env,
                &owner,
                enforcement_enabled,
                accepted_rebate_minimums.clone(),
            )
            .unwrap();

            assert_eq!(
                CONFIG.load(deps.as_ref().storage).unwrap(),
                SchedulerConfig {
                    owner: owner.clone(),
                    enforcement_enabled,
                    accepted_rebate_minimums,
                }
            );
        }
    }

    #[test]
    fn rejects_invalid_policy_lists() {
        let mut deps = mock_dependencies();
        initialize_test_config(deps.as_mut());
        let env = mock_env();
        let owner = CONFIG.load(deps.as_ref().storage).unwrap().owner;

        let invalid = [
            (
                true,
                vec![],
                "Rebate enforcement requires at least one accepted minimum",
            ),
            (
                false,
                vec![Coin::new(0u128, "rune")],
                "must be greater than zero",
            ),
            (
                false,
                vec![Coin::new(1u128, "rune"), Coin::new(2u128, "rune")],
                "Duplicate accepted rebate denom: rune",
            ),
        ];

        for (enabled, minimums, expected_error) in invalid {
            let err = update_config(deps.as_mut(), &env, &owner, enabled, minimums).unwrap_err();
            assert!(
                err.to_string().contains(expected_error),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn enforcement_disabled_accepts_arbitrary_or_no_rebate() {
        let mut deps = mock_dependencies();
        initialize_test_config(deps.as_mut());
        let env = mock_env();
        let creator = deps.api.addr_make("creator");
        let create = create_msg(&env, creator.clone());

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&creator, &[Coin::new(1u128, "unsupported")]),
            SchedulerExecuteMsg::Create(Box::new(create.clone())),
        )
        .unwrap();
        execute(
            deps.as_mut(),
            env,
            message_info(&creator, &[]),
            SchedulerExecuteMsg::Create(Box::new(create.clone())),
        )
        .unwrap();

        assert!(TRIGGERS
            .load(deps.as_ref().storage, create.id(&creator).unwrap())
            .unwrap()
            .execution_rebate
            .is_empty());
    }

    #[test]
    fn enforcement_accepts_exact_or_higher_supported_rebate() {
        let mut deps = mock_dependencies();
        initialize_test_config(deps.as_mut());
        let env = mock_env();
        let owner = CONFIG.load(deps.as_ref().storage).unwrap().owner;
        let creator = deps.api.addr_make("creator");
        let create = create_msg(&env, creator.clone());

        update_config(
            deps.as_mut(),
            &env,
            &owner,
            true,
            vec![
                Coin::new(100_000u128, "x/ruji"),
                Coin::new(50_000u128, "rune"),
            ],
        )
        .unwrap();

        for funds in [
            vec![Coin::new(100_000u128, "x/ruji")],
            vec![Coin::new(150_000u128, "x/ruji")],
            vec![Coin::new(50_000u128, "rune")],
            vec![
                Coin::new(100_000u128, "x/ruji"),
                Coin::new(1u128, "eth-usdc"),
            ],
        ] {
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&creator, &funds),
                SchedulerExecuteMsg::Create(Box::new(create.clone())),
            )
            .unwrap();

            assert_eq!(
                TRIGGERS
                    .load(deps.as_ref().storage, create.id(&creator).unwrap())
                    .unwrap()
                    .execution_rebate,
                Coins::try_from(funds).unwrap().to_vec()
            );
        }
    }

    #[test]
    fn enforcement_rejects_below_unsupported_or_missing_rebate() {
        let mut deps = mock_dependencies();
        initialize_test_config(deps.as_mut());
        let env = mock_env();
        let owner = CONFIG.load(deps.as_ref().storage).unwrap().owner;
        let creator = deps.api.addr_make("creator");
        let create = create_msg(&env, creator.clone());

        update_config(
            deps.as_mut(),
            &env,
            &owner,
            true,
            vec![
                Coin::new(100_000u128, "x/ruji"),
                Coin::new(50_000u128, "rune"),
            ],
        )
        .unwrap();

        for funds in [
            vec![Coin::new(99_999u128, "x/ruji")],
            vec![Coin::new(1_000u128, "eth-usdc")],
            vec![],
        ] {
            let err = execute(
                deps.as_mut(),
                env.clone(),
                message_info(&creator, &funds),
                SchedulerExecuteMsg::Create(Box::new(create.clone())),
            )
            .unwrap_err();

            assert!(err
                .to_string()
                .contains("Attached execution rebate does not meet any configured minimum"));
        }
    }

    #[test]
    fn existing_trigger_remains_executable_and_pays_rebate_after_enforcement() {
        let mut deps = mock_dependencies();
        initialize_test_config(deps.as_mut());
        let env = mock_env();
        let owner = CONFIG.load(deps.as_ref().storage).unwrap().owner;
        let creator = deps.api.addr_make("creator");
        let keeper = deps.api.addr_make("keeper");
        let create = create_msg(&env, creator.clone());
        let rebate = vec![Coin::new(7u128, "legacy")];

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&creator, &rebate),
            SchedulerExecuteMsg::Create(Box::new(create.clone())),
        )
        .unwrap();
        update_config(
            deps.as_mut(),
            &env,
            &owner,
            true,
            vec![Coin::new(100u128, "x/ruji")],
        )
        .unwrap();

        let response = execute(
            deps.as_mut(),
            env,
            message_info(&keeper, &[]),
            SchedulerExecuteMsg::Execute(vec![create.id(&creator).unwrap()]),
        )
        .unwrap();

        assert!(response
            .messages
            .contains(&SubMsg::reply_never(BankMsg::Send {
                to_address: keeper.to_string(),
                amount: rebate,
            })));
        assert!(TRIGGERS
            .load(deps.as_ref().storage, create.id(&creator).unwrap())
            .is_err());
    }

    #[test]
    fn replacement_must_comply_and_refunds_existing_rebate() {
        let mut deps = mock_dependencies();
        initialize_test_config(deps.as_mut());
        let env = mock_env();
        let owner = CONFIG.load(deps.as_ref().storage).unwrap().owner;
        let creator = deps.api.addr_make("creator");
        let create = create_msg(&env, creator.clone());
        let old_rebate = vec![Coin::new(70u128, "rune")];

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&creator, &old_rebate),
            SchedulerExecuteMsg::Create(Box::new(create.clone())),
        )
        .unwrap();
        update_config(
            deps.as_mut(),
            &env,
            &owner,
            true,
            vec![Coin::new(100u128, "x/ruji")],
        )
        .unwrap();

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&creator, &[Coin::new(99u128, "x/ruji")]),
            SchedulerExecuteMsg::Create(Box::new(create.clone())),
        )
        .unwrap_err();
        assert_eq!(
            TRIGGERS
                .load(deps.as_ref().storage, create.id(&creator).unwrap())
                .unwrap()
                .execution_rebate,
            old_rebate
        );

        let replacement_rebate = vec![Coin::new(5u128, "rune"), Coin::new(100u128, "x/ruji")];
        let response = execute(
            deps.as_mut(),
            env,
            message_info(&creator, &replacement_rebate),
            SchedulerExecuteMsg::Create(Box::new(create.clone())),
        )
        .unwrap();

        assert!(response
            .messages
            .contains(&SubMsg::reply_never(BankMsg::Send {
                to_address: creator.to_string(),
                amount: old_rebate,
            })));
        assert_eq!(
            TRIGGERS
                .load(deps.as_ref().storage, create.id(&creator).unwrap())
                .unwrap()
                .execution_rebate,
            replacement_rebate
        );
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
        initialize_test_config(deps.as_mut());
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
        initialize_test_config(deps.as_mut());
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
    fn checks_existing_trigger_owner_before_rebate_policy() {
        let mut deps = mock_dependencies();
        initialize_test_config(deps.as_mut());
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

        let mut config = CONFIG.load(deps.as_ref().storage).unwrap();
        config.enforcement_enabled = true;
        config.accepted_rebate_minimums = vec![Coin::new(100u128, "x/ruji")];
        CONFIG.save(deps.as_mut().storage, &config).unwrap();

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
        initialize_test_config(deps.as_mut());
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
        initialize_test_config(deps.as_mut());
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
        initialize_test_config(deps.as_mut());
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
        initialize_test_config(deps.as_mut());
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
    fn sends_rebate_to_nominated_receiver() {
        let mut deps = mock_dependencies();
        initialize_test_config(deps.as_mut());
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let rebate_receiver = deps.api.addr_make("rebate-receiver");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);
        let create_trigger_msg = CreateTriggerMsg {
            condition: Condition::BlocksCompleted(env.block.height - 10),
            msg: Binary::default(),
            contract_address: owner.clone(),
            executors: vec![executor.clone()],
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
            env,
            message_info(&executor, &[]),
            SchedulerExecuteMsg::ExecuteWithRebateReceiver {
                ids: vec![create_trigger_msg.id(&owner).unwrap()],
                rebate_receiver: rebate_receiver.clone(),
            },
        )
        .unwrap();

        assert!(response
            .messages
            .contains(&SubMsg::reply_never(BankMsg::Send {
                to_address: rebate_receiver.to_string(),
                amount: create_trigger_info.funds,
            })));
    }

    #[test]
    fn rejects_invalid_rebate_receiver() {
        let mut deps = mock_dependencies();
        initialize_test_config(deps.as_mut());
        let executor = deps.api.addr_make("executor");

        let err = execute(
            deps.as_mut(),
            mock_env(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::ExecuteWithRebateReceiver {
                ids: vec![],
                rebate_receiver: Addr::unchecked(""),
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("Invalid rebate receiver address"));
    }

    #[test]
    fn deletes_trigger_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        initialize_test_config(deps.as_mut());
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
        initialize_test_config(deps.as_mut());
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
        initialize_test_config(deps.as_mut());
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
        initialize_test_config(deps.as_mut());
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
        initialize_test_config(deps.as_mut());
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
