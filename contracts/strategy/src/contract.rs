use calc_rs::msg::{StrategyExecuteMsg, StrategyInstantiateMsg, StrategyQueryMsg};
use calc_rs::types::{ContractError, ContractResult};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Deps, DepsMut, Env, MessageInfo, Reply, StdError, StdResult,
};

use crate::state::{CONFIG, MANAGER};
use crate::types::Runnable;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    MANAGER.save(deps.storage, &info.sender)?;
    msg.strategy.clone().initialize(deps, env, info)
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
            "Must invoke strategy execute methods via manager: {}",
            manager_address
        ))));
    }

    let mut strategy = CONFIG.load(deps.storage)?;

    match msg {
        StrategyExecuteMsg::Execute { executor } => {
            let execution_fee = strategy.get_execution_fee(deps.as_ref(), env.clone())?;

            if execution_fee.amount.is_zero() {
                return Ok(strategy.execute(deps, env)?);
            }

            Ok(strategy.execute(deps, env).map(|response| {
                response.add_message(BankMsg::Send {
                    to_address: executor.to_string(),
                    amount: vec![execution_fee],
                })
            })?)
        }
        StrategyExecuteMsg::Withdraw { amounts } => strategy.withdraw(deps.as_ref(), env, amounts),
        StrategyExecuteMsg::Pause {} => strategy.pause(deps.as_ref(), env),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, reply: Reply) -> ContractResult {
    CONFIG.load(deps.storage)?.handle_reply(deps, env, reply)
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
