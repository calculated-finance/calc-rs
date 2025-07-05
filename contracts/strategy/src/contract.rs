use std::{
    cmp::{max, min},
    collections::HashSet,
};

use calc_rs::{
    actions::{
        action::Action,
        behaviour::Behaviour,
        operation::Operation,
        recipients::{Destination, Recipient, Recipients},
    },
    core::{Contract, ContractError, ContractResult},
    manager::{Affiliate, StrategyStatus},
    statistics::Statistics,
    strategy::{StrategyConfig, StrategyExecuteMsg, StrategyInstantiateMsg, StrategyQueryMsg},
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, BankMsg, Binary, Coin, Coins, Decimal, Deps, DepsMut, Env, Event,
    MessageInfo, Reply, Response, StdResult, SubMsg, SubMsgResult, Uint128,
};

use crate::state::{STATE, STATS, STRATEGY};

const BASE_FEE_BPS: u64 = 25;
const FEE_COLLECTOR: &str = "sthor17pfp4qvy5vrmtjar7kntachm0cfm9m9azl3jka"; // TODO: replace with actual fee collector address

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: StrategyInstantiateMsg,
) -> ContractResult {
    if msg.action.size() > 10 {
        return Err(ContractError::generic_err(
            "Behaviour cannot exceed 10 actions",
        ));
    }

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
            label: "CALC automation fee".to_string(),
        }],
    ]
    .concat();

    let action = with_affiliates(msg.action, affiliates).init(deps.as_ref(), &env)?;
    let escrowed = action.escrowed(deps.as_ref(), &env)?;

    STRATEGY.save(
        deps.storage,
        StrategyConfig {
            manager: info.sender.clone(),
            owner: msg.owner,
            action,
            escrowed,
        },
    )?;

    STATS.save(deps.storage, &Statistics::default())?;

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
        STATE.save(deps.storage, &msg)?;
    }

    let strategy = STRATEGY.load(deps.storage)?;

    let mut all_messages: Vec<SubMsg> = vec![];
    let mut all_events: Vec<Event> = vec![];

    match msg.clone() {
        StrategyExecuteMsg::Update(update) => {
            if info.sender != strategy.manager {
                return Err(ContractError::Unauthorized {});
            }

            let (cancelled_action, cancel_messages, cancel_events) =
                strategy.action.cancel(deps.as_ref(), &env)?;

            all_events.extend(cancel_events);

            if cancel_messages.is_empty() {
                let (action, messages, events) = update
                    .init(deps.as_ref(), &env)?
                    .execute(deps.as_ref(), &env)?;

                let escrowed = strategy
                    .escrowed
                    .union(&action.escrowed(deps.as_ref(), &env)?)
                    .cloned()
                    .collect::<HashSet<String>>();

                STRATEGY.save(
                    deps.storage,
                    StrategyConfig {
                        action,
                        escrowed,
                        ..strategy
                    },
                )?;

                all_messages.extend(messages);
                all_events.extend(events);
            } else {
                STRATEGY.save(
                    deps.storage,
                    StrategyConfig {
                        action: cancelled_action,
                        ..strategy
                    },
                )?;

                let update_msg = Contract(env.contract.address.clone())
                    .call(to_json_binary(&StrategyExecuteMsg::Update(update))?, vec![]);

                all_messages.extend(cancel_messages);
                all_messages.push(SubMsg::reply_never(update_msg));
            }
        }
        StrategyExecuteMsg::Execute {} => {
            if info.sender != strategy.manager && info.sender != env.contract.address {
                return Err(ContractError::Unauthorized {});
            }

            let (action, messages, event) = strategy.action.execute(deps.as_ref(), &env)?;

            STRATEGY.save(deps.storage, StrategyConfig { action, ..strategy })?;

            all_messages.extend(messages);
            all_events.extend(event);
        }
        StrategyExecuteMsg::Withdraw(amounts) => {
            if info.sender != strategy.owner {
                return Err(ContractError::Unauthorized {});
            }

            let mut desired = Coins::try_from(amounts)?;
            let mut withdrawals = Coins::default();

            for amount in desired.clone().iter() {
                if desired.is_empty() {
                    break;
                }

                if strategy.escrowed.contains(&amount.denom) {
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
                desired.sub(withdrawal)?;
            }

            if !desired.is_empty() {
                let (messages, behaviour_withdrawals) =
                    strategy.action.withdraw(deps.as_ref(), &env, &desired)?;

                all_messages.extend(messages);

                for withdrawal in behaviour_withdrawals.into_iter() {
                    withdrawals.add(withdrawal)?;
                }
            }

            let bank_msg = BankMsg::Send {
                to_address: strategy.owner.to_string(),
                amount: withdrawals.to_vec(),
            };

            all_messages.push(SubMsg::reply_never(bank_msg));
        }
        StrategyExecuteMsg::UpdateStatus(status) => {
            if info.sender != strategy.manager {
                return Err(ContractError::Unauthorized {});
            }

            match status {
                StrategyStatus::Active => {
                    let (action, messages, event) = strategy.action.execute(deps.as_ref(), &env)?;

                    STRATEGY.save(deps.storage, StrategyConfig { action, ..strategy })?;

                    all_messages.extend(messages);
                    all_events.extend(event);
                }
                StrategyStatus::Paused | StrategyStatus::Archived => {
                    let (action, messages, event) = strategy.action.cancel(deps.as_ref(), &env)?;

                    STRATEGY.save(deps.storage, StrategyConfig { action, ..strategy })?;

                    all_messages.extend(messages);
                    all_events.extend(event);
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

    let clear_state_msg = SubMsg::reply_never(
        Contract(env.contract.address.clone())
            .call(to_json_binary(&StrategyExecuteMsg::Clear {})?, vec![]),
    );

    all_messages.push(clear_state_msg);

    Ok(Response::default()
        .add_submessages(all_messages)
        .add_events(all_events))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, reply: Reply) -> ContractResult {
    if matches!(reply.result, SubMsgResult::Ok(_)) {
        let stats = from_json::<Statistics>(reply.payload);
        if let Ok(stats) = stats {
            STATS.update(_deps.storage, |s| s.add(stats))?;
        }
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
            let mut balances = strategy.action.balances(deps, &env, &include)?;

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

fn with_affiliates(action: Action, affiliates: &Vec<Affiliate>) -> Action {
    match action {
        Action::DistributeTo(Recipients {
            denoms,
            mutable_destinations,
            immutable_destinations,
        }) => {
            let total_affiliate_bps = affiliates
                .iter()
                .fold(0, |acc, affiliate| acc + affiliate.bps);

            let total_shares = mutable_destinations
                .iter()
                .chain(immutable_destinations.iter())
                .fold(Uint128::zero(), |acc, d| acc + d.shares);

            let total_shares_with_fees =
                total_shares.mul_ceil(Decimal::bps(10_000 + total_affiliate_bps));

            Action::DistributeTo(Recipients {
                denoms: denoms.clone(),
                mutable_destinations: mutable_destinations.clone(),
                immutable_destinations: [
                    immutable_destinations.clone(),
                    affiliates
                        .iter()
                        .map(|affiliate| Destination {
                            recipient: Recipient::Bank {
                                address: affiliate.address.clone(),
                            },
                            shares: total_shares_with_fees.mul_floor(Decimal::bps(affiliate.bps)),
                            label: Some(affiliate.label.clone()),
                        })
                        .collect::<Vec<_>>(),
                ]
                .concat(),
            })
        }
        Action::Exhibit(Behaviour { actions, threshold }) => Action::Exhibit(Behaviour {
            actions: actions
                .into_iter()
                .map(|action| with_affiliates(action, affiliates))
                .collect(),
            threshold,
        }),
        _ => action,
    }
}
