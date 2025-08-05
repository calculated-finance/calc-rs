use std::{
    cmp::max,
    hash::{DefaultHasher, Hasher},
};

use calc_rs::{
    constants::{BASE_FEE_BPS, MAX_TOTAL_AFFILIATE_BPS},
    core::{Contract, ContractError, ContractResult},
    manager::{
        Affiliate, ManagerConfig, ManagerExecuteMsg, ManagerQueryMsg, Strategy, StrategyStatus,
    },
    strategy::{StrategyExecuteMsg, StrategyInstantiateMsg},
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    instantiate2_address, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdError, StdResult, WasmMsg,
};
use cw_storage_plus::Bound;

use crate::state::{updated_at_cursor, CONFIG, STRATEGIES, STRATEGY_COUNTER};

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

pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> ContractResult {
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(deps: DepsMut, _env: Env, msg: ManagerConfig) -> ContractResult {
    CONFIG.save(deps.storage, &msg)?;
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ManagerExecuteMsg,
) -> ContractResult {
    Ok(match msg {
        ManagerExecuteMsg::Instantiate {
            owner,
            label,
            affiliates,
            nodes,
        } => {
            if deps.api.addr_validate(owner.as_str()).is_err() {
                return Err(ContractError::generic_err("Invalid owner address"));
            }

            if label.is_empty() || label.len() > 100 {
                return Err(ContractError::generic_err(
                    "Strategy label must be between 1 and 100 characters",
                ));
            }

            let total_affiliate_bps = affiliates
                .iter()
                .fold(0, |acc, affiliate| acc + affiliate.bps);

            if total_affiliate_bps > MAX_TOTAL_AFFILIATE_BPS {
                return Err(ContractError::generic_err(format!(
                    "Total affiliate bps cannot exceed {MAX_TOTAL_AFFILIATE_BPS}, got {total_affiliate_bps}"
                )));
            }

            let config = CONFIG.load(deps.storage)?;

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

            let id = STRATEGY_COUNTER.update(deps.storage, |id| Ok::<u64, StdError>(id + 1))?;
            let salt_data = to_json_binary(&(owner.to_string(), id, env.block.height))?;
            let mut hash = DefaultHasher::new();
            hash.write(salt_data.as_slice());
            let salt = hash.finish().to_le_bytes();

            let contract_address = deps.api.addr_humanize(
                &instantiate2_address(
                    deps.querier
                        .query_wasm_code_info(config.strategy_code_id)?
                        .checksum
                        .as_slice(),
                    &deps.api.addr_canonicalize(env.contract.address.as_str())?,
                    &salt,
                )
                .map_err(|e| {
                    StdError::generic_err(format!("Failed to instantiate contract address: {e}"))
                })?,
            )?;

            STRATEGIES.save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    id,
                    owner: owner.clone(),
                    contract_address: contract_address.clone(),
                    created_at: env.block.time.seconds(),
                    updated_at: env.block.time.seconds(),
                    label: label.clone(),
                    status: StrategyStatus::Active,
                },
            )?;

            let init_message = WasmMsg::Instantiate2 {
                admin: Some(owner.to_string()),
                code_id: config.strategy_code_id,
                label: label,
                salt: salt.into(),
                msg: to_json_binary(&StrategyInstantiateMsg {
                    contract_address,
                    owner,
                    affiliates,
                    nodes,
                })?,
                funds: info.funds,
            };

            Response::default().add_message(init_message)
        }
        ManagerExecuteMsg::Execute { contract_address } => {
            let strategy = STRATEGIES.load(deps.storage, contract_address.clone())?;

            if strategy.status != StrategyStatus::Active {
                return Err(ContractError::generic_err(
                    "Cannot execute strategy that is not active",
                ));
            }

            STRATEGIES.save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            let execute_msg = Contract(contract_address.clone())
                .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, info.funds);

            Response::default().add_message(execute_msg)
        }
        ManagerExecuteMsg::Update {
            contract_address,
            nodes,
            label,
        } => {
            let strategy = STRATEGIES.load(deps.storage, contract_address.clone())?;

            if strategy.owner != info.sender {
                return Err(ContractError::Unauthorized {});
            }

            if let Some(label) = &label {
                if label.is_empty() || label.len() > 100 {
                    return Err(ContractError::generic_err(
                        "Strategy label must be between 1 and 100 characters",
                    ));
                }
            }

            STRATEGIES.save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    updated_at: env.block.time.seconds(),
                    label: label.unwrap_or(strategy.label),
                    ..strategy
                },
            )?;

            let update_msg = Contract(contract_address).call(
                to_json_binary(&StrategyExecuteMsg::Update(nodes))?,
                info.funds,
            );

            Response::default().add_message(update_msg)
        }
        ManagerExecuteMsg::UpdateStatus {
            contract_address,
            status,
        } => {
            let strategy = STRATEGIES.load(deps.storage, contract_address.clone())?;

            if strategy.owner != info.sender {
                return Err(ContractError::Unauthorized {});
            }

            STRATEGIES.save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    status: status.clone(),
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            let strategy_msg = Contract(contract_address).call(
                to_json_binary(&match status {
                    StrategyStatus::Active => StrategyExecuteMsg::Execute,
                    StrategyStatus::Paused => StrategyExecuteMsg::Cancel,
                })?,
                info.funds,
            );

            Response::default().add_message(strategy_msg)
        }
    })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: ManagerQueryMsg) -> StdResult<Binary> {
    match msg {
        ManagerQueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        ManagerQueryMsg::Strategy { address } => {
            to_json_binary(&STRATEGIES.load(deps.storage, address.clone())?)
        }
        ManagerQueryMsg::Strategies {
            owner,
            status,
            start_after,
            limit,
        } => {
            let partition = match owner {
                Some(owner) => match status {
                    Some(status) => STRATEGIES
                        .idx
                        .owner_status_updated_at
                        .prefix((owner, status as u8)),
                    None => STRATEGIES.idx.owner_updated_at.prefix(owner),
                },
                None => match status {
                    Some(status) => STRATEGIES.idx.status_updated_at.prefix(status as u8),
                    None => STRATEGIES.idx.updated_at.prefix(()),
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
                .collect::<Vec<Strategy>>();

            to_json_binary(&strategies)
        }
    }
}

#[cfg(test)]
mod tests {
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

        let strategy = Strategy {
            id: 1,
            owner: info.sender.clone(),
            contract_address: Addr::unchecked("contract"),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds(),
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Paused,
        };

        STRATEGIES
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
            ManagerExecuteMsg::Execute {
                contract_address: strategy.contract_address.clone(),
            },
        )
        .is_err());

        STRATEGIES
            .save(
                deps.as_mut().storage,
                strategy.contract_address.clone(),
                &Strategy {
                    status: StrategyStatus::Active,
                    ..strategy.clone()
                },
            )
            .unwrap();

        assert!(execute(
            deps.as_mut(),
            env,
            info,
            ManagerExecuteMsg::Execute {
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

        let strategy = Strategy {
            id: 1,
            owner: info.sender.clone(),
            contract_address: Addr::unchecked("contract"),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds(),
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Paused,
        };

        STRATEGIES
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
            ManagerExecuteMsg::Update {
                contract_address: strategy.contract_address.clone(),
                nodes: vec![],
                label: None,
            },
        )
        .is_ok());

        let not_owner = deps.api.addr_make("not-owner");

        assert!(execute(
            deps.as_mut(),
            env,
            message_info(&not_owner, &[]),
            ManagerExecuteMsg::Update {
                contract_address: strategy.contract_address.clone(),
                nodes: vec![],
                label: None,
            },
        )
        .is_err());
    }

    #[test]
    fn test_only_owner_can_update_strategy_status() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = message_info(&deps.api.addr_make("owner"), &[]);

        let strategy = Strategy {
            id: 1,
            owner: info.sender.clone(),
            contract_address: Addr::unchecked("contract"),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds(),
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Paused,
        };

        STRATEGIES
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
            ManagerExecuteMsg::UpdateStatus {
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
            ManagerExecuteMsg::UpdateStatus {
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

        let strategy = Strategy {
            id: 1,
            owner: info.sender.clone(),
            contract_address: Addr::unchecked("contract"),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds() - 1000,
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Active,
        };

        STRATEGIES
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
            ManagerExecuteMsg::Execute {
                contract_address: strategy.contract_address.clone(),
            },
        )
        .unwrap();

        let updated_strategy = STRATEGIES
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

        let strategy = Strategy {
            id: 1,
            owner: info.sender.clone(),
            contract_address: Addr::unchecked("contract"),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds() - 1000,
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Active,
        };

        STRATEGIES
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
            ManagerExecuteMsg::Update {
                contract_address: strategy.contract_address.clone(),
                nodes: vec![],
                label: None,
            },
        )
        .unwrap();

        let updated_strategy = STRATEGIES
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

        let strategy = Strategy {
            id: 1,
            owner: info.sender.clone(),
            contract_address: Addr::unchecked("contract"),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds() - 1000,
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Active,
        };

        STRATEGIES
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
            ManagerExecuteMsg::UpdateStatus {
                contract_address: strategy.contract_address.clone(),
                status: StrategyStatus::Paused,
            },
        )
        .unwrap();

        let updated_strategy = STRATEGIES
            .load(deps.as_mut().storage, strategy.contract_address.clone())
            .unwrap();

        assert!(updated_strategy.status == StrategyStatus::Paused);
        assert!(updated_strategy.updated_at == env.block.time.seconds());
        assert!(updated_strategy.updated_at > strategy.updated_at);
    }
}
