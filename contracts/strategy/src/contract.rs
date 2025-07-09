use std::collections::HashSet;

use calc_rs::{
    constants::{LOG_ERRORS_REPLY_ID, PROCESS_PAYLOAD_REPLY_ID},
    core::{Contract, ContractError, ContractResult},
    manager::StrategyStatus,
    strategy::{
        StrategyConfig, StrategyExecuteMsg, StrategyInstantiateMsg, StrategyMsgPayload,
        StrategyQueryMsg,
    },
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, BankMsg, Binary, Coins, Deps, DepsMut, Env, MessageInfo, Reply,
    Response, StdResult, SubMsg, SubMsgResult,
};

use crate::state::{ACTIVE_STRATEGY, CONFIG, ESCROWED, STATE, STATS};

const MAX_BEHAVIOUR_ACTIONS: usize = 10;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    if msg.size() > MAX_BEHAVIOUR_ACTIONS {
        return Err(ContractError::generic_err(format!(
            "Behaviour cannot exceed {MAX_BEHAVIOUR_ACTIONS} actions"
        )));
    }

    // Collate escrowed denoms & initialise the strategy
    let escrowed = msg.escrowed(deps.as_ref(), &env)?;
    let response = msg.init(&mut deps, &env, |storage, strategy| {
        CONFIG.init(
            storage,
            StrategyConfig {
                manager: info.sender.clone(),
                strategy,
                escrowed,
            },
        )
    })?;

    // Execute the strategy immediately after instantiation
    Ok(response.add_submessage(SubMsg::reply_always(
        Contract(env.contract.address.clone())
            .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, vec![]),
        LOG_ERRORS_REPLY_ID,
    )))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyExecuteMsg,
) -> ContractResult {
    let state = STATE.may_load(deps.storage)?;

    // We allow arbitrary distribution messages to be sent, which
    // could result in recursive calls between strategies and/or other
    // contracts. This is a safety check to short circuit that.
    if let Some(state) = state {
        if msg == state {
            return Err(ContractError::generic_err(format!(
                "Contract is already in the {state:?} state, cannot execute again"
            )));
        }
    }

    let config = CONFIG.load(deps.storage)?;

    let response = match msg {
        StrategyExecuteMsg::Update(update) => {
            if info.sender != config.manager {
                return Err(ContractError::Unauthorized {});
            }

            let cancel_response = config
                .strategy
                .activate()
                .prepare_to_cancel(deps.as_ref(), &env)?
                .execute(&mut deps, &env, |store, strategy| {
                    ACTIVE_STRATEGY.save(store, &strategy)
                })?;

            // If no stateful actions to unwind, we can proceed with the update
            if cancel_response.messages.is_empty() {
                // Accumulate any newly escrowed denoms
                let escrowed = update
                    .escrowed(deps.as_ref(), &env)?
                    .union(&config.escrowed)
                    .cloned()
                    .collect::<HashSet<String>>();

                ESCROWED.save(deps.storage, &escrowed)?;

                // Get the required messages to initialize the new strategy
                let init_response = update.init(&mut deps, &env, |storage, strategy| {
                    CONFIG.save(storage, strategy)
                })?;

                let execute_msg = SubMsg::reply_always(
                    Contract(env.contract.address.clone())
                        .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, vec![]),
                    LOG_ERRORS_REPLY_ID,
                );

                // Execute the new strategy after all init messages have completed
                init_response.add_submessage(execute_msg)
            } else {
                let update_again_msg = SubMsg::reply_always(
                    Contract(env.contract.address.clone())
                        .call(to_json_binary(&StrategyExecuteMsg::Update(update))?, vec![]),
                    LOG_ERRORS_REPLY_ID,
                );

                // Clear the state so we can run update again
                STATE.remove(deps.storage);

                cancel_response // Unwind any stateful actions before we overwrite them
                    .add_submessage(update_again_msg) // Run update to setup the new strategy
            }
        }
        StrategyExecuteMsg::Execute => {
            if info.sender != config.manager && info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            config
                .strategy
                .activate()
                .prepare_to_execute(deps.as_ref(), &env)?
                .execute(&mut deps, &env, |store, strategy| {
                    ACTIVE_STRATEGY.save(store, &strategy)
                })?
        }
        StrategyExecuteMsg::Withdraw(desired) => {
            if info.sender != config.strategy.owner && info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            for denom in desired.iter() {
                if config.escrowed.contains(denom) {
                    return Err(ContractError::generic_err(format!(
                        "Cannot withdraw escrowed denom: {denom}"
                    )));
                }
            }

            let owner = config.strategy.owner.to_string();

            let withdraw_response = config
                .strategy
                .activate()
                .prepare_to_withdraw(deps.as_ref(), &env, &desired)?
                .execute(&mut deps, &env, |store, strategy| {
                    ACTIVE_STRATEGY.save(store, &strategy)
                })?;

            if withdraw_response.messages.is_empty() {
                let mut withdrawals = Coins::default();

                for denom in desired.iter() {
                    let balance = deps
                        .querier
                        .query_balance(env.contract.address.clone(), denom.clone())?;

                    withdrawals.add(balance.clone())?;
                }

                let withdrawal_bank_msg = SubMsg::reply_always(
                    BankMsg::Send {
                        to_address: owner,
                        amount: withdrawals.to_vec(),
                    },
                    LOG_ERRORS_REPLY_ID,
                );

                Response::default().add_submessage(withdrawal_bank_msg)
            } else {
                let withdraw_again_msg = SubMsg::reply_always(
                    Contract(env.contract.address.clone()).call(
                        to_json_binary(&StrategyExecuteMsg::Withdraw(desired))?,
                        vec![],
                    ),
                    LOG_ERRORS_REPLY_ID,
                );

                // Clear the state so we can run withdraw again
                STATE.remove(deps.storage);

                withdraw_response.add_submessage(withdraw_again_msg)
            }
        }
        StrategyExecuteMsg::UpdateStatus(status) => {
            if info.sender != config.manager {
                return Err(ContractError::Unauthorized {});
            }

            match status {
                StrategyStatus::Active => config
                    .strategy
                    .activate()
                    .prepare_to_execute(deps.as_ref(), &env)?
                    .execute(&mut deps, &env, |store, strategy| {
                        ACTIVE_STRATEGY.save(store, &strategy)
                    }),
                // Paused & Archived are no different in terms of execution,
                // they are only used for filtering strategies in factory queries
                StrategyStatus::Paused | StrategyStatus::Archived => config
                    .strategy
                    .activate()
                    .prepare_to_cancel(deps.as_ref(), &env)?
                    .execute(&mut deps, &env, |store, strategy| {
                        ACTIVE_STRATEGY.save(store, &strategy)
                    }),
            }?
        }
        StrategyExecuteMsg::Commit => {
            if info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            let active_strategy = ACTIVE_STRATEGY.load(deps.storage)?;
            let committed_strategy = active_strategy.commit(deps.as_ref(), &env)?;

            ACTIVE_STRATEGY.remove(deps.storage);
            CONFIG.save(deps.storage, committed_strategy)?;

            Response::default()
        }
        StrategyExecuteMsg::Clear {} => {
            if info.sender != env.contract.address && info.sender != config.strategy.owner {
                return Err(ContractError::Unauthorized {});
            }

            STATE.remove(deps.storage);

            // Hard return to avoid sending another clear state message
            return Ok(Response::default());
        }
    };

    let clear_state_msg = SubMsg::reply_always(
        Contract(env.contract.address.clone())
            .call(to_json_binary(&StrategyExecuteMsg::Clear {})?, vec![]),
        LOG_ERRORS_REPLY_ID,
    );

    Ok(response.add_submessage(clear_state_msg))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, _env: Env, reply: Reply) -> ContractResult {
    let response = Response::new().add_attribute("reply_id", reply.id.to_string());
    match reply.id {
        PROCESS_PAYLOAD_REPLY_ID => {
            let payload = from_json::<StrategyMsgPayload>(reply.payload.clone());
            if let Ok(payload) = payload {
                match reply.result {
                    SubMsgResult::Ok(_) => {
                        STATS.update(deps.storage, |s| s.update(payload.statistics.clone()))?;
                        Ok(response.add_events(payload.decorated_events("succeeded")))
                    }
                    SubMsgResult::Err(err) => Ok(response
                        .add_events(payload.decorated_events("failed"))
                        .add_attribute("msg_error", err)),
                }
            } else {
                Ok(response
                    .add_attribute("msg_error", "Failed to parse reply payload")
                    .add_attribute("msg_payload", reply.payload.to_string()))
            }
        }
        _ => match reply.result {
            SubMsgResult::Ok(_) => Ok(response),
            SubMsgResult::Err(err) => Ok(response
                .add_attribute("msg_error", err)
                .add_attribute("msg_payload", reply.payload.to_string())),
        },
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: StrategyQueryMsg) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;

    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&config),
        StrategyQueryMsg::Statistics {} => to_json_binary(&STATS.load(deps.storage)?),
        StrategyQueryMsg::Balances(include) => {
            let mut balances = config.strategy.balances(deps, &env, &include)?;

            for denom in include {
                let balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), denom)?;

                balances.add(balance)?;
            }

            to_json_binary(&balances.to_vec())
        }
    }
}
