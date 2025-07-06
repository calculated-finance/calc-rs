use std::{cmp::min, collections::HashSet};

use calc_rs::{
    core::{Contract, ContractError, ContractResult},
    manager::StrategyStatus,
    statistics::Statistics,
    strategy::{StrategyConfig, StrategyExecuteMsg, StrategyInstantiateMsg, StrategyQueryMsg},
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, BankMsg, Binary, Coin, Coins, Deps, DepsMut, Env, MessageInfo,
    Reply, Response, StdResult, SubMsg, SubMsgResult,
};

use crate::state::{CONFIG, ESCROWED, STATE, STATS};

const MAX_BEHAVIOUR_ACTIONS: usize = 10;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    if msg.0.size() > MAX_BEHAVIOUR_ACTIONS {
        return Err(ContractError::generic_err(format!(
            "Behaviour cannot exceed {} actions",
            MAX_BEHAVIOUR_ACTIONS
        )));
    }

    let escrowed = msg.0.escrowed(deps.as_ref(), &env)?;

    Ok(msg.0.init(&mut deps, &env, |storage, strategy| {
        CONFIG.init(
            storage,
            StrategyConfig {
                manager: info.sender.clone(),
                strategy,
                escrowed,
            },
        )
    })?)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyExecuteMsg,
) -> ContractResult {
    let state = STATE.may_load(deps.storage)?;

    if let Some(state) = state {
        if msg == state {
            return Err(ContractError::generic_err(
                "Contract is already in the requested state",
            ));
        }
    }

    if matches!(msg, StrategyExecuteMsg::Clear {}) {
        STATE.remove(deps.storage);
        return Ok(Response::default());
    }

    let config = CONFIG.load(deps.storage)?;

    let response = match msg {
        StrategyExecuteMsg::Update(update) => {
            if info.sender != config.manager {
                return Err(ContractError::Unauthorized {});
            }

            let cancel_response = config
                .strategy
                .prepare_to_cancel(deps.as_ref(), &env)?
                .execute(&mut deps, |store, strategy| CONFIG.save(store, strategy))?;

            if cancel_response.messages.is_empty() {
                let escrowed = update
                    .escrowed(deps.as_ref(), &env)?
                    .union(&config.escrowed)
                    .cloned()
                    .collect::<HashSet<String>>();

                ESCROWED.save(deps.storage, &escrowed)?;

                let init_response = update.init(&mut deps, &env, |storage, strategy| {
                    CONFIG.save(storage, strategy)
                })?;

                let execute_msg = SubMsg::reply_always(
                    Contract(env.contract.address.clone())
                        .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, vec![]),
                    0,
                );

                init_response.add_submessage(execute_msg) // Execute the new strategy after any init messages
            } else {
                let clear_state_msg = SubMsg::reply_never(
                    Contract(env.contract.address.clone())
                        .call(to_json_binary(&StrategyExecuteMsg::Clear {})?, vec![]),
                );

                let update_again_msg = SubMsg::reply_never(
                    Contract(env.contract.address.clone())
                        .call(to_json_binary(&StrategyExecuteMsg::Update(update))?, vec![]),
                );

                cancel_response
                    .add_submessage(clear_state_msg) // Clear the state so we can run update again
                    .add_submessage(update_again_msg) // Run update to setup the new strategy
            }
        }
        StrategyExecuteMsg::Execute {} => {
            if info.sender != config.manager && info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            config
                .strategy
                .prepare_to_execute(deps.as_ref(), &env)?
                .execute(&mut deps, |store, strategy| CONFIG.save(store, strategy))?
        }
        StrategyExecuteMsg::Withdraw(amounts) => {
            if info.sender != config.strategy.owner {
                return Err(ContractError::Unauthorized {});
            }

            let mut remaining_desired = Coins::try_from(amounts.clone())?;
            let mut withdrawals = Coins::default();

            for amount in amounts.iter() {
                if config.escrowed.contains(&amount.denom) {
                    return Err(ContractError::generic_err(format!(
                        "Cannot withdraw escrowed denom: {}",
                        amount.denom
                    )));
                }

                let balance = deps
                    .querier
                    .query_balance(env.contract.address.clone(), amount.denom.clone())?;

                let withdrawal =
                    Coin::new(min(balance.amount, amount.amount), amount.denom.clone());

                withdrawals.add(withdrawal.clone())?;
                remaining_desired.sub(withdrawal)?;
            }

            let withdrawal_msg = SubMsg::reply_never(BankMsg::Send {
                to_address: config.strategy.owner.to_string(),
                amount: withdrawals.to_vec(),
            });

            if !remaining_desired.is_empty() {
                Response::default().add_submessage(withdrawal_msg)
            } else {
                config
                    .strategy
                    .prepare_to_withdraw(deps.as_ref(), &env, &remaining_desired)?
                    .execute(&mut deps, |store, strategy| CONFIG.save(store, strategy))?
                    .add_submessage(withdrawal_msg)
            }
        }
        StrategyExecuteMsg::UpdateStatus(status) => {
            if info.sender != config.manager {
                return Err(ContractError::Unauthorized {});
            }

            let executable_strategy = match status {
                StrategyStatus::Active => {
                    config.strategy.prepare_to_execute(deps.as_ref(), &env)?
                }
                StrategyStatus::Paused | StrategyStatus::Archived => {
                    config.strategy.prepare_to_cancel(deps.as_ref(), &env)?
                }
            };

            executable_strategy
                .execute(&mut deps, |store, strategy| CONFIG.save(store, strategy))?
        }
        StrategyExecuteMsg::Clear {} => {
            if info.sender != env.contract.address && info.sender != config.strategy.owner {
                return Err(ContractError::Unauthorized {});
            }

            STATE.remove(deps.storage);
            Response::default()
        }
    };

    let clear_state_msg = SubMsg::reply_never(
        Contract(env.contract.address.clone())
            .call(to_json_binary(&StrategyExecuteMsg::Clear {})?, vec![]),
    );

    Ok(response.add_submessage(clear_state_msg))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, reply: Reply) -> ContractResult {
    if let SubMsgResult::Ok(_) = reply.result {
        let stats = from_json::<Statistics>(reply.payload);
        if let Ok(stats) = stats {
            STATS.update(_deps.storage, |s| s.add(stats))?;
        }
    }

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: StrategyQueryMsg) -> StdResult<Binary> {
    let strategy = CONFIG.load(deps.storage)?;
    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&strategy),
        StrategyQueryMsg::Statistics {} => to_json_binary(&STATS.load(deps.storage)?),
        StrategyQueryMsg::Balances { include } => {
            let mut balances = strategy.strategy.balances(deps, &env, &include)?;

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
