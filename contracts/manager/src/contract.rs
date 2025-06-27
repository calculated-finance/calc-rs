use calc_rs::types::{
    Contract, ContractError, ContractResult, DomainEvent, ManagerConfig, ManagerExecuteMsg,
    ManagerInstantiateMsg, ManagerQueryMsg, Strategy, StrategyExecuteMsg, StrategyInstantiateMsg,
    StrategyStatus,
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    instantiate2_address, to_json_binary, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo,
    Order, Response, StdError, StdResult, WasmMsg,
};
use cw_storage_plus::Bound;

use crate::state::{strategy_store, AFFILIATES, CODE_IDS, CONFIG, STRATEGY_COUNTER};

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
            fee_collector: msg.fee_collector,
        },
    )?;

    for (strategy_type, code_id) in msg.code_ids {
        CODE_IDS.save(deps.storage, strategy_type, &code_id)?;
    }

    STRATEGY_COUNTER.save(deps.storage, &0)?;

    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ManagerExecuteMsg,
) -> ContractResult {
    let mut messages: Vec<CosmosMsg> = Vec::new();
    let mut events: Vec<DomainEvent> = Vec::new();

    match msg.clone() {
        ManagerExecuteMsg::InstantiateStrategy {
            owner,
            label,
            strategy,
        } => {
            let config = CONFIG.load(deps.storage)?;
            let strategy_id = STRATEGY_COUNTER.load(deps.storage)? + 1;
            let salt = to_json_binary(&(owner.clone(), strategy_id, env.block.time.seconds()))?;
            let code_id = CODE_IDS
                .load(deps.storage, strategy.strategy_type())?;

            let contract_address = deps.api.addr_humanize(&instantiate2_address(
                &deps
                    .querier
                    .query_wasm_code_info(code_id)?
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
                    label: label.clone(),
                    status: StrategyStatus::Active,
                    affiliates: Vec::new(),
                },
            )?;

            let instantiate_strategy_msg = WasmMsg::Instantiate2 {
                admin: Some(owner.to_string()),
                code_id,
                label,
                msg: to_json_binary(&StrategyInstantiateMsg {
                    fee_collector: config.fee_collector,
                    strategy: strategy.clone(),
                })?,
                funds: info.funds,
                salt,
            };

            let strategy_instantiated_event = DomainEvent::StrategyInstantiated { contract_address };

            messages.push(instantiate_strategy_msg.into());
            events.push(strategy_instantiated_event);
        }
        ManagerExecuteMsg::ExecuteStrategy {
            contract_address, .. // We include optional an msg in the API for future extension
        } => {
            let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            let execute_msg = Contract(contract_address.clone())
                    .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, info.funds);

            let execute_event = DomainEvent::StrategyExecuted { contract_address };

            messages.push(execute_msg.into());
            events.push(execute_event);
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

            let update_msg = Contract(contract_address.clone()).call(
                    to_json_binary(&StrategyExecuteMsg::Update(update))?,
                    info.funds,
                );

            let update_event = DomainEvent::StrategyUpdated {
                contract_address,
            };

            messages.push(update_msg.into());
            events.push(update_event);
        }
        ManagerExecuteMsg::UpdateStrategyStatus { contract_address, status } => {
            let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            if strategy.owner != info.sender {
                return Err(ContractError::Std(StdError::generic_err("Unauthorized")));
            }

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    status: status.clone(),
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            let update_status_msg = Contract(contract_address.clone()).call(
                    to_json_binary(&StrategyExecuteMsg::UpdateStatus(status))?,
                    info.funds,
                );

            let update_status_event = DomainEvent::StrategyStatusUpdated {
                contract_address,
            };

            messages.push(update_status_msg.into());
            events.push(update_status_event);
        }
        ManagerExecuteMsg::AddAffiliate { affiliate } => {
            AFFILIATES.save(deps.storage, affiliate.code.clone(), &affiliate)?;
        }
        ManagerExecuteMsg::RemoveAffiliate { code } => {
            AFFILIATES.remove(deps.storage, code);
        }
    };

    Ok(Response::default()
        .add_messages(messages)
        .add_events(events))
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
    use calc_rs::types::{ManagerQueryMsg, Strategy, StrategyStatus};
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
            label: "Test Strategy".to_string(),
            status: StrategyStatus::Active,
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
