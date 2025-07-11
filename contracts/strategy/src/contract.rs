use std::collections::HashSet;

use calc_rs::{
    constants::{LOG_ERRORS_REPLY_ID, PROCESS_PAYLOAD_REPLY_ID},
    core::{Contract, ContractError, ContractResult},
    manager::StrategyStatus,
    strategy::{
        StrategyConfig, StrategyExecuteMsg, StrategyInstantiateMsg, StrategyMsgPayload,
        StrategyQueryMsg,
    },
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, BankMsg, Binary, Coins, Deps, DepsMut, Env, MessageInfo, Reply,
    Response, StdResult, SubMsg, SubMsgResult,
};

use crate::state::{ACTIVE_STRATEGY, CONFIG, ESCROWED, STATE, STATS};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    if msg.state.contract_address != env.contract.address {
        return Err(ContractError::generic_err(format!(
            "Strategy contract address mismatch: expected {}, got {}",
            env.contract.address, msg.state.contract_address
        )));
    }

    // Collate escrowed denoms & initialise the strategy
    let escrowed = msg.escrowed(deps.as_ref(), &env)?;
    let response = msg.init(&mut deps, &env, |storage, strategy| {
        CONFIG.init(
            storage,
            StrategyConfig {
                manager: info.sender.clone(),
                strategy,
                escrowed,
            },
        )
    })?;

    // Execute the strategy immediately after instantiation
    Ok(response.add_submessage(SubMsg::reply_always(
        Contract(env.contract.address.clone())
            .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, vec![]),
        LOG_ERRORS_REPLY_ID,
    )))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyExecuteMsg,
) -> ContractResult {
    let state = STATE.may_load(deps.storage)?;

    // We allow arbitrary distribution messages to be sent, which
    // could result in recursive calls between strategies and/or other
    // contracts. This is a safety check to short circuit that.
    if let Some(state) = state {
        if msg == state {
            return Err(ContractError::generic_err(format!(
                "Contract is already in the {state:?} state, cannot execute again"
            )));
        }
    }

    let config = CONFIG.load(deps.storage)?;

    let response = match msg {
        StrategyExecuteMsg::Execute => {
            if info.sender != config.manager && info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            let execute_strategy_response = config
                .strategy
                .activate()
                .prepare_to_execute(deps.as_ref(), &env)?
                .execute(&mut deps, &env, |store, strategy| {
                    ACTIVE_STRATEGY.save(store, &strategy)
                })?;

            execute_strategy_response
        }
        StrategyExecuteMsg::Update(update) => {
            if info.sender != config.manager {
                return Err(ContractError::Unauthorized {});
            }

            let cancel_strategy_response = config
                .strategy
                .activate()
                .prepare_to_cancel(deps.as_ref(), &env)?
                .execute(&mut deps, &env, |store, strategy| {
                    ACTIVE_STRATEGY.save(store, &strategy)
                })?;

            // If no stateful actions to unwind, we can proceed with the update
            if cancel_strategy_response.messages.is_empty() {
                // Accumulate any newly escrowed denoms
                let escrowed = update
                    .escrowed(deps.as_ref(), &env)?
                    .union(&config.escrowed)
                    .cloned()
                    .collect::<HashSet<String>>();

                ESCROWED.save(deps.storage, &escrowed)?;

                // Get the required messages to initialize the new strategy
                let init_strategy_response =
                    update.init(&mut deps, &env, |storage, strategy| {
                        CONFIG.save(storage, strategy)
                    })?;

                let execute_new_strategy_msg = SubMsg::reply_always(
                    Contract(env.contract.address.clone())
                        .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, vec![]),
                    LOG_ERRORS_REPLY_ID,
                );

                // Execute the new strategy after all init messages have completed
                init_strategy_response.add_submessage(execute_new_strategy_msg)
            } else {
                let update_again_msg = SubMsg::reply_always(
                    Contract(env.contract.address.clone())
                        .call(to_json_binary(&StrategyExecuteMsg::Update(update))?, vec![]),
                    LOG_ERRORS_REPLY_ID,
                );

                // Clear the state so we can run update again
                STATE.remove(deps.storage);

                cancel_strategy_response // Unwind any stateful actions before we overwrite them
                    .add_submessage(update_again_msg) // Run update to setup the new strategy
            }
        }
        StrategyExecuteMsg::Withdraw(desired) => {
            if info.sender != config.strategy.owner && info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            for denom in desired.iter() {
                if config.escrowed.contains(denom) {
                    return Err(ContractError::generic_err(format!(
                        "Cannot withdraw escrowed denom: {denom}"
                    )));
                }
            }

            let owner = config.strategy.owner.to_string();

            let withdraw_from_strategy_response = config
                .strategy
                .activate()
                .prepare_to_withdraw(deps.as_ref(), &env, &desired)?
                .execute(&mut deps, &env, |store, strategy| {
                    ACTIVE_STRATEGY.save(store, &strategy)
                })?;

            // If no stateful actions to unwind, go ahead
            // and withdraw from the contract address itself.
            if withdraw_from_strategy_response.messages.is_empty() {
                let mut withdrawals = Coins::default();

                for denom in desired.iter() {
                    let balance = deps
                        .querier
                        .query_balance(env.contract.address.clone(), denom.clone())?;

                    withdrawals.add(balance.clone())?;
                }

                let withdrawal_bank_msg = SubMsg::reply_always(
                    BankMsg::Send {
                        to_address: owner,
                        amount: withdrawals.to_vec(),
                    },
                    LOG_ERRORS_REPLY_ID,
                );

                Response::default().add_submessage(withdrawal_bank_msg)
            } else {
                let withdraw_again_msg = SubMsg::reply_always(
                    Contract(env.contract.address.clone()).call(
                        to_json_binary(&StrategyExecuteMsg::Withdraw(desired))?,
                        vec![],
                    ),
                    LOG_ERRORS_REPLY_ID,
                );

                // Clear the state so we can run withdraw again
                STATE.remove(deps.storage);

                withdraw_from_strategy_response.add_submessage(withdraw_again_msg)
            }
        }
        StrategyExecuteMsg::UpdateStatus(status) => {
            if info.sender != config.manager {
                return Err(ContractError::Unauthorized {});
            }

            match status {
                StrategyStatus::Active => {
                    let execute_strategy_response = config
                        .strategy
                        .activate()
                        .prepare_to_execute(deps.as_ref(), &env)?
                        .execute(&mut deps, &env, |store, strategy| {
                            ACTIVE_STRATEGY.save(store, &strategy)
                        })?;

                    execute_strategy_response
                }
                // Paused & Archived are no different in terms of execution,
                // they are only used for filtering strategies in factory queries
                StrategyStatus::Paused | StrategyStatus::Archived => {
                    let cancel_strategy_response = config
                        .strategy
                        .activate()
                        .prepare_to_cancel(deps.as_ref(), &env)?
                        .execute(&mut deps, &env, |store, strategy| {
                            ACTIVE_STRATEGY.save(store, &strategy)
                        })?;

                    cancel_strategy_response
                }
            }
        }
        StrategyExecuteMsg::Commit => {
            if info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            let active_strategy = ACTIVE_STRATEGY.load(deps.storage)?;

            active_strategy
                .prepare_to_commit(deps.as_ref(), &env)?
                .commit(&mut deps, |store, strategy| {
                    ACTIVE_STRATEGY.remove(store);
                    CONFIG.save(store, strategy)
                })?
        }
        StrategyExecuteMsg::Clear => {
            if info.sender != env.contract.address && info.sender != config.strategy.owner {
                return Err(ContractError::Unauthorized {});
            }

            STATE.remove(deps.storage);

            // Avoid sending another clear state message
            return Ok(Response::default());
        }
    };

    let clear_state_msg = SubMsg::reply_always(
        Contract(env.contract.address.clone())
            .call(to_json_binary(&StrategyExecuteMsg::Clear {})?, vec![]),
        LOG_ERRORS_REPLY_ID,
    );

    Ok(response.add_submessage(clear_state_msg))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, reply: Reply) -> ContractResult {
    let response = Response::new().add_attribute("reply_id", reply.id.to_string());
    match reply.id {
        PROCESS_PAYLOAD_REPLY_ID => {
            let payload = from_json::<StrategyMsgPayload>(reply.payload.clone());
            if let Ok(payload) = payload {
                match reply.result {
                    SubMsgResult::Ok(_) => {
                        let events = payload.decorated_events("succeeded");
                        STATS.update(deps.storage, |s| s.update(payload.statistics))?;
                        Ok(response.add_events(events))
                    }
                    SubMsgResult::Err(err) => Ok(response
                        .add_events(payload.decorated_events("failed"))
                        .add_attribute("msg_error", err)),
                }
            } else {
                Ok(response
                    .add_attribute("msg_error", "Failed to parse reply payload")
                    .add_attribute("msg_payload", reply.payload.to_string()))
            }
        }
        _ => match reply.result {
            SubMsgResult::Ok(_) => Ok(response),
            SubMsgResult::Err(err) => Ok(response
                .add_attribute("msg_error", err)
                .add_attribute("msg_payload", reply.payload.to_string())),
        },
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: StrategyQueryMsg) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;

    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&config),
        StrategyQueryMsg::Statistics {} => to_json_binary(&STATS.load(deps.storage)?),
        StrategyQueryMsg::Balances(include) => {
            let mut balances = config.strategy.balances(deps, &env, &include)?;

            for denom in include {
                let balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), denom)?;

                balances.add(balance)?;
            }

            to_json_binary(&balances.to_vec())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::CONFIG;
    use calc_rs::{
        actions::action::Action,
        strategy::{Active, Committed, Indexed, Strategy},
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr,
    };

    #[test]
    fn test_only_manager_can_invoke_update() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("owner");
        let manager = Addr::unchecked("manager");

        let strategy = Strategy {
            owner: owner.clone(),
            action: Action::Many(vec![]),
            state: Indexed {
                contract_address: env.contract.address.clone(),
            },
        };

        CONFIG
            .init(
                deps.as_mut().storage,
                StrategyConfig {
                    manager: manager.clone(),
                    strategy: Strategy {
                        owner: strategy.owner.clone(),
                        action: strategy.action.clone(),
                        state: Committed {
                            contract_address: env.contract.address.clone(),
                        },
                    },
                    escrowed: HashSet::new(),
                },
            )
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&owner, &[]),
                StrategyExecuteMsg::Update(strategy.clone())
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&env.contract.address, &[]),
                StrategyExecuteMsg::Update(strategy.clone())
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("anyone"), &[]),
                StrategyExecuteMsg::Update(strategy.clone())
            ),
            Err(ContractError::Unauthorized {})
        );
    }

    #[test]
    fn test_only_manager_and_contract_can_invoke_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("owner");
        let manager = Addr::unchecked("manager");

        let strategy = Strategy {
            owner: owner.clone(),
            action: Action::Many(vec![]),
            state: Indexed {
                contract_address: env.contract.address.clone(),
            },
        };

        CONFIG
            .init(
                deps.as_mut().storage,
                StrategyConfig {
                    manager: manager.clone(),
                    strategy: Strategy {
                        owner: strategy.owner.clone(),
                        action: strategy.action.clone(),
                        state: Committed {
                            contract_address: env.contract.address.clone(),
                        },
                    },
                    escrowed: HashSet::new(),
                },
            )
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&manager, &[]),
            StrategyExecuteMsg::Execute
        )
        .is_ok());

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&env.contract.address, &[]),
            StrategyExecuteMsg::Execute
        )
        .is_ok());

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&owner, &[]),
                StrategyExecuteMsg::Execute
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("anyone"), &[]),
                StrategyExecuteMsg::Execute
            ),
            Err(ContractError::Unauthorized {})
        );
    }

    #[test]
    fn test_only_owner_and_contract_can_invoke_withdraw() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("owner");
        let manager = Addr::unchecked("manager");

        let strategy = Strategy {
            owner: owner.clone(),
            action: Action::Many(vec![]),
            state: Indexed {
                contract_address: env.contract.address.clone(),
            },
        };

        CONFIG
            .init(
                deps.as_mut().storage,
                StrategyConfig {
                    manager: manager.clone(),
                    strategy: Strategy {
                        owner: strategy.owner.clone(),
                        action: strategy.action.clone(),
                        state: Committed {
                            contract_address: env.contract.address.clone(),
                        },
                    },
                    escrowed: HashSet::new(),
                },
            )
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[]),
            StrategyExecuteMsg::Withdraw(HashSet::new())
        )
        .is_ok());

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&env.contract.address, &[]),
            StrategyExecuteMsg::Withdraw(HashSet::new())
        )
        .is_ok());

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&manager, &[]),
                StrategyExecuteMsg::Withdraw(HashSet::new())
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("anyone"), &[]),
                StrategyExecuteMsg::Withdraw(HashSet::new())
            ),
            Err(ContractError::Unauthorized {})
        );
    }

    #[test]
    fn test_only_manager_can_invoke_update_status() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("owner");
        let manager = Addr::unchecked("manager");

        let strategy = Strategy {
            owner: owner.clone(),
            action: Action::Many(vec![]),
            state: Indexed {
                contract_address: env.contract.address.clone(),
            },
        };

        CONFIG
            .init(
                deps.as_mut().storage,
                StrategyConfig {
                    manager: manager.clone(),
                    strategy: Strategy {
                        owner: strategy.owner.clone(),
                        action: strategy.action.clone(),
                        state: Committed {
                            contract_address: env.contract.address.clone(),
                        },
                    },
                    escrowed: HashSet::new(),
                },
            )
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&manager, &[]),
            StrategyExecuteMsg::UpdateStatus(StrategyStatus::Archived)
        )
        .is_ok());

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&env.contract.address, &[]),
                StrategyExecuteMsg::UpdateStatus(StrategyStatus::Archived)
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&owner, &[]),
                StrategyExecuteMsg::UpdateStatus(StrategyStatus::Archived)
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("anyone"), &[]),
                StrategyExecuteMsg::UpdateStatus(StrategyStatus::Archived)
            ),
            Err(ContractError::Unauthorized {})
        );
    }

    #[test]
    fn test_only_contract_can_invoke_commit() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("owner");
        let manager = Addr::unchecked("manager");

        let strategy = Strategy {
            owner: owner.clone(),
            action: Action::Many(vec![]),
            state: Indexed {
                contract_address: env.contract.address.clone(),
            },
        };

        CONFIG
            .init(
                deps.as_mut().storage,
                StrategyConfig {
                    manager: manager.clone(),
                    strategy: Strategy {
                        owner: strategy.owner.clone(),
                        action: strategy.action.clone(),
                        state: Committed {
                            contract_address: env.contract.address.clone(),
                        },
                    },
                    escrowed: HashSet::new(),
                },
            )
            .unwrap();

        ACTIVE_STRATEGY
            .save(
                deps.as_mut().storage,
                &Strategy {
                    owner: strategy.owner.clone(),
                    action: strategy.action.clone(),
                    state: Active {
                        contract_address: env.contract.address.clone(),
                    },
                },
            )
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&env.contract.address, &[]),
            StrategyExecuteMsg::Commit
        )
        .is_ok());

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&manager, &[]),
                StrategyExecuteMsg::Commit
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&owner, &[]),
                StrategyExecuteMsg::Commit
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("anyone"), &[]),
                StrategyExecuteMsg::Commit
            ),
            Err(ContractError::Unauthorized {})
        );
    }

    #[test]
    fn test_only_contract_and_owner_can_invoke_clear() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("owner");
        let manager = Addr::unchecked("manager");

        let strategy = Strategy {
            owner: owner.clone(),
            action: Action::Many(vec![]),
            state: Indexed {
                contract_address: env.contract.address.clone(),
            },
        };

        CONFIG
            .init(
                deps.as_mut().storage,
                StrategyConfig {
                    manager: manager.clone(),
                    strategy: Strategy {
                        owner: strategy.owner.clone(),
                        action: strategy.action.clone(),
                        state: Committed {
                            contract_address: env.contract.address.clone(),
                        },
                    },
                    escrowed: HashSet::new(),
                },
            )
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&env.contract.address, &[]),
            StrategyExecuteMsg::Clear {}
        )
        .is_ok());

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[]),
            StrategyExecuteMsg::Clear {}
        )
        .is_ok());

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&manager, &[]),
                StrategyExecuteMsg::Clear {}
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("anyone"), &[]),
                StrategyExecuteMsg::Clear {}
            ),
            Err(ContractError::Unauthorized {})
        );
    }

    #[test]
    fn test_cannot_execute_if_already_in_state() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        STATE
            .save(deps.as_mut().storage, &StrategyExecuteMsg::Execute)
            .unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&env.contract.address, &[]),
                StrategyExecuteMsg::Execute
            ),
            Err(ContractError::generic_err(
                "Contract is already in the Execute state, cannot execute again"
            ))
        );
    }

    #[test]
    fn test_clears_the_state() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("owner");
        let manager = Addr::unchecked("manager");

        let strategy = Strategy {
            owner: owner.clone(),
            action: Action::Many(vec![]),
            state: Indexed {
                contract_address: env.contract.address.clone(),
            },
        };

        CONFIG
            .init(
                deps.as_mut().storage,
                StrategyConfig {
                    manager: manager.clone(),
                    strategy: Strategy {
                        owner: strategy.owner.clone(),
                        action: strategy.action.clone(),
                        state: Committed {
                            contract_address: env.contract.address.clone(),
                        },
                    },
                    escrowed: HashSet::new(),
                },
            )
            .unwrap();

        STATE
            .save(deps.as_mut().storage, &StrategyExecuteMsg::Execute)
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&env.contract.address, &[]),
            StrategyExecuteMsg::Clear
        )
        .is_ok());

        assert!(!STATE.exists(deps.as_ref().storage));
    }
}
