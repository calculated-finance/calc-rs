use calc_rs::msg::{VaultExecuteMsg, VaultInstantiateMsg, VaultQueryMsg};
use calc_rs::types::ContractResult;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};
// use cw2::set_contract_version;

use crate::state::get_config;
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
    msg: VaultInstantiateMsg,
) -> ContractResult {
    msg.config.validate(deps.as_ref(), info)?;
    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: VaultExecuteMsg,
) -> ContractResult {
    match msg {
        VaultExecuteMsg::Execute {} => {
            unimplemented!()
        }
        VaultExecuteMsg::Withdraw {} => {
            unimplemented!()
        }
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: VaultQueryMsg) -> StdResult<Binary> {
    to_json_binary(&match msg {
        VaultQueryMsg::Config {} => get_config(deps.storage),
    }?)
}

#[cfg(test)]
mod tests {}
