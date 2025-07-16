use std::vec;

use calc_rs::{
    conditions::Condition,
    constants::LOG_ERRORS_REPLY_ID,
    core::{Contract, ContractError, ContractResult},
    manager::ManagerExecuteMsg,
    scheduler::{SchedulerExecuteMsg, SchedulerInstantiateMsg, SchedulerQueryMsg},
};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Coin, Coins, Deps, DepsMut, Env, MessageInfo, Reply, Response,
    StdError, StdResult, SubMsg, SubMsgResult,
};
use rujira_rs::fin::{ConfigResponse, ExecuteMsg, OrderResponse, Price, QueryMsg};

use crate::state::{MANAGER, TRIGGERS};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: SchedulerInstantiateMsg,
) -> ContractResult {
    MANAGER.save(_deps.storage, &msg.manager)?;
    Ok(Response::default())
}

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, StdError> {
    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: SchedulerExecuteMsg,
) -> ContractResult {
    let mut sub_messages = vec![];

    match msg {
        SchedulerExecuteMsg::Create(condition) => {
            let trigger_id = condition.id(info.sender.clone())?;

            if let Ok(existing_trigger) = TRIGGERS.load(deps.storage, trigger_id) {
                // We delete any existing trigger as we
                // may be updating the execution rebate
                TRIGGERS.delete(deps.storage, existing_trigger.id)?;

                sub_messages.push(SubMsg::reply_never(BankMsg::Send {
                    to_address: info.sender.to_string(),
                    amount: existing_trigger.execution_rebate,
                }));
            }

            let mut execution_rebate = Coins::try_from(info.funds)?;

            if let Condition::LimitOrderFilled {
                pair_address,
                side,
                price,
                ..
            } = condition.clone()
            {
                let pair = deps.querier.query_wasm_smart::<ConfigResponse>(
                    pair_address.clone(),
                    &QueryMsg::Config {},
                )?;

                let bid_denom = pair.denoms.bid(&side);
                let bid_amount = Coin::new(execution_rebate.amount_of(bid_denom), bid_denom);

                if bid_amount.amount.is_zero() {
                    return Err(ContractError::generic_err("No funds sent for limit order"));
                }

                execution_rebate.sub(bid_amount.clone())?;

                // Reply never as we want to rollback the
                // trigger storage update if the order fails
                let set_order_msg = SubMsg::reply_never(Contract(pair_address).call(
                    to_json_binary(&ExecuteMsg::Order((
                        vec![(side, Price::Fixed(price), Some(bid_amount.amount))],
                        None,
                    )))?,
                    vec![bid_amount],
                ));

                sub_messages.push(set_order_msg);
            }

            TRIGGERS.save(
                deps.storage,
                trigger_id,
                info.sender.clone(),
                condition,
                execution_rebate.to_vec(),
            )?;
        }
        SchedulerExecuteMsg::Execute(ids) => {
            for id in ids {
                let trigger = TRIGGERS.load(deps.storage, id).map_err(|e| {
                    ContractError::generic_err(format!("Failed to load trigger: {e}"))
                })?;

                if let Ok(trigger_is_satisfied) =
                    trigger.condition.is_satisfied(deps.as_ref(), &env)
                {
                    // Only skip execution if the condition is valid but not satisfied.
                    // Process invalid triggers to clear them from the store & reward the executor.
                    if !trigger_is_satisfied {
                        continue;
                    }

                    if let Condition::LimitOrderFilled {
                        pair_address,
                        side,
                        price,
                        ..
                    } = trigger.condition
                    {
                        // Should never fail if the trigger is satisfied
                        let order = deps.querier.query_wasm_smart::<OrderResponse>(
                            pair_address.clone(),
                            &QueryMsg::Order((
                                env.contract.address.to_string(),
                                side.clone(),
                                Price::Fixed(price.clone()),
                            )),
                        )?;

                        // Rollback all msgs if the order is not retracted as we don't
                        // want to send out the filled amount if it's not available
                        let withdraw_order_msg =
                            SubMsg::reply_never(Contract(pair_address.clone()).call(
                                to_json_binary(&ExecuteMsg::Order((
                                    vec![(side.clone(), Price::Fixed(price), None)],
                                    None,
                                )))?,
                                vec![],
                            ));

                        sub_messages.push(withdraw_order_msg);

                        let pair = deps.querier.query_wasm_smart::<ConfigResponse>(
                            pair_address,
                            &QueryMsg::Config {},
                        )?;

                        // rebate the filled amount to the executor (should never be 0)
                        let rebate_msg = SubMsg::reply_never(BankMsg::Send {
                            to_address: info.sender.to_string(),
                            amount: vec![Coin::new(order.filled, pair.denoms.ask(&side))],
                        });

                        sub_messages.push(rebate_msg);
                    }
                }

                TRIGGERS.delete(deps.storage, trigger.id)?;

                // swallow errors from execute trigger replies to prevent
                // hanging triggers due to a misconfigured downstream contract.
                let execute_msg = SubMsg::reply_on_error(
                    Contract(MANAGER.load(deps.storage)?).call(
                        to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                            contract_address: trigger.owner,
                        })?,
                        vec![],
                    ),
                    LOG_ERRORS_REPLY_ID,
                );

                sub_messages.push(execute_msg);

                if !trigger.execution_rebate.is_empty() {
                    let rebate_msg = SubMsg::reply_never(BankMsg::Send {
                        to_address: info.sender.to_string(),
                        amount: trigger.execution_rebate,
                    });

                    sub_messages.push(rebate_msg);
                }
            }
        }
    };

    Ok(Response::default().add_submessages(sub_messages))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: SchedulerQueryMsg) -> StdResult<Binary> {
    match msg {
        SchedulerQueryMsg::Owned {
            owner,
            limit,
            start_after,
        } => to_json_binary(&TRIGGERS.owned(deps.storage, owner, limit, start_after)),
        SchedulerQueryMsg::Filtered { filter, limit } => {
            let filtered = TRIGGERS.filtered(deps.storage, filter, limit)?;
            to_json_binary(&filtered)
        }
        SchedulerQueryMsg::CanExecute(id) => to_json_binary(
            &TRIGGERS
                .load(deps.storage, id)?
                .condition
                // We don't coalesce errors into false so that external
                // executors can distinguish between a misconfigured trigger
                // and a trigger that is not satisfied.
                .is_satisfied(deps, &env)?,
        ),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, reply: Reply) -> ContractResult {
    match reply.result {
        SubMsgResult::Ok(_) => Ok(Response::default()),
        SubMsgResult::Err(err) => Ok(Response::default()
            .add_attribute("msg_error", err)
            .add_attribute("msg_payload", reply.payload.to_string())
            .add_attribute("reply_id", reply.id.to_string())),
    }
}

#[cfg(test)]
mod create_trigger_tests {
    use super::*;
    use calc_rs::{conditions::Condition, scheduler::Trigger};
    use cosmwasm_std::{
        testing::{message_info, mock_dependencies, mock_env},
        Addr, Coin, ContractResult as ContractQueryResult, Decimal, SystemResult,
    };
    use rujira_rs::fin::{Denoms, Price, Side, Tick};

    #[test]
    fn creates_block_trigger_correctly() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let info = message_info(&owner.clone(), &[Coin::new(3123_u128, "rune")]);

        let condition = Condition::BlocksCompleted(env.block.height + 10);

        execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        let triggers = TRIGGERS.owned(deps.as_ref().storage, owner.clone(), None, None);

        assert_eq!(
            triggers,
            vec![Trigger {
                id: condition.id(owner.clone()).unwrap(),
                owner: owner.clone(),
                condition: condition.clone(),
                execution_rebate: info.funds.clone(),
            }]
        );
    }

    #[test]
    fn updates_existing_block_trigger_correctly() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let info = message_info(&owner.clone(), &[Coin::new(3123_u128, "rune")]);

        let condition = Condition::BlocksCompleted(env.block.height + 10);

        execute(
            deps.as_mut(),
            env.clone(),
            info.clone(),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        let triggers = TRIGGERS.owned(deps.as_ref().storage, owner.clone(), None, None);

        assert_eq!(
            triggers,
            vec![Trigger {
                id: condition.id(owner.clone()).unwrap(),
                owner: owner.clone(),
                condition: condition.clone(),
                execution_rebate: info.funds.clone(),
            }]
        );

        let updated_info = message_info(&owner.clone(), &[Coin::new(1234_u128, "rune")]);

        execute(
            deps.as_mut(),
            env.clone(),
            updated_info.clone(),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        let updated_triggers = TRIGGERS.owned(deps.as_ref().storage, owner.clone(), None, None);

        assert_eq!(
            updated_triggers,
            vec![Trigger {
                id: condition.id(owner.clone()).unwrap(),
                owner: owner.clone(),
                condition: condition.clone(),
                execution_rebate: updated_info.funds.clone(),
            }]
        );
    }

    #[test]
    fn fails_to_create_limit_order_trigger_if_no_bid_amount_sent() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");

        let pair_address = Addr::unchecked("pair-0");

        let condition = Condition::LimitOrderFilled {
            owner: Addr::unchecked("owner"),
            pair_address,
            side: Side::Base,
            price: Decimal::percent(100),
        };

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractQueryResult::Ok(
                to_json_binary(&ConfigResponse {
                    denoms: Denoms::new("btc-btc", "rune"),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(1),
                    fee_taker: Decimal::percent(10),
                    fee_maker: Decimal::percent(5),
                    fee_address: Addr::unchecked("fee_address").to_string(),
                })
                .unwrap(),
            ))
        });

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[]),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap_err()
        .to_string()
        .contains("No funds sent for limit order"));

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[Coin::new(1213_u128, "random-denom")]),
            SchedulerExecuteMsg::Create(condition),
        )
        .unwrap_err()
        .to_string()
        .contains("No funds sent for limit order"));
    }

    #[test]
    fn fails_to_create_limit_order_trigger_if_pair_does_not_exist() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let info = message_info(&owner, &[Coin::new(1234_u128, "rune")]);

        let pair_address = Addr::unchecked("pair-0");

        let condition = Condition::LimitOrderFilled {
            owner: Addr::unchecked("owner"),
            pair_address: pair_address.clone(),
            side: Side::Base,
            price: Decimal::percent(100),
        };

        assert!(execute(
            deps.as_mut(),
            env.clone(),
            info,
            SchedulerExecuteMsg::Create(condition),
        )
        .unwrap_err()
        .to_string()
        .contains(format!("Querier system error: No such contract: {}", pair_address).as_str()));
    }

    #[test]
    fn sets_limit_order_with_provided_bid_denom_funds() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let bid_amount = Coin::new(1234_u128, "btc-btc");
        let info = message_info(&owner, &[bid_amount.clone()]);

        let pair_address = Addr::unchecked("pair-0");
        let side = Side::Base;
        let price = Decimal::percent(100);

        let condition = Condition::LimitOrderFilled {
            owner: Addr::unchecked("owner"),
            pair_address: pair_address.clone(),
            side: side.clone(),
            price: price.clone(),
        };

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractQueryResult::Ok(
                to_json_binary(&ConfigResponse {
                    denoms: Denoms::new("btc-btc", "rune"),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(1),
                    fee_taker: Decimal::percent(10),
                    fee_maker: Decimal::percent(5),
                    fee_address: Addr::unchecked("fee_address").to_string(),
                })
                .unwrap(),
            ))
        });

        let response = execute(
            deps.as_mut(),
            env.clone(),
            info,
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        assert_eq!(
            response.messages,
            vec![SubMsg::reply_never(
                Contract(pair_address.clone()).call(
                    to_json_binary(&ExecuteMsg::Order((
                        vec![(side, Price::Fixed(price), Some(bid_amount.amount.clone()),)],
                        None,
                    )))
                    .unwrap(),
                    vec![bid_amount],
                ),
            )]
        )
    }

    #[test]
    fn saves_remaining_funds_as_limit_order_trigger_execution_rebate() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let owner = deps.api.addr_make("creator");
        let info = message_info(
            &owner,
            &[
                Coin::new(1234_u128, "rune"),
                Coin::new(1234_u128, "eth-eth"),
            ],
        );

        let pair_address = Addr::unchecked("pair-0");

        let condition = Condition::LimitOrderFilled {
            owner: Addr::unchecked("owner"),
            pair_address: pair_address.clone(),
            side: Side::Quote,
            price: Decimal::percent(100),
        };

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractQueryResult::Ok(
                to_json_binary(&ConfigResponse {
                    denoms: Denoms::new("btc-btc", "rune"),
                    oracles: None,
                    market_maker: None,
                    tick: Tick::new(1),
                    fee_taker: Decimal::percent(10),
                    fee_maker: Decimal::percent(5),
                    fee_address: Addr::unchecked("fee_address").to_string(),
                })
                .unwrap(),
            ))
        });

        execute(
            deps.as_mut(),
            env.clone(),
            info,
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        let triggers = TRIGGERS.owned(deps.as_ref().storage, owner.clone(), None, None);

        assert_eq!(
            triggers,
            vec![Trigger {
                id: condition.id(owner.clone()).unwrap(),
                owner: owner.clone(),
                condition: condition.clone(),
                execution_rebate: vec![Coin::new(1234_u128, "eth-eth")],
            }]
        );
    }
}

#[cfg(test)]
mod execute_trigger_tests {
    use super::*;
    use calc_rs::conditions::Condition;
    use cosmwasm_std::testing::message_info;
    use cosmwasm_std::{from_json, Addr, Decimal, Uint128, WasmMsg, WasmQuery};
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Coin, ContractResult as CosmosContractResult, SubMsg, SystemResult,
    };
    use rujira_rs::fin::{ConfigResponse, Denoms, OrderResponse, Price, QueryMsg, Side, Tick};

    #[test]
    fn returns_error_if_trigger_does_not_exist() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let execution_info = MessageInfo {
            sender: deps.api.addr_make("executor"),
            funds: vec![],
        };

        let err = execute(
            deps.as_mut(),
            env.clone(),
            execution_info.clone(),
            SchedulerExecuteMsg::Execute(vec![1]),
        )
        .unwrap_err();

        assert!(err.to_string().contains("Failed to load trigger"));
    }

    #[test]
    fn fails_silently_if_trigger_cannot_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let manager = deps.api.addr_make("creator");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let condition =
            Condition::BlocksCompleted(cosmwasm_std::testing::mock_env().block.height + 10);

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&owner, &[Coin::new(327612u128, "rune")]),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![condition.id(owner.clone()).unwrap()]),
        )
        .unwrap();

        assert!(response.messages.is_empty());
    }

    #[test]
    fn withdraws_limit_order_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let manager = deps.api.addr_make("manager");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");

        let side = Side::Base;
        let price = Decimal::percent(100);

        let condition = Condition::LimitOrderFilled {
            owner: Addr::unchecked("owner"),
            pair_address: Addr::unchecked("pair-0"),
            side: side.clone(),
            price: price.clone(),
        };

        deps.querier.update_wasm(move |query| {
            SystemResult::Ok(CosmosContractResult::Ok(match query {
                WasmQuery::Smart { msg, .. } => match from_json(msg).unwrap() {
                    QueryMsg::Config {} => to_json_binary(&ConfigResponse {
                        denoms: Denoms::new("btc-btc", "rune"),
                        oracles: None,
                        market_maker: None,
                        tick: Tick::new(1),
                        fee_taker: Decimal::percent(10),
                        fee_maker: Decimal::percent(5),
                        fee_address: Addr::unchecked("fee_address").to_string(),
                    })
                    .unwrap(),
                    QueryMsg::Order(_) => to_json_binary(&OrderResponse {
                        filled: Uint128::new(21312),
                        owner: Addr::unchecked("contract").to_string(),
                        rate: Decimal::percent(10),
                        updated_at: env.block.time,
                        offer: Uint128::new(12321),
                        remaining: Uint128::zero(),
                        side: Side::Base,
                        price: Price::Fixed(Decimal::percent(100)),
                    })
                    .unwrap(),
                    _ => panic!("Unexpected query type"),
                },
                _ => panic!("Unexpected query type"),
            }))
        });

        let remaining_rebate = Coin::new(12312u128, "other");

        TRIGGERS
            .save(
                deps.as_mut().storage,
                condition.id(owner.clone()).unwrap(),
                owner.clone(),
                condition.clone(),
                vec![remaining_rebate.clone()],
            )
            .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![condition.id(owner.clone()).unwrap()]),
        )
        .unwrap();

        assert_eq!(
            response.messages,
            vec![
                SubMsg::reply_never(
                    Contract(Addr::unchecked("pair-0")).call(
                        to_json_binary(&ExecuteMsg::Order((
                            vec![(side.clone(), Price::Fixed(price.clone()), None)],
                            None,
                        )))
                        .unwrap(),
                        vec![],
                    )
                ),
                SubMsg::reply_never(BankMsg::Send {
                    to_address: executor.to_string(),
                    amount: vec![Coin::new(21312u128, "rune")],
                }),
                SubMsg::reply_on_error(
                    WasmMsg::Execute {
                        contract_addr: manager.to_string(),
                        msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                            contract_address: owner.clone(),
                        })
                        .unwrap(),
                        funds: vec![],
                    },
                    LOG_ERRORS_REPLY_ID,
                ),
                SubMsg::reply_never(BankMsg::Send {
                    to_address: executor.to_string(),
                    amount: vec![remaining_rebate],
                }),
            ]
        )
    }

    #[test]
    fn adds_execute_message_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let manager = deps.api.addr_make("creator");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        let condition = Condition::BlocksCompleted(env.block.height - 10);

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![condition.id(owner.clone()).unwrap()]),
        )
        .unwrap();

        assert!(response.messages.contains(&SubMsg::reply_on_error(
            WasmMsg::Execute {
                contract_addr: manager.to_string(),
                msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                    contract_address: owner.clone(),
                })
                .unwrap(),
                funds: vec![]
            },
            LOG_ERRORS_REPLY_ID
        )));
    }

    #[test]
    fn adds_send_rebate_msg_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let manager = deps.api.addr_make("creator");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        let condition = Condition::BlocksCompleted(env.block.height - 10);

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        let response = execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![condition.id(owner.clone()).unwrap()]),
        )
        .unwrap();

        assert!(response.messages.contains(&SubMsg::reply_on_error(
            WasmMsg::Execute {
                contract_addr: manager.to_string(),
                msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                    contract_address: owner.clone(),
                })
                .unwrap(),
                funds: vec![]
            },
            LOG_ERRORS_REPLY_ID
        )));

        assert!(response
            .messages
            .contains(&SubMsg::reply_never(BankMsg::Send {
                to_address: executor.to_string(),
                amount: create_trigger_info.funds.clone(),
            })));
    }

    #[test]
    fn deletes_trigger_if_trigger_can_execute() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let manager = deps.api.addr_make("manager");

        MANAGER.save(deps.as_mut().storage, &manager).unwrap();

        let owner = deps.api.addr_make("creator");
        let executor = deps.api.addr_make("executor");
        let create_trigger_info = message_info(&owner, &[Coin::new(235463u128, "rune")]);

        let condition = Condition::BlocksCompleted(env.block.height - 10);

        execute(
            deps.as_mut(),
            env.clone(),
            create_trigger_info.clone(),
            SchedulerExecuteMsg::Create(condition.clone()),
        )
        .unwrap();

        let triggers = TRIGGERS.owned(deps.as_ref().storage, owner.clone(), None, None);
        assert!(!triggers.is_empty());

        execute(
            deps.as_mut(),
            env.clone(),
            message_info(&executor, &[]),
            SchedulerExecuteMsg::Execute(vec![condition.id(owner.clone()).unwrap()]),
        )
        .unwrap();

        let triggers = TRIGGERS.owned(deps.as_ref().storage, owner.clone(), None, None);
        assert!(triggers.is_empty());
    }
}

#[cfg(test)]
mod owned_triggers_tests {
    use calc_rs::{conditions::Condition, scheduler::Trigger};
    use cosmwasm_std::{
        from_json,
        testing::{mock_dependencies, mock_env},
        Addr,
    };

    use super::*;

    #[test]
    fn fetches_owned_triggers() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let mut id = 0;

        for i in 1..=5 {
            id += 1;
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    id,
                    Addr::unchecked("owner"),
                    Condition::BlocksCompleted(env.block.height),
                    vec![],
                )
                .unwrap();

            id += 1;
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    id,
                    Addr::unchecked(format!("other-owner-{i}")),
                    Condition::BlocksCompleted(env.block.height),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Owned {
                    owner: Addr::unchecked("owner"),
                    limit: None,
                    start_after: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (0..5)
                .map(|i| Trigger {
                    id: i * 2 + 1, // odd ids for the owner
                    owner: Addr::unchecked("owner"),
                    condition: Condition::BlocksCompleted(env.block.height),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_owned_triggers_after_start_after() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    i,
                    Addr::unchecked("owner"),
                    Condition::BlocksCompleted(env.block.height),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Owned {
                    owner: Addr::unchecked("owner"),
                    limit: None,
                    start_after: Some(2),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (3..=5)
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::BlocksCompleted(env.block.height),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_owned_triggers_with_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    i,
                    Addr::unchecked("owner"),
                    Condition::BlocksCompleted(env.block.height),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Owned {
                    owner: Addr::unchecked("owner"),
                    limit: Some(3),
                    start_after: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (1..=3)
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::BlocksCompleted(env.block.height),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_owned_triggers_with_limit_and_start_after() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    i,
                    Addr::unchecked("owner"),
                    Condition::BlocksCompleted(env.block.height),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Owned {
                    owner: Addr::unchecked("owner"),
                    limit: Some(3),
                    start_after: Some(1),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (2..=4)
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::BlocksCompleted(env.block.height),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }
}

#[cfg(test)]
mod filtered_triggers_tests {
    use std::str::FromStr;

    use super::*;

    use calc_rs::{
        conditions::Condition,
        scheduler::{ConditionFilter, Trigger},
    };
    use cosmwasm_std::{
        from_json,
        testing::{mock_dependencies, mock_env},
        Addr, Decimal,
    };
    use rujira_rs::fin::Side;

    #[test]
    fn fetches_triggers_with_timestamp_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    i,
                    Addr::unchecked("owner"),
                    Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10)),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::Timestamp {
                        start: Some(env.block.time.plus_seconds(20)),
                        end: Some(env.block.time.plus_seconds(50)),
                    },
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (3..=5)
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10),),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_timestamp_filter_and_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    i,
                    Addr::unchecked("owner"),
                    Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10)),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::Timestamp {
                        start: Some(env.block.time),
                        end: Some(env.block.time.plus_seconds(50)),
                    },
                    limit: Some(3),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (1..=3)
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::TimestampElapsed(env.block.time.plus_seconds(i * 10),),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_block_height_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    i,
                    Addr::unchecked("owner"),
                    Condition::BlocksCompleted(env.block.height + i * 10),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::BlockHeight {
                        start: Some(env.block.height + 20),
                        end: Some(env.block.height + 50),
                    },
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (3..=5)
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::BlocksCompleted(env.block.height + i * 10,),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_block_height_filter_and_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=5 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    i,
                    Addr::unchecked("owner"),
                    Condition::BlocksCompleted(env.block.height + i * 10),
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::BlockHeight {
                        start: Some(env.block.height + 10),
                        end: Some(env.block.height + 50),
                    },
                    limit: Some(3),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (2..=4)
                .map(|i| Trigger {
                    id: i,
                    owner: Addr::unchecked("owner"),
                    condition: Condition::BlocksCompleted(env.block.height + i * 10,),
                    execution_rebate: vec![],
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_pair_only_limit_order_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=10 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    i,
                    Addr::unchecked("owner"),
                    Condition::LimitOrderFilled {
                        owner: Addr::unchecked("owner"),
                        pair_address: Addr::unchecked(format!("pair-{}", i % 2)),
                        side: Side::Base,
                        price: Decimal::from_str(&i.to_string()).unwrap(),
                    },
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::LimitOrder {
                        pair_address: Addr::unchecked("pair-0"),
                        price_range: None,
                        start_after: Some(4),
                    },
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (3..=5)
                .map(|i| {
                    let j = i * 2;
                    Trigger {
                        id: j,
                        owner: Addr::unchecked("owner"),
                        condition: Condition::LimitOrderFilled {
                            owner: Addr::unchecked("owner"),
                            pair_address: Addr::unchecked("pair-0"),
                            side: Side::Base,
                            price: Decimal::from_str(&j.to_string()).unwrap(),
                        },
                        execution_rebate: vec![],
                    }
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_pair_and_price_range_limit_order_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=10 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    i,
                    Addr::unchecked("owner"),
                    Condition::LimitOrderFilled {
                        owner: Addr::unchecked("owner"),
                        pair_address: Addr::unchecked(format!("pair-{}", i % 2)),
                        side: Side::Base,
                        price: Decimal::from_str(&i.to_string()).unwrap(),
                    },
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::LimitOrder {
                        pair_address: Addr::unchecked("pair-0"),
                        price_range: Some((
                            Decimal::from_str("7.0").unwrap(),
                            Decimal::from_str("9.0").unwrap(),
                        )),
                        start_after: None,
                    },
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (4..=4)
                .map(|i| {
                    let j = i * 2;
                    Trigger {
                        id: j,
                        owner: Addr::unchecked("owner"),
                        condition: Condition::LimitOrderFilled {
                            owner: Addr::unchecked("owner"),
                            pair_address: Addr::unchecked("pair-0"),
                            side: Side::Base,
                            price: Decimal::from_str(&j.to_string()).unwrap(),
                        },
                        execution_rebate: vec![],
                    }
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_pair_and_price_range_and_start_after_limit_order_filter() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=10 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    i,
                    Addr::unchecked("owner"),
                    Condition::LimitOrderFilled {
                        owner: Addr::unchecked("owner"),
                        pair_address: Addr::unchecked(format!("pair-{}", i % 2)),
                        side: Side::Base,
                        price: Decimal::from_str(&i.to_string()).unwrap(),
                    },
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::LimitOrder {
                        pair_address: Addr::unchecked("pair-0"),
                        price_range: Some((
                            Decimal::from_str("1.0").unwrap(),
                            Decimal::from_str("9.0").unwrap(),
                        )),
                        start_after: Some(4),
                    },
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (1..=4)
                .map(|i| {
                    let j = i * 2;
                    Trigger {
                        id: j,
                        owner: Addr::unchecked("owner"),
                        condition: Condition::LimitOrderFilled {
                            owner: Addr::unchecked("owner"),
                            pair_address: Addr::unchecked("pair-0"),
                            side: Side::Base,
                            price: Decimal::from_str(&j.to_string()).unwrap(),
                        },
                        execution_rebate: vec![],
                    }
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fetches_triggers_with_limit_order_filter_and_limit() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        for i in 1..=10 {
            TRIGGERS
                .save(
                    deps.as_mut().storage,
                    i,
                    Addr::unchecked("owner"),
                    Condition::LimitOrderFilled {
                        owner: Addr::unchecked("owner"),
                        pair_address: Addr::unchecked(format!("pair-{}", i % 2)),
                        side: Side::Base,
                        price: Decimal::from_str(&i.to_string()).unwrap(),
                    },
                    vec![],
                )
                .unwrap();
        }

        let response = from_json::<Vec<Trigger>>(
            query(
                deps.as_ref(),
                env.clone(),
                SchedulerQueryMsg::Filtered {
                    filter: ConditionFilter::LimitOrder {
                        pair_address: Addr::unchecked("pair-0"),
                        price_range: Some((
                            Decimal::from_str("1.0").unwrap(),
                            Decimal::from_str("9.0").unwrap(),
                        )),
                        start_after: None,
                    },
                    limit: Some(1),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(
            response,
            (1..=1)
                .map(|i| {
                    let j = i * 2;
                    Trigger {
                        id: j,
                        owner: Addr::unchecked("owner"),
                        condition: Condition::LimitOrderFilled {
                            owner: Addr::unchecked("owner"),
                            pair_address: Addr::unchecked("pair-0"),
                            side: Side::Base,
                            price: Decimal::from_str(&j.to_string()).unwrap(),
                        },
                        execution_rebate: vec![],
                    }
                })
                .collect::<Vec<_>>()
        );
    }
}
