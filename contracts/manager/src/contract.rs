use calc_rs::{
    msg::{
        ManagerExecuteMsg, ManagerInstantiateMsg, ManagerMigrateMsg, ManagerQueryMsg,
        StrategyExecuteMsg, StrategyInstantiateMsg,
    },
    types::{Contract, ContractError, ContractResult, ManagerConfig, Status, Strategy},
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    instantiate2_address, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response,
    StdError, StdResult, WasmMsg,
};
use cw_storage_plus::Bound;

use crate::state::{strategy_store, AFFILIATES, CONFIG, STRATEGY_COUNTER};

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ManagerInstantiateMsg,
) -> ContractResult {
    CONFIG.save(
        deps.storage,
        &ManagerConfig {
            admin: info.sender,
            checksum: msg.checksum,
            code_id: msg.code_id,
            fee_collector: msg.fee_collector,
        },
    )?;

    STRATEGY_COUNTER.save(deps.storage, &0)?;

    Ok(Response::default())
}

#[entry_point]
pub fn migrate(deps: DepsMut, _: Env, msg: ManagerMigrateMsg) -> ContractResult {
    let admin = CONFIG.load(deps.storage)?.admin;

    STRATEGY_COUNTER.save(deps.storage, &0)?; // TODO: remove

    CONFIG.save(
        deps.storage,
        &ManagerConfig {
            admin,
            checksum: msg.checksum,
            code_id: msg.code_id,
            fee_collector: msg.fee_collector,
        },
    )?;

    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ManagerExecuteMsg,
) -> ContractResult {
    match msg.clone() {
        ManagerExecuteMsg::InstantiateStrategy {
            owner,
            label,
            strategy,
        } => {
            let config = CONFIG.load(deps.storage)?;

            let salt = to_json_binary(&(
                env.block.time.seconds(),
                owner.clone(),
                STRATEGY_COUNTER.load(deps.storage)?,
            ))?;

            let contract_address = deps.api.addr_humanize(&instantiate2_address(
                &config.checksum,
                &deps.api.addr_canonicalize(env.contract.address.as_str())?,
                &salt,
            )?)?;

            let strategy_id = STRATEGY_COUNTER.may_load(deps.storage)?.unwrap_or_default() + 1;
            STRATEGY_COUNTER.save(deps.storage, &strategy_id)?;

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    owner: owner.clone(),
                    contract_address,
                    created_at: env.block.time.seconds(),
                    updated_at: env.block.time.seconds(),
                    executions: 0,
                    label: label.clone(),
                    status: Status::Active,
                    affiliates: Vec::new(),
                },
            )?;

            Ok(Response::default().add_message(WasmMsg::Instantiate2 {
                admin: Some(owner.to_string()),
                code_id: config.code_id,
                label,
                msg: to_json_binary(&StrategyInstantiateMsg {
                    fee_collector: config.fee_collector,
                    strategy,
                })?,
                funds: info.funds,
                salt,
            }))
        }
        ManagerExecuteMsg::ExecuteStrategy { contract_address } => {
            let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    executions: strategy.executions + 1,
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            Ok(Response::default().add_message(
                Contract(contract_address)
                    .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, info.funds)?,
            ))
        }
        ManagerExecuteMsg::PauseStrategy { contract_address } => {
            let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            if strategy.owner != info.sender {
                return Err(ContractError::Std(StdError::generic_err("Unauthorized")));
            }

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            Ok(Response::default().add_message(
                Contract(contract_address.clone())
                    .call(to_json_binary(&StrategyExecuteMsg::Pause {})?, info.funds)?,
            ))
        }
        ManagerExecuteMsg::ResumeStrategy { contract_address } => {
            let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            if strategy.owner != info.sender {
                return Err(ContractError::Std(StdError::generic_err("Unauthorized")));
            }

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            Ok(Response::default().add_message(
                Contract(contract_address.clone())
                    .call(to_json_binary(&StrategyExecuteMsg::Resume {})?, info.funds)?,
            ))
        }
        ManagerExecuteMsg::WithdrawFromStrategy {
            contract_address,
            amounts,
        } => {
            // let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            // if strategy.owner != info.sender {
            //     return Err(ContractError::Std(StdError::generic_err("Unauthorized")));
            // }

            // strategy_store().save(
            //     deps.storage,
            //     contract_address.clone(),
            //     &Strategy {
            //         updated_at: env.block.time.seconds(),
            //         ..strategy
            //     },
            // )?;

            Ok(
                Response::default().add_message(Contract(contract_address).call(
                    to_json_binary(&StrategyExecuteMsg::Withdraw { amounts })?,
                    info.funds,
                )?),
            )
        }
        ManagerExecuteMsg::UpdateStrategy {
            contract_address,
            update,
        } => {
            let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            if strategy.owner != info.sender {
                return Err(ContractError::Std(StdError::generic_err("Unauthorized")));
            }

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            Ok(
                Response::default().add_message(Contract(contract_address).call(
                    to_json_binary(&StrategyExecuteMsg::Update { update })?,
                    info.funds,
                )?),
            )
        }
        ManagerExecuteMsg::UpdateStatus { status } => {
            let strategy = strategy_store().load(deps.storage, info.sender.clone())?;

            if strategy.contract_address != info.sender {
                return Err(ContractError::Std(StdError::generic_err("Unauthorized")));
            }

            strategy_store().save(
                deps.storage,
                info.sender.clone(),
                &Strategy {
                    status: status,
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            Ok(Response::default())
        }
        ManagerExecuteMsg::AddAffiliate { affiliate } => {
            AFFILIATES.save(deps.storage, affiliate.code.clone(), &affiliate)?;
            Ok(Response::default())
        }
        ManagerExecuteMsg::RemoveAffiliate { code } => {
            AFFILIATES.remove(deps.storage, code);
            Ok(Response::default())
        }
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: ManagerQueryMsg) -> StdResult<Binary> {
    match msg {
        ManagerQueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        ManagerQueryMsg::Strategy { address } => {
            to_json_binary(&strategy_store().load(deps.storage, address)?)
        }
        ManagerQueryMsg::Strategies {
            owner,
            status,
            start_after,
            limit,
        } => to_json_binary(
            &(match owner {
                Some(owner) => match status {
                    Some(status) => strategy_store()
                        .idx
                        .owner_status
                        .prefix((owner, status as u8)),
                    None => strategy_store()
                        .idx
                        .owner_updated_at
                        .prefix((owner, u64::MAX)),
                },
                None => match status {
                    Some(status) => strategy_store()
                        .idx
                        .status_updated_at
                        .prefix((status, u64::MAX)),
                    None => strategy_store().idx.updated_at.prefix(u64::MAX),
                },
            }
            .range(
                deps.storage,
                start_after.map(Bound::exclusive),
                None,
                Order::Ascending,
            )
            .take(match limit {
                Some(limit) => match limit {
                    0..=30 => limit as usize,
                    _ => 30,
                },
                None => 30,
            })
            .flat_map(|result| result.map(|(_, handle)| handle))
            .collect::<Vec<Strategy>>()),
        ),
        ManagerQueryMsg::Affiliate { code } => {
            to_json_binary(&AFFILIATES.load(deps.storage, code)?)
        }
        ManagerQueryMsg::Affiliates { start_after, limit } => to_json_binary(
            &AFFILIATES
                .range(
                    deps.storage,
                    start_after.map(|addr| Bound::exclusive(addr)),
                    None,
                    cosmwasm_std::Order::Ascending,
                )
                .take(match limit {
                    Some(limit) => match limit {
                        0..=30 => limit as usize,
                        _ => 30,
                    },
                    None => 30,
                })
                .map(|item| item.map(|(_, affiliate)| affiliate))
                .collect::<StdResult<Vec<_>>>()?,
        ),
    }
}

#[cfg(test)]
mod tests {}
