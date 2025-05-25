use calc_rs::{
    msg::{
        FactoryExecuteMsg, FactoryInstantiateMsg, FactoryMigrateMsg, FactoryQueryMsg,
        StrategyInstantiateMsg,
    },
    types::{Contract, ContractResult, Status},
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    instantiate2_address, to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult, WasmMsg,
};

use crate::{
    state::{
        create_strategy_handle, get_config, update_strategy_status, CreateStrategyHandleCommand,
        UpdateStrategyStatusCommand, CONFIG, STRATEGY_COUNTER,
    },
    types::Config,
};

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: FactoryInstantiateMsg,
) -> ContractResult {
    CONFIG.save(
        deps.storage,
        &Config {
            checksum: msg.checksum,
            code_id: msg.code_id,
        },
    )?;
    Ok(Response::default())
}

#[entry_point]
pub fn migrate(deps: DepsMut, _: Env, msg: FactoryMigrateMsg) -> ContractResult {
    CONFIG.save(
        deps.storage,
        &Config {
            checksum: msg.checksum,
            code_id: msg.code_id,
        },
    )?;
    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: FactoryExecuteMsg,
) -> ContractResult {
    match msg.clone() {
        FactoryExecuteMsg::InstantiateStrategy {
            owner,
            label,
            strategy,
        } => {
            let config = get_config(deps.storage)?;

            let salt = to_json_binary(&(
                env.block.time.seconds(),
                owner.clone(),
                msg.clone(),
                STRATEGY_COUNTER.load(deps.storage)?,
            ))?;

            create_strategy_handle(
                deps.storage,
                CreateStrategyHandleCommand {
                    owner: owner.clone(),
                    contract_address: deps.api.addr_humanize(&instantiate2_address(
                        &config.checksum,
                        &deps.api.addr_canonicalize(env.contract.address.as_str())?,
                        &salt,
                    )?)?,
                    status: Status::Active,
                    updated_at: env.block.time.seconds(),
                },
            )?;

            Ok(Response::default().add_message(WasmMsg::Instantiate2 {
                admin: Some(owner.to_string()),
                code_id: config.code_id,
                label,
                msg: to_json_binary(&StrategyInstantiateMsg { strategy })?,
                funds: info.funds,
                salt,
            }))
        }
        FactoryExecuteMsg::Proxy {
            contract_address,
            msg,
        } => Ok(Response::default()
            .add_message(Contract(contract_address).call(to_json_binary(&msg)?, info.funds)?)),
        FactoryExecuteMsg::UpdateStatus { status } => {
            update_strategy_status(
                deps.storage,
                UpdateStrategyStatusCommand {
                    contract_address: info.sender.clone(),
                    status: status.clone(),
                    updated_at: env.block.time.seconds(),
                },
            )?;

            Ok(Response::default())
        }
    }
}

#[entry_point]
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
