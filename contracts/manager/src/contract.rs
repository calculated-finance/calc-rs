use calc_rs::{
    msg::{FactoryExecuteMsg, FactoryInstantiateMsg, FactoryQueryMsg, VaultInstantiateMsg},
    types::ContractResult,
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult, WasmMsg,
};
// use cw2::set_contract_version;

use crate::{
    state::{get_config, update_config},
    types::Config,
};

/*
// version info for migration info
const CONTRACT_NAME: &str = "crates.io:factory";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
*/

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: FactoryInstantiateMsg,
) -> ContractResult {
    update_config(
        deps.storage,
        Config {
            vault_code_id: msg.vault_code_id,
        },
    )?;
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: FactoryExecuteMsg,
) -> ContractResult {
    let local_config = get_config(deps.storage)?;
    match msg {
        FactoryExecuteMsg::Create { label, config } => {
            Ok(Response::default().add_message(WasmMsg::Instantiate {
                admin: None,
                code_id: local_config.vault_code_id,
                label,
                msg: to_json_binary(&VaultInstantiateMsg { config })?,
                funds: info.funds,
            }))
        }
        FactoryExecuteMsg::Update { .. } => {
            unimplemented!()
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(_deps: Deps, _env: Env, msg: FactoryQueryMsg) -> StdResult<Binary> {
    match msg {
        FactoryQueryMsg::Strategy { .. } => {
            unimplemented!()
        }
        FactoryQueryMsg::Strategies { .. } => {
            unimplemented!()
        }
    }
}

#[cfg(test)]
mod tests {}
