use std::collections::HashSet;

use calc_rs::{
    actions::operation::{Operation, StatefulOperation},
    constants::{MAX_STRATEGY_SIZE, PROCESS_PAYLOAD_REPLY_ID},
    core::{Contract, ContractError, ContractResult},
    manager::StrategyStatus,
    strategy::{
        OpNode, Indexed, Strategy, StrategyConfig, StrategyExecuteMsg, StrategyMsgPayload,
        StrategyOperation, StrategyQueryMsg,
    },
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, BankMsg, Binary, Coins, Deps, DepsMut, Env, MessageInfo, Reply,
    Response, StdError, StdResult, SubMsg, SubMsgResult,
};

use crate::state::{ACTIONS, CONFIG, DENOMS, ESCROWED, STATS};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    strategy: Strategy<Indexed>,
) -> ContractResult {
    if strategy.state.contract_address != env.contract.address {
        return Err(ContractError::generic_err(format!(
            "Strategy contract address mismatch: expected {}, got {}",
            env.contract.address, strategy.state.contract_address
        )));
    }

    if strategy.actions.is_empty() {
        return Err(ContractError::generic_err(
            "Strategy must have at least one action",
        ));
    }

    if strategy.size() > MAX_STRATEGY_SIZE {
        return Err(ContractError::generic_err(format!(
            "Strategy size exceeds maximum limit of {}",
            MAX_STRATEGY_SIZE
        )));
    }

    let denoms = strategy.denoms(deps.as_ref(), &env)?;
    let escrowed = strategy.escrowed(deps.as_ref(), &env)?;

    CONFIG.init(
        deps.storage,
        StrategyConfig {
            manager: info.sender.clone(),
            strategy,
            denoms,
            escrowed,
        },
    )?;

    let init_actions_msg = Contract(env.contract.address.clone()).call(
        to_json_binary(&StrategyExecuteMsg::ProcessNext {
            operation: StrategyOperation::Init,
            previous: None,
        })?,
        vec![],
    );

    let execute_actions_msg = Contract(env.contract.address.clone()).call(
        to_json_binary(&StrategyExecuteMsg::ProcessNext {
            operation: StrategyOperation::Execute,
            previous: None,
        })?,
        vec![],
    );

    Ok(Response::new()
        .add_message(init_actions_msg)
        .add_message(execute_actions_msg))
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
    msg: StrategyExecuteMsg,
) -> ContractResult {
    let response = match msg {
        StrategyExecuteMsg::Execute => {
            let config = CONFIG.load(deps.storage)?;

            if info.sender != config.manager && info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            let execute_actions_msg = Contract(env.contract.address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::ProcessNext {
                    operation: StrategyOperation::Execute,
                    previous: None,
                })?,
                vec![],
            );

            Response::new().add_message(execute_actions_msg)
        }

        StrategyExecuteMsg::Update(strategy) => {
            let config = CONFIG.load(deps.storage)?;

            if info.sender != config.manager {
                return Err(ContractError::Unauthorized {});
            }

            let cancel_actions_msg = Contract(env.contract.address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::ProcessNext {
                    operation: StrategyOperation::Cancel,
                    previous: None,
                })?,
                vec![],
            );

            let init_strategy_msg = Contract(env.contract.address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::Process {
                    operation: StrategyOperation::Init,
                    strategy,
                })?,
                vec![],
            );

            Response::new()
                .add_message(cancel_actions_msg)
                .add_message(init_strategy_msg)
        }
        StrategyExecuteMsg::Withdraw {
            mut denoms,
            from_actions,
        } => {
            let config = CONFIG.load(deps.storage)?;

            if info.sender != config.strategy.owner && info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            for denom in denoms.iter() {
                if config.escrowed.contains(denom) {
                    return Err(ContractError::generic_err(format!(
                        "Cannot withdraw escrowed denom: {denom}"
                    )));
                }
            }

            if denoms.is_empty() {
                denoms = DENOMS
                    .load(deps.storage)?
                    .difference(&ESCROWED.load(deps.storage)?)
                    .cloned()
                    .collect::<HashSet<_>>();
            }

            if denoms.is_empty() {
                return Ok(Response::new());
            }

            if from_actions {
                let withdraw_from_actions_msg = Contract(env.contract.address.clone()).call(
                    to_json_binary(&StrategyExecuteMsg::ProcessNext {
                        operation: StrategyOperation::Withdraw(denoms.clone()),
                        previous: None,
                    })?,
                    vec![],
                );

                let withdraw_after_actions_msg = Contract(env.contract.address.clone()).call(
                    to_json_binary(&StrategyExecuteMsg::Withdraw {
                        denoms,
                        from_actions: false,
                    })?,
                    vec![],
                );

                return Ok(Response::new()
                    .add_message(withdraw_from_actions_msg)
                    .add_message(withdraw_after_actions_msg));
            }

            let owner = config.strategy.owner.to_string();

            let mut withdrawals = Coins::default();

            for denom in denoms.iter() {
                let balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), denom.clone())?;

                withdrawals.add(balance.clone())?;
            }

            if withdrawals.is_empty() {
                return Ok(Response::new());
            }

            let withdrawal_from_bank_msg = BankMsg::Send {
                to_address: owner,
                amount: withdrawals.to_vec(),
            };

            Response::new().add_message(withdrawal_from_bank_msg)
        }
        StrategyExecuteMsg::UpdateStatus(status) => {
            let config = CONFIG.load(deps.storage)?;

            if info.sender != config.manager {
                return Err(ContractError::Unauthorized {});
            }

            match status {
                StrategyStatus::Active => {
                    let execute_actions_msg = Contract(env.contract.address.clone()).call(
                        to_json_binary(&StrategyExecuteMsg::ProcessNext {
                            operation: StrategyOperation::Execute,
                            previous: None,
                        })?,
                        vec![],
                    );

                    Response::new().add_message(execute_actions_msg)
                }
                StrategyStatus::Paused | StrategyStatus::Archived => {
                    let cancel_actions_msg = Contract(env.contract.address.clone()).call(
                        to_json_binary(&StrategyExecuteMsg::ProcessNext {
                            operation: StrategyOperation::Cancel,
                            previous: None,
                        })?,
                        vec![],
                    );

                    Response::new().add_message(cancel_actions_msg)
                }
            }
        }
        StrategyExecuteMsg::Process {
            operation,
            strategy,
        } => {
            let config = CONFIG.load(deps.storage)?;

            let denoms = strategy.denoms(deps.as_ref(), &env)?;
            let escrowed = strategy.escrowed(deps.as_ref(), &env)?;

            CONFIG.update(
                deps.storage,
                StrategyConfig {
                    manager: config.manager,
                    strategy,
                    denoms,
                    escrowed,
                },
            )?;

            let process_actions_msg = Contract(env.contract.address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::ProcessNext {
                    operation,
                    previous: None,
                })?,
                vec![],
            );

            Response::new().add_message(process_actions_msg)
        }
        StrategyExecuteMsg::ProcessNext {
            operation,
            previous,
        } => {
            if info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            if let Some(previous) = previous.clone() {
                let updated_action = previous.operation.commit(deps.as_ref(), &env)?;

                ACTIONS.save(
                    deps.storage,
                    &OpNode {
                        operation: updated_action,
                        ..previous
                    },
                )?;
            }

            let mut action_node = ACTIONS.get_next(deps.as_ref(), &env, previous)?;

            loop {
                if let Some(actual_action_node) = action_node.clone() {
                    let (messages, events, action) = match operation {
                        StrategyOperation::Init => {
                            actual_action_node.operation.init(deps.as_ref(), &env)?
                        }
                        StrategyOperation::Execute => {
                            actual_action_node.operation.execute(deps.as_ref(), &env)
                        }
                        StrategyOperation::Withdraw(ref desired) => actual_action_node
                            .operation
                            .withdraw(deps.as_ref(), &env, desired)?,
                        StrategyOperation::Cancel => {
                            actual_action_node.operation.cancel(deps.as_ref(), &env)?
                        }
                    };

                    if !messages.is_empty() {
                        break Response::new()
                            .add_submessages(
                                messages.into_iter().map(SubMsg::from).collect::<Vec<_>>(),
                            )
                            .add_submessage(SubMsg::reply_never(
                                Contract(env.contract.address.clone()).call(
                                    to_json_binary(&StrategyExecuteMsg::ProcessNext {
                                        operation,
                                        previous: Some(OpNode {
                                            operation,
                                            ..actual_action_node
                                        }),
                                    })?,
                                    vec![],
                                ),
                            ))
                            .add_events(events);
                    }

                    action_node = ACTIONS.get_next(deps.as_ref(), &env, action_node)?;
                } else {
                    break Response::new();
                }
            }
        }
    };

    Ok(response)
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
        StrategyQueryMsg::Balances(mut include) => {
            if include.is_empty() {
                include = DENOMS.load(deps.storage)?;
            }

            let mut balances = Coins::default();

            for action in ACTIONS.load(deps.storage)? {
                let action_balances = action.operation.balances(deps, &env, &include)?;

                for balance in action_balances {
                    balances.add(balance)?;
                }
            }

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
        actions::{
            action::Action,
            swaps::swap::{Swap, SwapAmountAdjustment},
        },
        strategy::{Indexed, Strategy},
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr, Coin,
    };

    #[test]
    fn test_only_manager_can_invoke_update() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("owner");
        let manager = Addr::unchecked("manager");

        let strategy = Strategy {
            owner: owner.clone(),
            actions: vec![Action::Swap(Swap {
                swap_amount: Coin::new(1000u128, "rune"),
                minimum_receive_amount: Coin::new(100u128, "rune"),
                maximum_slippage_bps: 100,
                adjustment: SwapAmountAdjustment::Fixed,
                routes: vec![],
            })],
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
                        actions: strategy.actions.clone(),
                        state: Indexed {
                            contract_address: env.contract.address.clone(),
                        },
                    },
                    denoms: HashSet::new(),
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
            actions: vec![Action::Swap(Swap {
                swap_amount: Coin::new(1000u128, "rune"),
                minimum_receive_amount: Coin::new(100u128, "rune"),
                maximum_slippage_bps: 100,
                adjustment: SwapAmountAdjustment::Fixed,
                routes: vec![],
            })],
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
                        actions: strategy.actions.clone(),
                        state: Indexed {
                            contract_address: env.contract.address.clone(),
                        },
                    },
                    denoms: HashSet::new(),
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
            actions: vec![Action::Swap(Swap {
                swap_amount: Coin::new(1000u128, "rune"),
                minimum_receive_amount: Coin::new(100u128, "rune"),
                maximum_slippage_bps: 100,
                adjustment: SwapAmountAdjustment::Fixed,
                routes: vec![],
            })],
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
                        actions: strategy.actions.clone(),
                        state: Indexed {
                            contract_address: env.contract.address.clone(),
                        },
                    },
                    denoms: HashSet::new(),
                    escrowed: HashSet::new(),
                },
            )
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[]),
            StrategyExecuteMsg::Withdraw {
                denoms: HashSet::new(),
                from_actions: true
            },
        )
        .is_ok());

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&env.contract.address, &[]),
            StrategyExecuteMsg::Withdraw {
                denoms: HashSet::new(),
                from_actions: true
            },
        )
        .is_ok());

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&manager, &[]),
                StrategyExecuteMsg::Withdraw {
                    denoms: HashSet::new(),
                    from_actions: true
                },
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("anyone"), &[]),
                StrategyExecuteMsg::Withdraw {
                    denoms: HashSet::new(),
                    from_actions: true
                },
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
            actions: vec![Action::Swap(Swap {
                swap_amount: Coin::new(1000u128, "rune"),
                minimum_receive_amount: Coin::new(100u128, "rune"),
                maximum_slippage_bps: 100,
                adjustment: SwapAmountAdjustment::Fixed,
                routes: vec![],
            })],
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
                        actions: strategy.actions.clone(),
                        state: Indexed {
                            contract_address: env.contract.address.clone(),
                        },
                    },
                    denoms: HashSet::new(),
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
}
