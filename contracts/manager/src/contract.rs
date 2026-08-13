use std::{
    collections::BTreeMap,
    hash::{DefaultHasher, Hasher},
};

use calc_rs::{
    constants::{BASE_FEE_BPS, MAX_TOTAL_AFFILIATE_BPS, MIN_FEE_BPS},
    core::{Contract, ContractError, ContractResult},
    manager::{
        Affiliate, ManagerConfig, ManagerExecuteMsg, ManagerQueryMsg, ManagerSudoMsg, NodeStatus,
        RegisteredNode, Strategy, StrategyStatus,
    },
    strategy::{
        Node, StrategyConfig, StrategyExecuteMsg, StrategyInstantiateMsg, StrategyQueryMsg,
    },
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    instantiate2_address, to_json_binary, Binary, Deps, DepsMut, Env, Event, MessageInfo, Order,
    Response, StdError, StdResult, WasmMsg,
};
use cw_storage_plus::{Bound, Item};

use crate::state::{updated_at_cursor, CONFIG, NODES, NODE_COUNTER, STRATEGIES, STRATEGY_COUNTER};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: ManagerConfig,
) -> ContractResult {
    deps.api
        .addr_validate(msg.owner.as_str())
        .map_err(|_| ContractError::generic_err("Invalid owner address"))?;

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
    NODE_COUNTER.save(deps.storage, &0)?;

    Ok(Response::new())
}

#[cw_serde]
pub struct MigrateMsg {
    pub owner: cosmwasm_std::Addr,
    pub strategy_code_id: u64,
}

#[cw_serde]
struct MigrationManagerConfig {
    pub owner: Option<cosmwasm_std::Addr>,
    pub fee_collector: cosmwasm_std::Addr,
    pub strategy_code_id: u64,
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: MigrateMsg) -> ContractResult {
    deps.api
        .addr_validate(msg.owner.as_str())
        .map_err(|_| ContractError::generic_err("Invalid owner address"))?;

    deps.querier
        .query_wasm_code_info(msg.strategy_code_id)
        .map_err(|_| {
            ContractError::generic_err(format!(
                "Invalid strategy code ID: {}",
                msg.strategy_code_id
            ))
        })?;

    let current = Item::<MigrationManagerConfig>::new("config").load(deps.storage)?;
    let owner = match current.owner {
        Some(owner) if owner != msg.owner => {
            return Err(ContractError::generic_err("Manager owner is immutable"));
        }
        Some(owner) => owner,
        None => msg.owner,
    };
    CONFIG.save(
        deps.storage,
        &ManagerConfig {
            owner,
            fee_collector: current.fee_collector,
            strategy_code_id: msg.strategy_code_id,
        },
    )?;
    if NODE_COUNTER.may_load(deps.storage)?.is_none() {
        NODE_COUNTER.save(deps.storage, &0)?;
    }

    Ok(Response::new())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn sudo(deps: DepsMut, _env: Env, msg: ManagerSudoMsg) -> ContractResult {
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

    CONFIG.update(deps.storage, |config| -> StdResult<_> {
        Ok(ManagerConfig {
            owner: config.owner,
            fee_collector: msg.fee_collector,
            strategy_code_id: msg.strategy_code_id,
        })
    })?;
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
            validate_external_nodes(deps.as_ref(), &nodes, None)?;

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

            if nodes.iter().any(|node| node.external().is_some())
                || NODES
                    .range(deps.storage, None, None, Order::Ascending)
                    .next()
                    .is_some()
            {
                let existing_counts =
                    match deps.querier.query_wasm_smart::<Vec<cosmwasm_std::Addr>>(
                        &contract_address,
                        &StrategyQueryMsg::ExternalNodeReferences {},
                    ) {
                        Ok(addresses) => external_address_counts(&addresses),
                        Err(_) => {
                            let existing = deps.querier.query_wasm_smart::<StrategyConfig>(
                                &contract_address,
                                &StrategyQueryMsg::Config {},
                            )?;
                            external_counts(&existing.nodes)
                        }
                    };
                validate_external_nodes(deps.as_ref(), &nodes, Some(&existing_counts))?;
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
                    StrategyStatus::Paused | StrategyStatus::Archived => {
                        StrategyExecuteMsg::Cancel {}
                    }
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
        ManagerExecuteMsg::DeployNode {
            code_id,
            label,
            instantiate_msg,
            size_weight,
        } => {
            let config = CONFIG.load(deps.storage)?;
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }
            validate_node_weight(size_weight)?;
            if label.is_empty() || label.len() > MAX_LABEL_LENGTH {
                return Err(ContractError::generic_err(format!(
                    "Node label must be between 1 and {MAX_LABEL_LENGTH} characters"
                )));
            }

            let code_info = deps.querier.query_wasm_code_info(code_id).map_err(|_| {
                ContractError::generic_err(format!("Invalid node code ID: {code_id}"))
            })?;
            let counter = NODE_COUNTER.update(deps.storage, |id| Ok::<_, StdError>(id + 1))?;
            let salt = counter.to_le_bytes();
            let address = deps.api.addr_humanize(
                &instantiate2_address(
                    code_info.checksum.as_slice(),
                    &deps.api.addr_canonicalize(env.contract.address.as_str())?,
                    &salt,
                )
                .map_err(|e| {
                    ContractError::generic_err(format!("Failed to instantiate node address: {e}"))
                })?,
            )?;

            NODES.save(
                deps.storage,
                address.clone(),
                &RegisteredNode {
                    address: address.clone(),
                    code_id,
                    checksum: cosmwasm_std::HexBinary::from(code_info.checksum.as_slice()),
                    status: NodeStatus::Active,
                    size_weight,
                },
            )?;

            Ok(Response::new()
                .add_event(
                    Event::new(format!("{}/node.deploy", env!("CARGO_PKG_NAME")))
                        .add_attribute("address", address.as_str())
                        .add_attribute("code_id", code_id.to_string())
                        .add_attribute("size_weight", size_weight.to_string()),
                )
                .add_message(WasmMsg::Instantiate2 {
                    admin: Some(env.contract.address.to_string()),
                    code_id,
                    label,
                    msg: instantiate_msg,
                    funds: info.funds,
                    salt: salt.into(),
                }))
        }
        ManagerExecuteMsg::UpdateNodeStatus { address, status } => {
            let config = CONFIG.load(deps.storage)?;
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }
            let node = NODES.update(
                deps.storage,
                address.clone(),
                |node| -> Result<RegisteredNode, ContractError> {
                    let mut node =
                        node.ok_or_else(|| ContractError::generic_err("Node is not registered"))?;
                    node.status = status.clone();
                    Ok(node)
                },
            )?;

            Ok(Response::new().add_event(
                Event::new(format!("{}/node.update-status", env!("CARGO_PKG_NAME")))
                    .add_attribute("address", address.as_str())
                    .add_attribute("status", node.status.as_str()),
            ))
        }
        ManagerExecuteMsg::MigrateNode {
            address,
            new_code_id,
            migrate_msg,
            new_size_weight,
        } => {
            let config = CONFIG.load(deps.storage)?;
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }
            validate_node_weight(new_size_weight)?;
            let code_info = deps
                .querier
                .query_wasm_code_info(new_code_id)
                .map_err(|_| {
                    ContractError::generic_err(format!("Invalid node code ID: {new_code_id}"))
                })?;
            let node = NODES.update(
                deps.storage,
                address.clone(),
                |node| -> Result<RegisteredNode, ContractError> {
                    let mut node =
                        node.ok_or_else(|| ContractError::generic_err("Node is not registered"))?;
                    node.code_id = new_code_id;
                    node.checksum = cosmwasm_std::HexBinary::from(code_info.checksum.as_slice());
                    node.size_weight = new_size_weight;
                    Ok(node)
                },
            )?;

            Ok(Response::new()
                .add_event(
                    Event::new(format!("{}/node.migrate", env!("CARGO_PKG_NAME")))
                        .add_attribute("address", address.as_str())
                        .add_attribute("code_id", new_code_id.to_string())
                        .add_attribute("size_weight", new_size_weight.to_string()),
                )
                .add_message(WasmMsg::Migrate {
                    contract_addr: node.address.to_string(),
                    new_code_id,
                    msg: migrate_msg,
                }))
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
        ManagerQueryMsg::StrategiesById { start_after, limit } => {
            let strategies: Vec<Strategy> = STRATEGIES
                .range(
                    deps.storage,
                    start_after.map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .filter_map(|item| item.ok().map(|(_, strategy)| strategy))
                .take(limit.unwrap_or(30) as usize)
                .collect();

            to_json_binary(&strategies)
        }
        ManagerQueryMsg::Count {} => to_json_binary(&STRATEGY_COUNTER.load(deps.storage)?),
        ManagerQueryMsg::Node { address } => to_json_binary(&NODES.load(deps.storage, address)?),
        ManagerQueryMsg::Nodes { start_after, limit } => {
            let nodes = NODES
                .range(
                    deps.storage,
                    start_after.map(Bound::exclusive),
                    None,
                    Order::Ascending,
                )
                .take(limit.unwrap_or(30) as usize)
                .map(|result| result.map(|(_, node)| node))
                .collect::<StdResult<Vec<_>>>()?;
            to_json_binary(&nodes)
        }
    }
}

fn validate_node_weight(size_weight: u16) -> Result<(), ContractError> {
    if size_weight == 0 {
        return Err(ContractError::generic_err(
            "Node size weight must be greater than zero",
        ));
    }
    Ok(())
}

fn external_counts(nodes: &[Node]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for node in nodes {
        if let Some(external) = node.external() {
            *counts
                .entry(external.contract_address.to_string())
                .or_default() += 1;
        }
    }
    counts
}

fn external_address_counts(addresses: &[cosmwasm_std::Addr]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for address in addresses {
        *counts.entry(address.to_string()).or_default() += 1;
    }
    counts
}

fn validate_external_nodes(
    deps: Deps,
    nodes: &[Node],
    existing_counts: Option<&BTreeMap<String, usize>>,
) -> Result<(), ContractError> {
    let existing_counts = existing_counts.cloned().unwrap_or_default();

    for address in existing_counts.keys() {
        let address = deps.api.addr_validate(address)?;
        let registered = NODES
            .may_load(deps.storage, address.clone())?
            .ok_or_else(|| {
                ContractError::generic_err(format!("Node is not registered: {address}"))
            })?;
        if registered.status == NodeStatus::Disabled {
            return Err(ContractError::generic_err(format!(
                "Disabled node prevents strategy update: {address}"
            )));
        }
    }

    for (address, count) in external_counts(nodes) {
        let address = deps.api.addr_validate(&address)?;
        let registered = NODES
            .may_load(deps.storage, address.clone())?
            .ok_or_else(|| {
                ContractError::generic_err(format!("Node is not registered: {address}"))
            })?;

        match registered.status {
            NodeStatus::Active => {}
            NodeStatus::Deprecated => {
                let existing = existing_counts
                    .get(address.as_str())
                    .copied()
                    .unwrap_or_default();
                if count > existing {
                    return Err(ContractError::generic_err(format!(
                        "Deprecated node reference count cannot increase: {address}"
                    )));
                }
            }
            NodeStatus::Disabled => {
                return Err(ContractError::generic_err(format!(
                    "Disabled node cannot be used: {address}"
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use cosmwasm_schema::cw_serde;
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        to_json_binary, Addr, Checksum, CodeInfoResponse, ContractResult as QueryContractResult,
        SystemResult,
    };

    use super::*;

    fn mock_code_info(
        deps: &mut cosmwasm_std::OwnedDeps<
            cosmwasm_std::testing::MockStorage,
            cosmwasm_std::testing::MockApi,
            cosmwasm_std::testing::MockQuerier,
        >,
    ) {
        let creator = deps.api.addr_make("code-creator");
        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(QueryContractResult::Ok(
                to_json_binary(&CodeInfoResponse::new(
                    7,
                    creator.clone(),
                    Checksum::from([7u8; 32]),
                ))
                .unwrap(),
            ))
        });
    }

    #[test]
    fn test_instantiate_stores_explicit_registry_owner() {
        let mut deps = mock_dependencies();
        mock_code_info(&mut deps);
        let sender = deps.api.addr_make("sender");
        let owner = deps.api.addr_make("registry-owner");
        let fee_collector = deps.api.addr_make("fee-collector");

        instantiate(
            deps.as_mut(),
            mock_env(),
            message_info(&sender, &[]),
            ManagerConfig {
                owner: owner.clone(),
                fee_collector,
                strategy_code_id: 7,
            },
        )
        .unwrap();

        assert_eq!(CONFIG.load(deps.as_ref().storage).unwrap().owner, owner);
        assert_eq!(NODE_COUNTER.load(deps.as_ref().storage).unwrap(), 0);
    }

    #[test]
    fn test_migrate_initializes_owner_once_and_keeps_it_immutable() {
        #[cw_serde]
        struct OldConfig {
            fee_collector: Addr,
            strategy_code_id: u64,
        }

        let mut deps = mock_dependencies();
        mock_code_info(&mut deps);
        let owner = deps.api.addr_make("registry-owner");
        let fee_collector = deps.api.addr_make("fee-collector");
        let replacement_owner = deps.api.addr_make("replacement-owner");
        Item::<OldConfig>::new("config")
            .save(
                deps.as_mut().storage,
                &OldConfig {
                    fee_collector,
                    strategy_code_id: 1,
                },
            )
            .unwrap();

        migrate(
            deps.as_mut(),
            mock_env(),
            MigrateMsg {
                owner: owner.clone(),
                strategy_code_id: 7,
            },
        )
        .unwrap();
        assert_eq!(CONFIG.load(deps.as_ref().storage).unwrap().owner, owner);

        assert!(migrate(
            deps.as_mut(),
            mock_env(),
            MigrateMsg {
                owner: replacement_owner,
                strategy_code_id: 7,
            },
        )
        .is_err());
    }

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
