use calc_rs::msg::{StrategyExecuteMsg, StrategyInstantiateMsg, StrategyQueryMsg};
use calc_rs::types::{ContractError, ContractResult, DomainEvent};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdResult,
};

use crate::state::{FACTORY, STRATEGY};
use crate::types::{Executable, Pausable, Schedulable, Validatable, Withdrawable};

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    FACTORY.save(deps.storage, &info.sender)?;
    msg.strategy.validate(deps.as_ref(), info)?;
    STRATEGY.save(deps.storage, &msg.strategy)?;
    Ok(Response::default().add_event(DomainEvent::StrategyCreated {
        contract_address: _env.contract.address,
        config: msg.strategy,
    }))
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

    let strategy = STRATEGY.load(deps.storage)?;

    match msg {
        StrategyExecuteMsg::Execute {} => strategy.execute(deps.as_ref(), env),
        StrategyExecuteMsg::Schedule {} => strategy.schedule(deps, env),
        StrategyExecuteMsg::Withdraw { denoms } => strategy.withdraw(deps.as_ref(), env, denoms),
        StrategyExecuteMsg::Pause {} => strategy.pause(deps.as_ref(), env),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, reply: Reply) -> ContractResult {
    STRATEGY.load(deps.storage)?.handle_reply(env, reply)
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: StrategyQueryMsg) -> StdResult<Binary> {
    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&STRATEGY.load(deps.storage)?),
        StrategyQueryMsg::CanExecute {} => {
            to_json_binary(&STRATEGY.load(deps.storage)?.can_execute(deps, env).is_ok())
        }
    }
}

#[cfg(test)]
mod tests {}
