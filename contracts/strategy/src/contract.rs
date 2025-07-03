use std::{
    cmp::min,
    collections::{HashMap, HashSet},
};

use calc_rs::{
    core::{Behaviour, Condition, Contract, ContractError, ContractResult, Statistics},
    distributor::{Destination, DistributorConfig, DistributorExecuteMsg, Recipient},
    ladder::{LadderConfig, LadderStatistics},
    manager::{
        Affiliate, CreateStrategyConfig, ManagerQueryMsg, StrategyExecuteMsg,
        StrategyInstantiateMsg, StrategyQueryMsg, StrategyStatus,
    },
    scheduler::SchedulerExecuteMsg,
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    instantiate2_address, to_json_binary, BankMsg, Binary, Coin, Coins, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Reply, Response, StdResult, SubMsg, Uint128, WasmMsg,
};
use rujira_rs::{
    fin::{
        BookResponse, ConfigResponse, ExecuteMsg, OrdersResponse, Price, QueryMsg, Side,
        SwapRequest,
    },
    CallbackData,
};
use serde::de;

use crate::{
    state::{STATE, STATS, STRATEGY},
    types::DomainEvent,
};

const BASE_FEE_BPS: u64 = 15;
const FEE_COLLECTOR: &str = "calc1feecollector";

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    mut msg: StrategyInstantiateMsg,
) -> ContractResult {
    let mut affiliates = vec![Affiliate {
        address: deps.api.addr_validate(FEE_COLLECTOR)?,
        bps: BASE_FEE_BPS,
        code: "CALC".to_string(),
    }];

    if let Some(affiliate_code) = msg.affiliate_code {
        let affiliate = deps.querier.query_wasm_smart::<Option<Affiliate>>(
            info.sender,
            &ManagerQueryMsg::Affiliate {
                code: affiliate_code,
            },
        )?;

        if let Some(affiliate) = affiliate {
            affiliates.push(affiliate);
        }
    }

    msg.config.init(deps.as_ref(), &affiliates)?;

    Ok(Response::default())
}

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> ContractResult {
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
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

    match msg {
        StrategyExecuteMsg::Clear {} => {}
        _ => STATE.save(deps.storage, &msg)?,
    }

    let mut config = STRATEGY.load(deps.storage)?;
    let mut stats = STATS.load(deps.storage)?;

    let mut messages: Vec<CosmosMsg> = vec![];
    let mut sub_messages: Vec<SubMsg> = vec![];
    let mut events: Vec<DomainEvent> = vec![];

    match msg.clone() {
        StrategyExecuteMsg::Update(new_config) => {
            if info.sender != config.manager {
                // this ensures distributor contracts are not updated by anyone other than the owning contract
                return Err(ContractError::Unauthorized {});
            }
        }
        StrategyExecuteMsg::Execute {} => {
            if info.sender != config.manager && info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            for mut behaviour in config.behaviours.clone().into_iter() {
                if behaviour
                    .conditions
                    .iter()
                    .all(|c| c.check(deps.as_ref(), &env).is_ok())
                {
                    sub_messages.extend(behaviour.execute(deps.as_ref(), &env)?);
                }
            }
        }
        StrategyExecuteMsg::Withdraw(amounts) => {
            if info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            sub_messages.extend(config.withdraw(
                deps.as_ref(),
                &env,
                Coins::try_from(amounts.clone())?,
            )?);
        }
        StrategyExecuteMsg::UpdateStatus(status) => {
            if info.sender != config.manager {
                return Err(ContractError::Unauthorized {});
            }

            match status {
                StrategyStatus::Active => {
                    let execute_msg = Contract(env.contract.address.clone())
                        .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, vec![]);

                    messages.push(execute_msg);
                }
                StrategyStatus::Paused | StrategyStatus::Archived => {
                    let set_triggers_msg = Contract(config.scheduler.clone()).call(
                        to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![]))?,
                        vec![],
                    );

                    messages.push(set_triggers_msg);
                }
            }
        }
        StrategyExecuteMsg::Clear {} => {
            if info.sender != env.contract.address && info.sender != config.owner {
                return Err(ContractError::Unauthorized {});
            }

            STATE.remove(deps.storage);
        }
    };

    STRATEGY.save(deps.storage, &config)?;
    STATS.save(deps.storage, &stats)?;

    match msg {
        StrategyExecuteMsg::Clear {} => {}
        _ => {
            messages.push(
                Contract(env.contract.address.clone())
                    .call(to_json_binary(&StrategyExecuteMsg::Clear {})?, vec![]),
            );
        }
    }

    Ok(Response::default()
        .add_messages(messages)
        .add_events(events))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, _reply: Reply) -> ContractResult {
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: StrategyQueryMsg) -> StdResult<Binary> {
    let config = STRATEGY.load(deps.storage)?;
    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&config),
        StrategyQueryMsg::Statistics {} => {
            let stats = STATS.load(deps.storage).map(|stats| {
                config.behaviours.iter().fold(stats, |mut acc, behaviour| {
                    acc.add(behaviour.statistics.clone());
                    acc
                })
            })?;

            to_json_binary(&stats)
        }
        StrategyQueryMsg::Balances { include } => {
            to_json_binary(&config.balances(deps, &env, include)?)
        }
    }
}
