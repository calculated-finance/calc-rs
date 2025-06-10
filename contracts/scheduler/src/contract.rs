use calc_rs::msg::{SchedulerExecuteMsg, SchedulerQueryMsg};
use calc_rs::types::{ConditionFilter, ContractResult, Executable, Trigger};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Coins, Deps, DepsMut, Env, MessageInfo, Response, StdError,
    StdResult,
};

use crate::state::{fetch_triggers, triggers, TRIGGER_COUNTER};

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

#[cw_serde]
pub struct MigrateMsg {}

#[entry_point]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, StdError> {
    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: SchedulerExecuteMsg,
) -> ContractResult {
    match msg {
        SchedulerExecuteMsg::CreateTrigger(trigger_to_create) => {
            let id = TRIGGER_COUNTER.update(deps.storage, |id| Ok::<u64, StdError>(id + 1))?;

            triggers().save(
                deps.storage,
                id,
                &Trigger {
                    id,
                    owner: info.sender,
                    condition: trigger_to_create.condition,
                    msg: trigger_to_create.msg,
                    to: trigger_to_create.to,
                    execution_rebate: info.funds,
                },
            )?;

            Ok(Response::default())
        }
        SchedulerExecuteMsg::SetTriggers(triggers_to_create) => {
            let triggers_to_delete = fetch_triggers(
                deps.as_ref(),
                ConditionFilter::Owner {
                    address: info.sender.clone(),
                },
                None,
            );

            let mut rebates_to_refund = Coins::default();

            for trigger in triggers_to_delete {
                triggers().remove(deps.storage, trigger.id)?;

                for coin in &trigger.execution_rebate {
                    rebates_to_refund.add(coin.clone())?;
                }
            }

            for trigger_to_create in triggers_to_create {
                let id = TRIGGER_COUNTER.update(deps.storage, |id| Ok::<u64, StdError>(id + 1))?;

                triggers().save(
                    deps.storage,
                    id,
                    &Trigger {
                        id,
                        owner: info.sender.clone(),
                        condition: trigger_to_create.condition,
                        msg: trigger_to_create.msg,
                        to: trigger_to_create.to,
                        execution_rebate: info.funds.clone(),
                    },
                )?;
            }

            if rebates_to_refund.is_empty() {
                return Ok(Response::default());
            }

            Ok(Response::default().add_message(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: rebates_to_refund.into(),
            }))
        }
        SchedulerExecuteMsg::DeleteTriggers {} => {
            let triggers_to_delete = fetch_triggers(
                deps.as_ref(),
                ConditionFilter::Owner {
                    address: info.sender.clone(),
                },
                None,
            );

            let mut rebates_to_refund = Coins::default();

            for trigger in triggers_to_delete {
                triggers().remove(deps.storage, trigger.id)?;

                for coin in &trigger.execution_rebate {
                    rebates_to_refund.add(coin.clone())?;
                }
            }

            if rebates_to_refund.is_empty() {
                return Ok(Response::default());
            }

            Ok(Response::default().add_message(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: rebates_to_refund.into(),
            }))
        }
        SchedulerExecuteMsg::ExecuteTrigger { id } => {
            let trigger = triggers().load(deps.storage, id)?;
            let response = trigger.execute(env)?;

            triggers().remove(deps.storage, id)?;

            Ok(response.add_message(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: trigger.execution_rebate,
            }))
        }
    }
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: SchedulerQueryMsg) -> StdResult<Binary> {
    match msg {
        SchedulerQueryMsg::Triggers { filter, limit } => {
            to_json_binary(&fetch_triggers(deps, filter, limit))
        }
        SchedulerQueryMsg::CanExecute { id } => {
            to_json_binary(&triggers().load(deps.storage, id)?.can_execute(env))
        }
    }
}

#[cfg(test)]
mod tests {}
