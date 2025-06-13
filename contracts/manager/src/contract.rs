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

    CONFIG.save(
        deps.storage,
        &ManagerConfig {
            admin,
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

            let strategy_id = STRATEGY_COUNTER.load(deps.storage)? + 1;

            let salt = to_json_binary(&(owner.clone(), strategy_id, env.block.time.seconds()))?;

            let contract_address = deps.api.addr_humanize(&instantiate2_address(
                &deps
                    .querier
                    .query_wasm_code_info(config.code_id)?
                    .checksum
                    .as_slice(),
                &deps.api.addr_canonicalize(env.contract.address.as_str())?,
                &salt,
            )?)?;

            STRATEGY_COUNTER.save(deps.storage, &strategy_id)?;

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    owner: owner.clone(),
                    contract_address: contract_address.clone(),
                    created_at: env.block.time.seconds(),
                    updated_at: env.block.time.seconds(),
                    executions: 0,
                    label: label.clone(),
                    status: Status::Active,
                    affiliates: Vec::new(),
                },
            )?;

            let instantiate_strategy_msg = WasmMsg::Instantiate2 {
                admin: Some(owner.to_string()),
                code_id: config.code_id,
                label,
                msg: to_json_binary(&StrategyInstantiateMsg {
                    fee_collector: config.fee_collector,
                    strategy: strategy.clone(),
                })?,
                funds: info.funds,
                salt,
            };

            Ok(Response::default()
                .add_message(instantiate_strategy_msg)
                .add_attribute("strategy_contract_address", contract_address))
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
            start_after: _,
            limit,
        } => to_json_binary(
            &match owner {
                Some(owner) => match status {
                    Some(status) => strategy_store()
                        .idx
                        .owner_status_updated_at
                        .sub_prefix((owner.into(), status as u8)),
                    None => strategy_store()
                        .idx
                        .owner_updated_at
                        .sub_prefix(owner.into()),
                },
                None => match status {
                    Some(status) => strategy_store()
                        .idx
                        .status_updated_at
                        .sub_prefix(status as u8),
                    None => strategy_store().idx.updated_at.sub_prefix(()),
                },
            }
            .range(deps.storage, None, None, Order::Descending)
            .take(match limit {
                Some(limit) => match limit {
                    0..=30 => limit as usize,
                    _ => 30,
                },
                None => 30,
            })
            .flat_map(|result| result.map(|(_, strategy)| strategy))
            .collect::<Vec<Strategy>>(),
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
mod tests {
    use calc_rs::{
        msg::ManagerQueryMsg,
        types::{Status, Strategy},
    };
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        to_json_binary, Addr,
    };

    use crate::{contract::query, state::strategy_store};

    #[test]
    fn can_fetch_strategies() {
        let sender = Addr::unchecked("sender");

        let mut deps = mock_dependencies();
        let env = mock_env();

        let contract_address = Addr::unchecked("strategy");

        let strategy = Strategy {
            owner: sender.clone(),
            contract_address: contract_address.clone(),
            created_at: env.block.time.seconds(),
            updated_at: env.block.time.seconds(),
            executions: 0,
            label: "Test Strategy".to_string(),
            status: Status::Active,
            affiliates: Vec::new(),
        };

        strategy_store()
            .save(&mut deps.storage, sender.clone(), &strategy)
            .unwrap();

        let strategies = query(
            deps.as_ref(),
            env.clone(),
            ManagerQueryMsg::Strategies {
                owner: None,
                status: None,
                start_after: None,
                limit: None,
            },
        )
        .unwrap();

        assert_eq!(strategies, to_json_binary(&vec![strategy]).unwrap());
    }
}
