use calc_rs::msg::{StrategyExecuteMsg, StrategyInstantiateMsg, StrategyQueryMsg};
use calc_rs::types::{ContractError, ContractResult, StrategyConfig};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, StdError, StdResult,
};

use crate::state::{CONFIG, FEE_COLLECTOR, IS_EXECUTING, MANAGER};
use crate::types::Runnable;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    MANAGER.save(deps.storage, &info.sender)?;
    FEE_COLLECTOR.save(deps.storage, &msg.fee_collector)?;
    IS_EXECUTING.save(deps.storage, &false)?;

    let mut strategy = StrategyConfig::from(msg.strategy.clone());
    let response = strategy.instantiate(deps.as_ref(), env, info)?;

    CONFIG.save(deps.storage, &strategy)?;

    Ok(response)
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyExecuteMsg,
) -> ContractResult {
    let manager_address = MANAGER.load(deps.storage)?;

    if info.sender != manager_address {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "Must invoke strategy execute methods via strategy manager contract: {}",
            manager_address
        ))));
    }

    let mut strategy = CONFIG.load(deps.storage)?;

    let response = match msg {
        StrategyExecuteMsg::Execute {} => {
            IS_EXECUTING.save(deps.storage, &true)?;
            strategy.execute(deps.as_ref(), env)
        }
        StrategyExecuteMsg::Pause {} => strategy.pause(deps.as_ref(), env),
        StrategyExecuteMsg::Resume {} => strategy.resume(deps.as_ref(), env),
        StrategyExecuteMsg::Deposit {} => strategy.deposit(deps.as_ref(), env, info),
        StrategyExecuteMsg::Withdraw { amounts } => strategy.withdraw(deps.as_ref(), env, amounts),
        StrategyExecuteMsg::Update { update } => strategy.update(deps.as_ref(), env, update),
    }?;

    CONFIG.save(deps.storage, &strategy)?;

    Ok(response)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, reply: Reply) -> ContractResult {
    IS_EXECUTING.save(deps.storage, &false)?;
    CONFIG
        .load(deps.storage)?
        .handle_reply(deps.as_ref(), env, reply)
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: StrategyQueryMsg) -> StdResult<Binary> {
    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        StrategyQueryMsg::CanExecute {} => to_json_binary(
            &(!IS_EXECUTING.load(deps.storage)?
                && CONFIG.load(deps.storage)?.can_execute(deps, &env).is_ok()),
        ),
    }
}

#[cfg(test)]
mod tests {}
