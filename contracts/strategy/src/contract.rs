use std::{cmp::min, collections::HashSet};

use calc_rs::{
    constants::PROCESS_PAYLOAD_REPLY_ID,
    core::{Contract, ContractError, ContractResult},
    operation::{Operation, StatefulOperation},
    statistics::Statistics,
    strategy::{
        StrategyConfig, StrategyExecuteMsg, StrategyInstantiateMsg, StrategyMsgPayload,
        StrategyOperation, StrategyQueryMsg,
    },
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, BankMsg, Binary, Coin, Coins, Decimal, Deps, DepsMut, Env,
    MessageInfo, Reply, Response, StdError, StdResult, SubMsg, SubMsgResult,
};

use crate::state::{AFFILIATES, DENOMS, MANAGER, NODES, OWNER, STATS};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    if msg.contract_address != env.contract.address {
        return Err(ContractError::generic_err(format!(
            "Strategy contract address mismatch: expected {}, got {}",
            env.contract.address, msg.contract_address
        )));
    }

    MANAGER.save(deps.storage, &info.sender)?;
    OWNER.save(deps.storage, &msg.owner)?;
    AFFILIATES.save(deps.storage, &msg.affiliates)?;
    STATS.save(deps.storage, &Statistics::default())?;

    let init_msg = Contract(env.contract.address.clone()).call(
        to_json_binary(&StrategyExecuteMsg::Init(msg.nodes))?,
        vec![],
    );

    Ok(Response::new().add_message(init_msg))
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
    Ok(match msg {
        StrategyExecuteMsg::Init(nodes) => {
            if info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            let existing_denoms = DENOMS.load(deps.storage).unwrap_or(HashSet::new());
            let mut new_denoms = HashSet::with_capacity(nodes.len());

            for node in nodes.iter() {
                new_denoms.extend(node.denoms(deps.as_ref(), &env)?);
            }

            DENOMS.save(
                deps.storage,
                &new_denoms
                    .union(&existing_denoms)
                    .cloned()
                    .collect::<HashSet<String>>(),
            )?;

            NODES.init(deps, &env, nodes)?;

            let execute_actions_msg = Contract(env.contract.address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::Process {
                    operation: StrategyOperation::Execute,
                    previous: None,
                })?,
                vec![],
            );

            Response::new().add_message(execute_actions_msg)
        }
        StrategyExecuteMsg::Execute => {
            if info.sender != MANAGER.load(deps.storage)? {
                return Err(ContractError::Unauthorized {});
            }

            let execute_actions_msg = Contract(env.contract.address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::Process {
                    operation: StrategyOperation::Execute,
                    previous: None,
                })?,
                vec![],
            );

            Response::new().add_message(execute_actions_msg)
        }
        StrategyExecuteMsg::Update(nodes) => {
            if info.sender != MANAGER.load(deps.storage)? {
                return Err(ContractError::Unauthorized {});
            }

            let cancel_actions_msg = Contract(env.contract.address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::Process {
                    operation: StrategyOperation::Cancel,
                    previous: None,
                })?,
                vec![],
            );

            let init_strategy_msg = Contract(env.contract.address.clone())
                .call(to_json_binary(&StrategyExecuteMsg::Init(nodes))?, vec![]);

            Response::new()
                .add_message(cancel_actions_msg)
                .add_message(init_strategy_msg)
        }
        StrategyExecuteMsg::Withdraw(amounts) => {
            if info.sender != OWNER.load(deps.storage)? {
                return Err(ContractError::Unauthorized {});
            }

            if amounts.is_empty() {
                return Ok(Response::new());
            }

            let mut withdrawals = Coins::default();

            for amount in amounts.iter() {
                let balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), amount.denom.clone())?;

                withdrawals.add(Coin::new(
                    min(balance.amount, amount.amount),
                    amount.denom.clone(),
                ))?;
            }

            if withdrawals.is_empty() {
                return Ok(Response::new());
            }

            let affiliates = AFFILIATES.load(deps.storage)?;

            let mut affiliate_amounts = affiliates
                .iter()
                .map(|a| (a, Coins::default()))
                .collect::<Vec<_>>();

            let mut final_withdrawals = Coins::default();

            for amount in withdrawals {
                let mut working_amount = amount.amount;

                for (affiliate, ref mut amounts) in affiliate_amounts.iter_mut() {
                    let fee = amount
                        .amount
                        .mul_floor(Decimal::from_ratio(affiliate.bps, 10_000_u128));

                    amounts.add(Coin::new(fee, amount.denom.clone()))?;
                    working_amount = working_amount.saturating_sub(fee);
                }

                final_withdrawals.add(Coin::new(working_amount, amount.denom))?;
            }

            let withdrawal_msg = BankMsg::Send {
                to_address: OWNER.load(deps.storage)?.to_string(),
                amount: final_withdrawals.to_vec(),
            };

            let fee_msgs = affiliate_amounts
                .into_iter()
                .filter_map(|(affiliate, amounts)| {
                    if amounts.is_empty() {
                        None
                    } else {
                        Some(BankMsg::Send {
                            to_address: affiliate.address.to_string(),
                            amount: amounts.to_vec(),
                        })
                    }
                })
                .collect::<Vec<_>>();

            Response::new()
                .add_message(withdrawal_msg)
                .add_messages(fee_msgs)
        }
        StrategyExecuteMsg::Cancel => {
            if info.sender != MANAGER.load(deps.storage)? {
                return Err(ContractError::Unauthorized {});
            }

            let cancel_actions_msg = Contract(env.contract.address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::Process {
                    operation: StrategyOperation::Cancel,
                    previous: None,
                })?,
                vec![],
            );

            Response::new().add_message(cancel_actions_msg)
        }
        StrategyExecuteMsg::Process {
            operation,
            previous,
        } => {
            if info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            let mut next_node = if let Some(previous) = previous {
                let previous_node = NODES.load(deps.storage, previous)?;
                let next_node = NODES
                    .get_next(deps.as_ref(), &env, &operation, &previous_node)
                    .ok();

                let updated_node = previous_node.commit(deps.as_ref(), &env)?;
                NODES.save(deps.storage, &updated_node)?;

                next_node
            } else {
                NODES.load(deps.storage, 0).ok()
            };

            loop {
                if let Some(current_node) = next_node {
                    let (messages, events, node) = match operation {
                        StrategyOperation::Execute => current_node.execute(deps.as_ref(), &env),
                        StrategyOperation::Withdraw(ref desired) => {
                            current_node.withdraw(deps.as_ref(), &env, desired)?
                        }
                        StrategyOperation::Cancel => current_node.cancel(deps.as_ref(), &env)?,
                    };

                    NODES.save(deps.storage, &node)?;

                    if !messages.is_empty() {
                        break Response::new()
                            .add_submessages(
                                messages.into_iter().map(SubMsg::from).collect::<Vec<_>>(),
                            )
                            .add_submessage(SubMsg::reply_never(
                                Contract(env.contract.address.clone()).call(
                                    to_json_binary(&StrategyExecuteMsg::Process {
                                        operation,
                                        previous: Some(node.index()),
                                    })?,
                                    vec![],
                                ),
                            ))
                            .add_events(events);
                    }

                    next_node = NODES.get_next(deps.as_ref(), &env, &operation, &node).ok();
                } else {
                    break Response::new();
                }
            }
        }
    })
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
    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&StrategyConfig {
            manager: MANAGER.load(deps.storage)?,
            owner: OWNER.load(deps.storage)?,
            nodes: NODES.all(deps.storage)?,
            denoms: DENOMS.load(deps.storage)?,
        }),
        StrategyQueryMsg::Statistics {} => to_json_binary(&STATS.load(deps.storage)?),
        StrategyQueryMsg::Balances(mut include) => {
            if include.is_empty() {
                include = DENOMS.load(deps.storage)?;
            }

            let mut balances = Coins::default();

            for node in NODES.all(deps.storage)? {
                let node_balances = node.balances(deps, &env, &include)?;

                for balance in node_balances {
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
    use calc_rs::{
        actions::{
            action::Action,
            distribution::{Destination, Distribution, Recipient},
        },
        strategy::Node,
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr, Uint128,
    };

    #[test]
    fn test_only_manager_can_invoke_update() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("owner");
        let manager = Addr::unchecked("manager");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();
        OWNER.save(deps.as_mut().storage, &owner).unwrap();
        AFFILIATES.save(deps.as_mut().storage, &vec![]).unwrap();
        STATS
            .save(deps.as_mut().storage, &Statistics::default())
            .unwrap();

        let nodes = vec![Node::Action {
            action: Action::Distribute(Distribution {
                denoms: vec!["rune".to_string()],
                destinations: vec![Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Bank {
                        address: owner.clone(),
                    },
                    label: None,
                }],
            }),
            index: 0,
            next: None,
        }];

        NODES.init(deps.as_mut(), &env, nodes.clone()).unwrap();

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&owner, &[]),
                StrategyExecuteMsg::Update(nodes.clone())
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&env.contract.address, &[]),
                StrategyExecuteMsg::Update(nodes.clone())
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env,
                message_info(&Addr::unchecked("anyone"), &[]),
                StrategyExecuteMsg::Update(nodes)
            ),
            Err(ContractError::Unauthorized {})
        );
    }

    #[test]
    fn test_only_manager_can_invoke_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("owner");
        let manager = Addr::unchecked("manager");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();
        OWNER.save(deps.as_mut().storage, &owner).unwrap();
        AFFILIATES.save(deps.as_mut().storage, &vec![]).unwrap();
        STATS
            .save(deps.as_mut().storage, &Statistics::default())
            .unwrap();

        let nodes = vec![Node::Action {
            action: Action::Distribute(Distribution {
                denoms: vec!["rune".to_string()],
                destinations: vec![Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Bank {
                        address: owner.clone(),
                    },
                    label: None,
                }],
            }),
            index: 0,
            next: None,
        }];

        NODES.init(deps.as_mut(), &env, nodes).unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&manager, &[]),
            StrategyExecuteMsg::Execute
        )
        .is_ok());

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&env.contract.address, &[]),
                StrategyExecuteMsg::Execute
            ),
            Err(ContractError::Unauthorized {})
        );

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
    fn test_only_owner_can_invoke_withdraw() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("owner");
        let manager = Addr::unchecked("manager");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();
        OWNER.save(deps.as_mut().storage, &owner).unwrap();
        AFFILIATES.save(deps.as_mut().storage, &vec![]).unwrap();
        STATS
            .save(deps.as_mut().storage, &Statistics::default())
            .unwrap();

        let nodes = vec![Node::Action {
            action: Action::Distribute(Distribution {
                denoms: vec!["rune".to_string()],
                destinations: vec![Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Bank {
                        address: owner.clone(),
                    },
                    label: None,
                }],
            }),
            index: 0,
            next: None,
        }];

        NODES.init(deps.as_mut(), &env, nodes).unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[]),
            StrategyExecuteMsg::Withdraw(vec![]),
        )
        .is_ok());

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&env.contract.address, &[]),
                StrategyExecuteMsg::Withdraw(vec![])
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&manager, &[]),
                StrategyExecuteMsg::Withdraw(vec![])
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("anyone"), &[]),
                StrategyExecuteMsg::Withdraw(vec![])
            ),
            Err(ContractError::Unauthorized {})
        );
    }

    #[test]
    fn test_only_manager_can_invoke_update_cancel() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("owner");
        let manager = Addr::unchecked("manager");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();
        OWNER.save(deps.as_mut().storage, &owner).unwrap();
        AFFILIATES.save(deps.as_mut().storage, &vec![]).unwrap();
        STATS
            .save(deps.as_mut().storage, &Statistics::default())
            .unwrap();

        let nodes = vec![Node::Action {
            action: Action::Distribute(Distribution {
                denoms: vec!["rune".to_string()],
                destinations: vec![Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Bank {
                        address: owner.clone(),
                    },
                    label: None,
                }],
            }),
            index: 0,
            next: None,
        }];

        NODES.init(deps.as_mut(), &env, nodes).unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&manager, &[]),
            StrategyExecuteMsg::Cancel
        )
        .is_ok());

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&env.contract.address, &[]),
                StrategyExecuteMsg::Cancel
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&owner, &[]),
                StrategyExecuteMsg::Cancel
            ),
            Err(ContractError::Unauthorized {})
        );

        assert_eq!(
            execute(
                deps.as_mut(),
                env.clone(),
                message_info(&Addr::unchecked("anyone"), &[]),
                StrategyExecuteMsg::Cancel
            ),
            Err(ContractError::Unauthorized {})
        );
    }
}
