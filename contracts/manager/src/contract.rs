use std::cmp::max;

use calc_rs::{
    constants::{BASE_FEE_BPS, MAX_TOTAL_AFFILIATE_BPS},
    core::{Contract, ContractError, ContractResult},
    manager::{
        Affiliate, ManagerConfig, ManagerExecuteMsg, ManagerQueryMsg, StrategyHandle,
        StrategyStatus,
    },
    strategy::StrategyExecuteMsg,
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response, StdError, StdResult,
};
use cw_storage_plus::Bound;

use crate::state::{strategy_store, updated_at_cursor, CONFIG, STRATEGY_COUNTER};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: ManagerConfig,
) -> ContractResult {
    CONFIG.save(deps.storage, &msg)?;
    STRATEGY_COUNTER.save(deps.storage, &0)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: ManagerConfig) -> ContractResult {
    CONFIG.save(deps.storage, &msg)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ManagerExecuteMsg,
) -> ContractResult {
    Ok(match msg {
        ManagerExecuteMsg::InstantiateStrategy {
            label,
            affiliates,
            strategy,
        } => {
            if deps.api.addr_validate(strategy.owner.as_str()).is_err() {
                return Err(ContractError::generic_err("Invalid owner address"));
            }

            if label.is_empty() || label.len() > 100 {
                return Err(ContractError::generic_err(
                    "Strategy label must be between 1 and 100 characters",
                ));
            }

            let config = CONFIG.load(deps.storage)?;

            let total_affiliate_bps = affiliates
                .iter()
                .fold(0, |acc, affiliate| acc + affiliate.bps);

            if total_affiliate_bps > MAX_TOTAL_AFFILIATE_BPS {
                return Err(ContractError::generic_err(format!(
                    "Total affiliate bps cannot exceed {MAX_TOTAL_AFFILIATE_BPS}, got {total_affiliate_bps}"
                )));
            }

            let affiliates = [
                affiliates,
                vec![Affiliate {
                    address: config.fee_collector,
                    bps: max(
                        BASE_FEE_BPS.saturating_sub(10),
                        BASE_FEE_BPS.saturating_sub(total_affiliate_bps),
                    ),
                    label: "CALC".to_string(),
                }],
            ]
            .concat();

            let init_message = strategy
                .with_affiliates(&affiliates)?
                .add_to_index(
                    &mut deps,
                    &env,
                    config.strategy_code_id,
                    label.clone(),
                    |storage, strategy| {
                        let id =
                            STRATEGY_COUNTER.update(storage, |id| Ok::<u64, StdError>(id + 1))?;

                        strategy_store().save(
                            storage,
                            strategy.state.contract_address.clone(),
                            &StrategyHandle {
                                id,
                                owner: strategy.owner.clone(),
                                contract_address: strategy.state.contract_address.clone(),
                                created_at: env.block.time.seconds(),
                                updated_at: env.block.time.seconds(),
                                label,
                                status: StrategyStatus::Active,
                                affiliates,
                            },
                        )
                    },
                )?
                .instantiate_msg(info)?;

            Response::default().add_message(init_message)
        }
        ManagerExecuteMsg::ExecuteStrategy { contract_address } => {
            let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            if strategy.status != StrategyStatus::Active {
                return Err(ContractError::generic_err(
                    "Cannot execute strategy that is not active",
                ));
            }

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &StrategyHandle {
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            let execute_msg = Contract(contract_address.clone())
                .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, info.funds);

            Response::default().add_message(execute_msg)
        }
        ManagerExecuteMsg::UpdateStrategy {
            contract_address,
            update,
        } => {
            let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            if strategy.owner != info.sender {
                return Err(ContractError::Unauthorized {});
            }

            let update_msg = update
                .with_affiliates(&strategy.affiliates)?
                .update_index(&mut deps, contract_address.clone(), |storage| {
                    strategy_store().save(
                        storage,
                        contract_address,
                        &StrategyHandle {
                            updated_at: env.block.time.seconds(),
                            ..strategy
                        },
                    )
                })?
                .update_msg(info)?;

            Response::default().add_message(update_msg)
        }
        ManagerExecuteMsg::UpdateStrategyStatus {
            contract_address,
            status,
        } => {
            let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            if strategy.owner != info.sender {
                return Err(ContractError::Unauthorized {});
            }

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &StrategyHandle {
                    status: status.clone(),
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            let update_status_msg = Contract(contract_address).call(
                to_json_binary(&StrategyExecuteMsg::UpdateStatus(status))?,
                info.funds,
            );

            Response::default().add_message(update_status_msg)
        }
    })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: ManagerQueryMsg) -> StdResult<Binary> {
    match msg {
        ManagerQueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        ManagerQueryMsg::Strategy { address } => {
            to_json_binary(&strategy_store().load(deps.storage, address.clone())?)
        }
        ManagerQueryMsg::Strategies {
            owner,
            status,
            start_after,
            limit,
        } => {
            let partition = match owner {
                Some(owner) => match status {
                    Some(status) => strategy_store()
                        .idx
                        .owner_status_updated_at
                        .prefix((owner, status as u8)),
                    None => strategy_store().idx.owner_updated_at.prefix(owner),
                },
                None => match status {
                    Some(status) => strategy_store().idx.status_updated_at.prefix(status as u8),
                    None => strategy_store().idx.updated_at.prefix(()),
                },
            };

            let strategies = partition
                .range(
                    deps.storage,
                    None,
                    start_after
                        .map(|updated_at| Bound::exclusive(updated_at_cursor(updated_at, None))),
                    Order::Descending,
                )
                .take(match limit {
                    Some(limit) => match limit {
                        0..=30 => limit as usize,
                        _ => 30,
                    },
                    None => 30,
                })
                .flat_map(|result| result.map(|(_, strategy)| strategy))
                .collect::<Vec<StrategyHandle>>();

            to_json_binary(&strategies)
        }
    }
}

#[cfg(test)]
mod tests {
    use calc_rs::{
        actions::action::Action,
        strategy::{Json, Strategy},
    };
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr,
    };

    use super::*;

    #[test]
    fn test_cannot_execute_inactive_strategy() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = message_info(&deps.api.addr_make("anyone"), &[]);

        let strategy = StrategyHandle {
            id: 1,
            owner: info.sender.clone(),
            contract_address: Addr::unchecked("contract"),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds(),
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Archived,
            affiliates: vec![],
        };

        strategy_store()
            .save(
                deps.as_mut().storage,
                strategy.contract_address.clone(),
                &strategy,
            )
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            ManagerExecuteMsg::ExecuteStrategy {
                contract_address: strategy.contract_address.clone(),
            },
        )
        .is_err());

        strategy_store()
            .save(
                deps.as_mut().storage,
                strategy.contract_address.clone(),
                &StrategyHandle {
                    status: StrategyStatus::Active,
                    ..strategy.clone()
                },
            )
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env,
            info,
            ManagerExecuteMsg::ExecuteStrategy {
                contract_address: strategy.contract_address.clone(),
            },
        )
        .is_ok());
    }

    #[test]
    fn test_only_owner_can_update_strategy() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = message_info(&deps.api.addr_make("owner"), &[]);

        let strategy = StrategyHandle {
            id: 1,
            owner: info.sender.clone(),
            contract_address: Addr::unchecked("contract"),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds(),
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Archived,
            affiliates: vec![],
        };

        strategy_store()
            .save(
                deps.as_mut().storage,
                strategy.contract_address.clone(),
                &strategy,
            )
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            ManagerExecuteMsg::UpdateStrategy {
                contract_address: strategy.contract_address.clone(),
                update: Strategy {
                    owner: info.sender.clone(),
                    action: Action::Many(vec![]),
                    state: Json
                }
            },
        )
        .is_ok());

        let not_owner = deps.api.addr_make("not-owner");

        assert!(execute(
            deps.as_mut(),
            env,
            message_info(&not_owner, &[]),
            ManagerExecuteMsg::UpdateStrategy {
                contract_address: strategy.contract_address.clone(),
                update: Strategy {
                    owner: info.sender.clone(),
                    action: Action::Many(vec![]),
                    state: Json
                }
            },
        )
        .is_err());
    }

    #[test]
    fn test_only_owner_can_update_strategy_status() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = message_info(&deps.api.addr_make("owner"), &[]);

        let strategy = StrategyHandle {
            id: 1,
            owner: info.sender.clone(),
            contract_address: Addr::unchecked("contract"),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds(),
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Archived,
            affiliates: vec![],
        };

        strategy_store()
            .save(
                deps.as_mut().storage,
                strategy.contract_address.clone(),
                &strategy,
            )
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            ManagerExecuteMsg::UpdateStrategyStatus {
                contract_address: strategy.contract_address.clone(),
                status: StrategyStatus::Active
            }
        )
        .is_ok());

        let not_owner = deps.api.addr_make("not-owner");

        assert!(execute(
            deps.as_mut(),
            env,
            message_info(&not_owner, &[]),
            ManagerExecuteMsg::UpdateStrategyStatus {
                contract_address: strategy.contract_address.clone(),
                status: StrategyStatus::Paused
            }
        )
        .is_err());
    }

    #[test]
    fn test_execute_strategy_updates_updated_at() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = message_info(&deps.api.addr_make("owner"), &[]);

        let strategy = StrategyHandle {
            id: 1,
            owner: info.sender.clone(),
            contract_address: Addr::unchecked("contract"),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds() - 1000,
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Active,
            affiliates: vec![],
        };

        strategy_store()
            .save(
                deps.as_mut().storage,
                strategy.contract_address.clone(),
                &strategy,
            )
            .unwrap();

        execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            ManagerExecuteMsg::ExecuteStrategy {
                contract_address: strategy.contract_address.clone(),
            },
        )
        .unwrap();

        let updated_strategy = strategy_store()
            .load(deps.as_mut().storage, strategy.contract_address.clone())
            .unwrap();

        assert!(updated_strategy.updated_at == env.block.time.seconds());
        assert!(updated_strategy.updated_at > strategy.updated_at);
    }

    #[test]
    fn test_update_strategy_updates_updated_at() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = message_info(&deps.api.addr_make("owner"), &[]);

        let strategy = StrategyHandle {
            id: 1,
            owner: info.sender.clone(),
            contract_address: Addr::unchecked("contract"),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds() - 1000,
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Active,
            affiliates: vec![],
        };

        strategy_store()
            .save(
                deps.as_mut().storage,
                strategy.contract_address.clone(),
                &strategy,
            )
            .unwrap();

        execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            ManagerExecuteMsg::UpdateStrategy {
                contract_address: strategy.contract_address.clone(),
                update: Strategy {
                    owner: info.sender.clone(),
                    action: Action::Many(vec![]),
                    state: Json,
                },
            },
        )
        .unwrap();

        let updated_strategy = strategy_store()
            .load(deps.as_mut().storage, strategy.contract_address.clone())
            .unwrap();

        assert!(updated_strategy.updated_at == env.block.time.seconds());
        assert!(updated_strategy.updated_at > strategy.updated_at);
    }

    #[test]
    fn test_update_status_updates_status_and_updated_at() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = message_info(&deps.api.addr_make("owner"), &[]);

        let strategy = StrategyHandle {
            id: 1,
            owner: info.sender.clone(),
            contract_address: Addr::unchecked("contract"),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds() - 1000,
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Active,
            affiliates: vec![],
        };

        strategy_store()
            .save(
                deps.as_mut().storage,
                strategy.contract_address.clone(),
                &strategy,
            )
            .unwrap();

        execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            ManagerExecuteMsg::UpdateStrategyStatus {
                contract_address: strategy.contract_address.clone(),
                status: StrategyStatus::Paused,
            },
        )
        .unwrap();

        let updated_strategy = strategy_store()
            .load(deps.as_mut().storage, strategy.contract_address.clone())
            .unwrap();

        assert!(updated_strategy.status == StrategyStatus::Paused);
        assert!(updated_strategy.updated_at == env.block.time.seconds());
        assert!(updated_strategy.updated_at > strategy.updated_at);
    }
}
