use std::cmp::max;

use calc_rs::{
    constants::BASE_FEE_BPS,
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
    deps: DepsMut,
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
            let config = CONFIG.load(deps.storage)?;

            let total_affiliate_bps = affiliates
                .iter()
                .fold(0, |acc, affiliate| acc + affiliate.bps);

            let affiliates = [
                affiliates,
                vec![Affiliate {
                    address: config.fee_collector,
                    bps: max(
                        BASE_FEE_BPS.saturating_sub(10),
                        BASE_FEE_BPS.saturating_sub(total_affiliate_bps),
                    ),
                    label: "CALC automation fee".to_string(),
                }],
            ]
            .concat();

            let strategy_with_affiliates = strategy
                .to_new(config.strategy_code_id, env.contract.address, label.clone())
                .with_affiliates(deps.as_ref(), &affiliates)?;

            let id = STRATEGY_COUNTER.update(deps.storage, |id| Ok::<u64, StdError>(id + 1))?;

            strategy_store().save(
                deps.storage,
                strategy_with_affiliates.state.contract_address.clone(),
                &StrategyHandle {
                    id,
                    owner: strategy_with_affiliates.owner.clone(),
                    contract_address: strategy_with_affiliates.state.contract_address.clone(),
                    created_at: env.block.time.seconds(),
                    updated_at: env.block.time.seconds(),
                    label,
                    status: StrategyStatus::Active,
                    affiliates,
                },
            )?;

            let instantiate_msg = strategy_with_affiliates.instantiate_msg(&info.funds)?;

            Response::default().add_message(instantiate_msg)
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

            let config = CONFIG.load(deps.storage)?;

            let update_msg = Contract(contract_address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::Update(
                    update
                        .to_new(
                            config.strategy_code_id,
                            env.contract.address,
                            strategy.label.clone(),
                        )
                        .with_affiliates(deps.as_ref(), &strategy.affiliates)?,
                ))?,
                info.funds,
            );

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &StrategyHandle {
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

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
