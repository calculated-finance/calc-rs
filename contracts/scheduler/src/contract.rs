use calc_rs::msg::{SchedulerExecuteMsg, SchedulerQueryMsg};
use calc_rs::types::{Condition, ConditionFilter, ContractResult};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Order, Response, StdResult,
};
use cw_storage_plus::Bound;

use crate::state::{save_trigger, triggers};

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
        SchedulerExecuteMsg::Create {
            condition,
            to,
            callback,
        } => {
            save_trigger(deps.storage, info.sender, condition, callback, to)?;
            Ok(Response::default())
        }
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: SchedulerQueryMsg) -> StdResult<Binary> {
    match msg {
        SchedulerQueryMsg::Get { filter, limit } => to_json_binary(
            &(match filter {
                ConditionFilter::Owner { address } => match address {
                    Some(addr) => triggers().idx.owner.prefix(addr).range(
                        deps.storage,
                        None,
                        None,
                        Order::Ascending,
                    ),
                    None => triggers().range(deps.storage, None, None, Order::Ascending),
                },
                ConditionFilter::Timestamp { start, end } => triggers().idx.timestamp.range(
                    deps.storage,
                    start.map(|s| Bound::inclusive((s.seconds(), u64::MAX))),
                    end.map(|e| Bound::inclusive((e.seconds(), u64::MAX))),
                    Order::Ascending,
                ),
                ConditionFilter::BlockHeight { start, end } => triggers().idx.block_height.range(
                    deps.storage,
                    start.map(|s| Bound::inclusive((s, u64::MAX))),
                    end.map(|e| Bound::inclusive((e, u64::MAX))),
                    Order::Ascending,
                ),
                ConditionFilter::LimitOrder {} => {
                    triggers()
                        .idx
                        .limit_order_id
                        .range(deps.storage, None, None, Order::Ascending)
                }
            })
            .take(limit.unwrap_or(30))
            .flat_map(|r| r.map(|(_, v)| v.condition))
            .collect::<Vec<Condition>>(),
        ),
    }
}

#[cfg(test)]
mod tests {}
