use calc_rs::msg::{StrategyExecuteMsg, StrategyInstantiateMsg, StrategyQueryMsg};
use calc_rs::types::{ContractResult, DomainEvent};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdResult,
};
// use cw2::set_contract_version;

use crate::state::{CONFIG, FACTORY};
use crate::strategies::{Executable, Validatable};

/*
// version info for migration info
const CONTRACT_NAME: &str = "crates.io:vault";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
*/

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    FACTORY.save(deps.storage, &info.sender)?;
    msg.config.validate(deps.as_ref(), info)?;
    CONFIG.save(deps.storage, &msg.config)?;
    Ok(Response::default().add_event(DomainEvent::StrategyCreated {
        contract_address: _env.contract.address,
        config: msg.config,
    }))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: StrategyExecuteMsg,
) -> ContractResult {
    match msg {
        StrategyExecuteMsg::Execute {} => {
            let config = CONFIG.load(deps.storage)?;
            match config.can_execute(deps.as_ref(), env.clone()) {
                Ok(_) => config.execute(),
                Err(reason) => Ok(
                    Response::default().add_event(DomainEvent::ExecutionSkipped {
                        contract_address: env.contract.address,
                        reason,
                    }),
                ),
            }
        }
        StrategyExecuteMsg::Withdraw { assets: _ } => {
            unimplemented!()
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, reply: Reply) -> ContractResult {
    let config = CONFIG.load(deps.storage)?;
    config.handle_result(deps, env, reply)
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: StrategyQueryMsg) -> StdResult<Binary> {
    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        StrategyQueryMsg::CanExecute {} => to_json_binary(&true),
    }
}

#[cfg(test)]
mod tests {}
