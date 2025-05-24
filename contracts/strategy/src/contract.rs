use calc_rs::msg::{StrategyExecuteMsg, StrategyInstantiateMsg, StrategyQueryMsg};
use calc_rs::types::{ContractError, ContractResult};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, StdResult};

use crate::state::{CONFIG, FACTORY};
use crate::types::Runnable;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    FACTORY.save(deps.storage, &info.sender)?;
    msg.strategy.clone().initialize(deps, env, info)
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyExecuteMsg,
) -> ContractResult {
    if info.sender != FACTORY.load(deps.storage)? {
        return Err(ContractError::Unauthorized {});
    }

    let strategy = CONFIG.load(deps.storage)?;

    match msg {
        StrategyExecuteMsg::Execute {} => strategy.execute(deps.as_ref(), env),
        StrategyExecuteMsg::Schedule {} => strategy.schedule(deps, env),
        StrategyExecuteMsg::Withdraw { denoms } => strategy.withdraw(deps.as_ref(), env, denoms),
        StrategyExecuteMsg::Pause {} => strategy.pause(deps.as_ref(), env),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, reply: Reply) -> ContractResult {
    CONFIG
        .load(deps.storage)?
        .handle_execute_reply(deps, env, reply)
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: StrategyQueryMsg) -> StdResult<Binary> {
    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        StrategyQueryMsg::CanExecute {} => {
            to_json_binary(&CONFIG.load(deps.storage)?.can_execute(deps, env).is_ok())
        }
    }
}

#[cfg(test)]
mod tests {}
