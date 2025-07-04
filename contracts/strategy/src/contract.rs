use std::cmp::max;

use calc_rs::{
    actions::operation::Operation,
    core::{Contract, ContractError, ContractResult},
    events::DomainEvent,
    manager::{Affiliate, StrategyStatus},
    statistics::Statistics,
    strategy::{Strategy2, StrategyExecuteMsg, StrategyInstantiateMsg, StrategyQueryMsg},
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, BankMsg, Binary, Coins, Deps, DepsMut, Env, MessageInfo, Reply,
    Response, StdResult, SubMsg, SubMsgResult,
};

use crate::state::{STATE, STATS, STRATEGY};

const BASE_FEE_BPS: u64 = 15;
const FEE_COLLECTOR: &str = "calc1feecollector";

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    let total_affiliate_bps = msg
        .affiliates
        .iter()
        .fold(0, |acc, affiliate| acc + affiliate.bps);

    let affiliates = &[
        msg.affiliates,
        vec![Affiliate {
            address: deps.api.addr_validate(FEE_COLLECTOR)?,
            bps: max(
                BASE_FEE_BPS.saturating_sub(10),
                BASE_FEE_BPS.saturating_sub(total_affiliate_bps),
            ),
            code: "CALC automation fee".to_string(),
        }],
    ]
    .concat();

    let actions_with_fees = msg
        .actions
        .iter()
        .map(|action| action.with_affiliates(affiliates))
        .collect::<Vec<_>>();

    STRATEGY.save(
        deps.storage,
        &Strategy2 {
            manager: info.sender.clone(),
            owner: msg.owner,
            actions: actions_with_fees,
        },
    )?;

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

    if !matches!(msg, StrategyExecuteMsg::Clear {}) {
        STATE.save(deps.storage, &msg)?
    }

    let strategy = STRATEGY.load(deps.storage)?;

    let mut all_messages: Vec<SubMsg> = vec![];
    let mut all_events: Vec<DomainEvent> = vec![];

    match msg.clone() {
        StrategyExecuteMsg::Update(new_config) => {
            if info.sender != strategy.manager {
                return Err(ContractError::Unauthorized {});
            }

            let (strategy, messages, events) = strategy.update(deps.as_ref(), &env, new_config)?;
            STRATEGY.save(deps.storage, &strategy)?;

            all_messages.extend(messages);
            all_events.extend(events);
        }
        StrategyExecuteMsg::Execute {} => {
            if info.sender != strategy.manager && info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            let (strategy, messages, event) = strategy.execute(deps.as_ref(), &env)?;
            STRATEGY.save(deps.storage, &strategy)?;

            all_messages.extend(messages);
            all_events.extend(event);
        }
        StrategyExecuteMsg::Withdraw(amounts) => {
            if info.sender != strategy.owner {
                return Err(ContractError::Unauthorized {});
            }

            let (strategy, messages, withdrawals) =
                strategy.withdraw(deps.as_ref(), &env, &Coins::try_from(amounts.clone())?)?;

            all_messages.extend(messages);

            let bank_msg = BankMsg::Send {
                to_address: strategy.owner.to_string(),
                amount: withdrawals.to_vec(),
            };

            all_messages.push(SubMsg::new(bank_msg));
        }
        StrategyExecuteMsg::UpdateStatus(status) => {
            if info.sender != strategy.manager {
                return Err(ContractError::Unauthorized {});
            }

            match status {
                StrategyStatus::Active => {
                    let (strategy, messages, event) = strategy.execute(deps.as_ref(), &env)?;
                    STRATEGY.save(deps.storage, &strategy)?;

                    all_messages.extend(messages);
                    all_events.extend(event);
                }
                StrategyStatus::Paused | StrategyStatus::Archived => {
                    // TODO: unwind actions
                }
            }
        }
        StrategyExecuteMsg::Clear {} => {
            if info.sender != env.contract.address && info.sender != strategy.owner {
                return Err(ContractError::Unauthorized {});
            }

            STATE.remove(deps.storage);
        }
    };

    if !matches!(msg, StrategyExecuteMsg::Clear {}) {
        all_messages.push(SubMsg::reply_never(
            Contract(env.contract.address.clone())
                .call(to_json_binary(&StrategyExecuteMsg::Clear {})?, vec![]),
        ));
    }

    Ok(Response::default()
        .add_submessages(all_messages)
        .add_events(all_events))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, reply: Reply) -> ContractResult {
    if matches!(reply.result, SubMsgResult::Ok(_)) {
        STATS.update(_deps.storage, |s| {
            s.add(from_json::<Statistics>(reply.payload).unwrap_or(Statistics::default()))
        })?;
    }

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: StrategyQueryMsg) -> StdResult<Binary> {
    let strategy = STRATEGY.load(deps.storage)?;
    match msg {
        StrategyQueryMsg::Config {} => to_json_binary(&strategy),
        StrategyQueryMsg::Statistics {} => to_json_binary(&STATS.load(deps.storage)?),
        StrategyQueryMsg::Balances { include } => {
            to_json_binary(&strategy.balances(deps, &env, &include)?.to_vec())
        }
    }
}
