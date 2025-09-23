use std::hash::{DefaultHasher, Hasher};

use calc_rs::{
    constants::{BASE_FEE_BPS, MAX_TOTAL_AFFILIATE_BPS, MIN_FEE_BPS},
    core::{Contract, ContractError, ContractResult},
    manager::{
        Affiliate, ManagerConfig, ManagerExecuteMsg, ManagerQueryMsg, Strategy, StrategyStatus,
    },
    strategy::{StrategyExecuteMsg, StrategyInstantiateMsg},
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    instantiate2_address, to_json_binary, Binary, Deps, DepsMut, Env, Event, MessageInfo, Order,
    Response, StdError, StdResult, WasmMsg,
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
    deps.api
        .addr_validate(msg.fee_collector.as_str())
        .map_err(|_| ContractError::generic_err("Invalid fee collector address"))?;

    deps.querier
        .query_wasm_code_info(msg.strategy_code_id)
        .map_err(|_| {
            ContractError::generic_err(format!(
                "Invalid strategy code ID: {}",
                msg.strategy_code_id
            ))
        })?;

    CONFIG.save(deps.storage, &msg)?;
    STRATEGY_COUNTER.save(deps.storage, &0)?;

    Ok(Response::new())
}

#[cw_serde]
pub struct MigrateMsg {
    pub strategy_code_id: u64,
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: MigrateMsg) -> ContractResult {
    deps.querier
        .query_wasm_code_info(msg.strategy_code_id)
        .map_err(|_| {
            ContractError::generic_err(format!(
                "Invalid strategy code ID: {}",
                msg.strategy_code_id
            ))
        })?;

    CONFIG.update(deps.storage, |mut config| -> StdResult<ManagerConfig> {
        config.strategy_code_id = msg.strategy_code_id;
        Ok(config)
    })?;

    Ok(Response::new())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(deps: DepsMut, _env: Env, msg: ManagerConfig) -> ContractResult {
    deps.api
        .addr_validate(msg.fee_collector.as_str())
        .map_err(|_| ContractError::generic_err("Invalid fee collector address"))?;

    deps.querier
        .query_wasm_code_info(msg.strategy_code_id)
        .map_err(|_| {
            ContractError::generic_err(format!(
                "Invalid strategy code ID: {}",
                msg.strategy_code_id
            ))
        })?;

    CONFIG.save(deps.storage, &msg)?;
    Ok(Response::new())
}

const MAX_LABEL_LENGTH: usize = 100;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ManagerExecuteMsg,
) -> ContractResult {
    match msg {
        ManagerExecuteMsg::Instantiate {
            source,
            owner,
            label,
            affiliates,
            nodes,
        } => {
            let owner = owner.unwrap_or(info.sender);

            if deps.api.addr_validate(owner.as_str()).is_err() {
                return Err(ContractError::generic_err(format!(
                    "Invalid owner address: {owner}"
                )));
            }

            if label.is_empty() || label.len() > MAX_LABEL_LENGTH {
                return Err(ContractError::generic_err(format!(
                    "Strategy label must be between 1 and {MAX_LABEL_LENGTH} characters: {label}",
                )));
            }

            let total_affiliate_bps = affiliates.iter().try_fold(0, |acc, affiliate| {
                if affiliate.label.is_empty() || affiliate.label.len() > MAX_LABEL_LENGTH {
                    return Err(ContractError::generic_err(format!(
                        "Affiliate label must be between 1 and {MAX_LABEL_LENGTH} characters: {}",
                        affiliate.label
                    )));
                }

                deps.api
                    .addr_validate(affiliate.address.as_str())
                    .map_err(|_| {
                        ContractError::generic_err(format!(
                            "Invalid affiliate address: {}",
                            affiliate.address
                        ))
                    })?;

                let total = acc + affiliate.bps;

                if total > MAX_TOTAL_AFFILIATE_BPS {
                    return Err(ContractError::generic_err(format!(
                        "Total affiliate bps cannot exceed {MAX_TOTAL_AFFILIATE_BPS}, got at least {total}",
                    )));
                }

                Ok(total)
            })?;

            let config = CONFIG.load(deps.storage)?;

            let affiliates = [
                vec![Affiliate {
                    address: config.fee_collector,
                    bps: BASE_FEE_BPS
                        .saturating_sub(total_affiliate_bps)
                        .max(MIN_FEE_BPS),
                    label: "CALC".to_string(),
                }],
                affiliates,
            ]
            .concat();

            let id = STRATEGY_COUNTER.update(deps.storage, |id| Ok::<u64, StdError>(id + 1))?;

            let mut hash = DefaultHasher::new();

            hash.write(owner.as_bytes());
            hash.write(&id.to_le_bytes());
            hash.write(&env.block.height.to_le_bytes());

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
                    ContractError::generic_err(format!(
                        "Failed to instantiate contract address: {e}"
                    ))
                })?,
            )?;

            STRATEGIES.save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    id,
                    source,
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
                label,
                salt: salt.into(),
                msg: to_json_binary(&StrategyInstantiateMsg {
                    contract_address: contract_address.clone(),
                    owner: owner.clone(),
                    affiliates,
                    nodes,
                })?,
                funds: info.funds,
            };

            Ok(Response::new()
                .add_event(
                    Event::new(format!("{}/strategy.create", env!("CARGO_PKG_NAME")))
                        .add_attribute("owner", owner.as_str())
                        .add_attribute("strategy_address", contract_address.as_str()),
                )
                .add_message(init_message))
        }
        ManagerExecuteMsg::Execute { contract_address } => {
            let strategy = STRATEGIES.load(deps.storage, contract_address.clone())?;

            if strategy.status != StrategyStatus::Active {
                return Err(ContractError::generic_err("Cannot execute paused strategy"));
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

            Ok(Response::new()
                .add_event(
                    Event::new(format!("{}/strategy.execute", env!("CARGO_PKG_NAME")))
                        .add_attribute("executor", info.sender)
                        .add_attribute("strategy_address", contract_address.as_str()),
                )
                .add_message(execute_msg))
        }
        ManagerExecuteMsg::Update {
            contract_address,
            nodes,
        } => {
            let strategy = STRATEGIES.load(deps.storage, contract_address.clone())?;

            if strategy.owner != info.sender {
                return Err(ContractError::Unauthorized {});
            }

            STRATEGIES.save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            let update_msg = Contract(contract_address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::Update(nodes))?,
                info.funds,
            );

            Ok(Response::new()
                .add_event(
                    Event::new(format!("{}/strategy.update", env!("CARGO_PKG_NAME")))
                        .add_attribute("strategy_address", contract_address.as_str()),
                )
                .add_message(update_msg))
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

            let strategy_msg = Contract(contract_address.clone()).call(
                to_json_binary(&match status {
                    StrategyStatus::Active => StrategyExecuteMsg::Execute {},
                    StrategyStatus::Paused => StrategyExecuteMsg::Cancel {},
                })?,
                info.funds,
            );

            Ok(Response::new()
                .add_event(
                    Event::new(format!("{}/strategy.update-status", env!("CARGO_PKG_NAME")))
                        .add_attribute("status", status.as_str())
                        .add_attribute("strategy_address", contract_address.as_str()),
                )
                .add_message(strategy_msg))
        }
        ManagerExecuteMsg::UpdateLabel {
            contract_address,
            label,
        } => {
            if label.is_empty() || label.len() > MAX_LABEL_LENGTH {
                return Err(ContractError::generic_err(format!(
                    "Strategy label must be between 1 and {MAX_LABEL_LENGTH} characters",
                )));
            }

            let strategy = STRATEGIES.load(deps.storage, contract_address.clone())?;

            if strategy.owner != info.sender {
                return Err(ContractError::Unauthorized {});
            }

            STRATEGIES.save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    label: label.clone(),
                    ..strategy
                },
            )?;

            Ok(Response::new().add_event(
                Event::new(format!("{}/strategy.update-label", env!("CARGO_PKG_NAME")))
                    .add_attribute("label", label)
                    .add_attribute("strategy_address", contract_address.as_str()),
            ))
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: ManagerQueryMsg) -> StdResult<Binary> {
    match msg {
        ManagerQueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        ManagerQueryMsg::Strategy { address } => {
            to_json_binary(&STRATEGIES.load(deps.storage, address)?)
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

            let strategies: Result<Vec<Strategy>, StdError> = partition
                .range(
                    deps.storage,
                    None,
                    start_after
                        .map(|updated_at| Bound::exclusive(updated_at_cursor(updated_at, None))),
                    Order::Descending,
                )
                .take(limit.unwrap_or(30) as usize)
                .map(|result| result.map(|(_, strategy)| strategy))
                .collect();

            to_json_binary(&strategies?)
        }
        ManagerQueryMsg::Count {} => to_json_binary(&STRATEGY_COUNTER.load(deps.storage)?),
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
            source: None,
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
            source: None,
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
            source: None,
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
            source: None,
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
            source: None,
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
            source: None,
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
