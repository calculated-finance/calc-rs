use calc_rs::msg::{StrategyExecuteMsg, StrategyInstantiateMsg, StrategyQueryMsg};
use calc_rs::types::{ContractError, ContractResult};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError, StdResult,
};
// use cw2::set_contract_version;

use crate::events::DomainEvent;
use crate::state::{get_config, update_config};
use crate::validation::Validate;

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
    msg.config.validate(deps.as_ref(), info)?;
    update_config(deps.storage, msg.config.clone())?;
    Ok(Response::default().add_event(DomainEvent::StrategyCreated {
        contract_address: _env.contract.address,
        config: msg.config,
    }))
}

#[entry_point]
pub fn execute(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: StrategyExecuteMsg,
) -> ContractResult {
    match msg {
        StrategyExecuteMsg::Execute {} => {
            unimplemented!()
        }
        StrategyExecuteMsg::Withdraw { assets: _ } => {
            unimplemented!()
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, reply: Reply) -> ContractResult {
    match reply.id {
        id => Err(ContractError::Std(StdError::generic_err(format!(
            "unhandled DCA contract reply id: {}",
            id
        )))),
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: StrategyQueryMsg) -> StdResult<Binary> {
    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&get_config(deps.storage)?),
        StrategyQueryMsg::CanExecute {} => to_json_binary(&true),
    }
}

#[cfg(test)]
mod tests {}
