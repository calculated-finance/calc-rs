use calc_rs::msg::{SchedulerExecuteMsg, SchedulerQueryMsg};
use calc_rs::types::{Condition, ConditionFilter, ContractResult};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response, StdResult,
};
use cw_storage_plus::Bound;

use crate::state::{delete_trigger, fetch_triggers, save_trigger, triggers};

#[cw_serde]
pub struct InstantiateMsg {}

#[entry_point]
pub fn instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: InstantiateMsg,
) -> ContractResult {
    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: SchedulerExecuteMsg,
) -> ContractResult {
    match msg {
        SchedulerExecuteMsg::CreateTrigger {
            condition,
            to,
            callback,
        } => {
            save_trigger(deps.storage, info.sender, condition, callback, to)?;
            Ok(Response::default())
        }
        SchedulerExecuteMsg::DeleteTriggers { owner } => {
            let triggers = fetch_triggers(
                deps.as_ref(),
                ConditionFilter::Owner { address: owner },
                None,
            );

            let x = triggers
                .iter()
                .map(|trigger| delete_trigger(deps.storage, trigger.id))
                .collect::<Vec<_>>();

            Ok(Response::default())
        }
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: SchedulerQueryMsg) -> StdResult<Binary> {
    match msg {
        SchedulerQueryMsg::Get { filter, limit } => {
            to_json_binary(&fetch_triggers(deps, filter, limit))
        }
    }
}

#[cfg(test)]
mod tests {}
