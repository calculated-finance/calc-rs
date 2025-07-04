use calc_rs::{
    core::{Contract, ContractError, ContractResult},
    events::DomainEvent,
    manager::{ManagerConfig, ManagerExecuteMsg, ManagerQueryMsg, Strategy, StrategyStatus},
    strategy::{StrategyExecuteMsg, StrategyInstantiateMsg},
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    instantiate2_address, to_json_binary, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo,
    Order, Response, StdError, StdResult, WasmMsg,
};
use cw_storage_plus::Bound;

use crate::state::{strategy_store, updated_at_cursor, CONFIG, STRATEGY_COUNTER};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: ManagerConfig,
) -> ContractResult {
    CONFIG.save(deps.storage, &msg)?;
    STRATEGY_COUNTER.save(deps.storage, &0)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, msg: ManagerConfig) -> ContractResult {
    CONFIG.save(deps.storage, &msg)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ManagerExecuteMsg,
) -> ContractResult {
    let mut messages: Vec<CosmosMsg> = Vec::new();
    let events: Vec<DomainEvent> = Vec::new();

    match msg.clone() {
        ManagerExecuteMsg::InstantiateStrategy {
            owner,
            label,
            affiliates,
            actions,
        } => {
            let config = CONFIG.load(deps.storage)?;
            let strategy_id =
                STRATEGY_COUNTER.update(deps.storage, |id| Ok::<u64, StdError>(id + 1))?;
            let salt = to_json_binary(&(owner.clone(), strategy_id, env.block.time.seconds()))?;
            let code_id = config.strategy_code_id;

            let contract_address = deps.api.addr_humanize(&instantiate2_address(
                deps.querier
                    .query_wasm_code_info(code_id)?
                    .checksum
                    .as_slice(),
                &deps.api.addr_canonicalize(env.contract.address.as_str())?,
                &salt,
            )?)?;

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    id: strategy_id,
                    owner: owner.clone(),
                    contract_address: contract_address.clone(),
                    created_at: env.block.time.seconds(),
                    updated_at: env.block.time.seconds(),
                    label: label.clone(),
                    status: StrategyStatus::Active,
                    affiliates: Vec::new(),
                },
            )?;

            let instantiate_strategy_msg = WasmMsg::Instantiate2 {
                admin: Some(owner.to_string()),
                code_id,
                label,
                msg: to_json_binary(&StrategyInstantiateMsg {
                    owner: info.sender,
                    affiliates,
                    actions,
                })?,
                funds: info.funds,
                salt,
            };

            messages.push(instantiate_strategy_msg.into());
        }
        ManagerExecuteMsg::ExecuteStrategy { contract_address } => {
            let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            let execute_msg = Contract(contract_address.clone())
                .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, info.funds);

            messages.push(execute_msg);
        }
        ManagerExecuteMsg::UpdateStrategy {
            contract_address,
            update,
        } => {
            let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            if strategy.owner != info.sender {
                return Err(ContractError::Unauthorized {});
            }

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            let update_msg = Contract(contract_address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::Update(update.clone()))?,
                info.funds,
            );

            messages.push(update_msg);
        }
        ManagerExecuteMsg::UpdateStrategyStatus {
            contract_address,
            status,
        } => {
            let strategy = strategy_store().load(deps.storage, contract_address.clone())?;

            if strategy.owner != info.sender && info.sender != strategy.contract_address {
                return Err(ContractError::Unauthorized {});
            }

            strategy_store().save(
                deps.storage,
                contract_address.clone(),
                &Strategy {
                    status: status.clone(),
                    updated_at: env.block.time.seconds(),
                    ..strategy
                },
            )?;

            let update_status_msg = Contract(contract_address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::UpdateStatus(status.clone()))?,
                info.funds,
            );

            messages.push(update_status_msg);
        }
    };

    Ok(Response::default()
        .add_messages(messages)
        .add_events(events))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: ManagerQueryMsg) -> StdResult<Binary> {
    match msg {
        ManagerQueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        ManagerQueryMsg::Strategy { address } => {
            to_json_binary(&strategy_store().load(deps.storage, address.clone())?)
        }
        ManagerQueryMsg::Strategies {
            owner,
            status,
            start_after,
            limit,
        } => {
            let partition = match owner {
                Some(owner) => match status {
                    Some(status) => strategy_store()
                        .idx
                        .owner_status_updated_at
                        .prefix((owner, status as u8)),
                    None => strategy_store().idx.owner_updated_at.prefix(owner),
                },
                None => match status {
                    Some(status) => strategy_store().idx.status_updated_at.prefix(status as u8),
                    None => strategy_store().idx.updated_at.prefix(()),
                },
            };

            let strategies = partition
                .range(
                    deps.storage,
                    None,
                    start_after
                        .map(|updated_at| Bound::exclusive(updated_at_cursor(updated_at, None))),
                    Order::Descending,
                )
                .take(match limit {
                    Some(limit) => match limit {
                        0..=30 => limit as usize,
                        _ => 30,
                    },
                    None => 30,
                })
                .flat_map(|result| result.map(|(_, strategy)| strategy))
                .collect::<Vec<Strategy>>();

            to_json_binary(&strategies)
        }
    }
}

// #[cfg(test)]
// mod instantiate_manager_tests {

//     use calc_rs::manager::{ManagerConfig, ManagerInstantiateMsg, StrategyType};
//     use cosmwasm_std::{
//         testing::{message_info, mock_dependencies, mock_env},
//         Addr, Coin,
//     };

//     use crate::{contract::instantiate, state::CONFIG};

//     #[test]
//     fn saves_config() {
//         let mut deps = mock_dependencies();

//         let msg = ManagerInstantiateMsg {
//             admin: Addr::unchecked("admin"),
//             code_ids: vec![(StrategyType::Twap, 3)],
//             affiliate_creation_fee: Coin::new(10u128, "rune"),
//             default_affiliate_bps: 2,
//             fee_collector: Addr::unchecked("fee_collector"),
//         };

//         instantiate(
//             deps.as_mut(),
//             mock_env(),
//             message_info(&Addr::unchecked("creator"), &[]),
//             msg.clone(),
//         )
//         .unwrap();

//         let config = CONFIG.load(deps.as_ref().storage).unwrap();

//         assert_eq!(
//             config,
//             ManagerConfig {
//                 code_ids: msg.code_ids,
//                 affiliate_creation_fee: msg.affiliate_creation_fee,
//                 default_affiliate_bps: msg.default_affiliate_bps,
//                 admin: msg.admin,
//                 fee_collector: msg.fee_collector,
//             }
//         );
//     }
// }

// #[cfg(test)]
// mod migrate_manager_tests {
//     use calc_rs::manager::{ManagerConfig, ManagerMigrateMsg, StrategyType};
//     use cosmwasm_std::{
//         testing::{mock_dependencies, mock_env},
//         Addr, Coin,
//     };

//     use crate::{contract::migrate, state::CONFIG};

//     #[test]
//     fn updates_config() {
//         let mut deps = mock_dependencies();

//         let existing_config = ManagerConfig {
//             admin: Addr::unchecked("admin"),
//             fee_collector: Addr::unchecked("fee_collector"),
//             affiliate_creation_fee: Coin::new(10u128, "rune"),
//             default_affiliate_bps: 2,
//             code_ids: vec![(StrategyType::Twap, 3)],
//         };

//         CONFIG
//             .save(deps.as_mut().storage, &existing_config)
//             .unwrap();

//         let msg = ManagerMigrateMsg {
//             fee_collector: Addr::unchecked("fee_collector_new"),
//             affiliate_creation_fee: Coin::new(4u128, "x/ruji"),
//             default_affiliate_bps: 4,
//             code_ids: vec![(StrategyType::Twap, 5)],
//         };

//         migrate(deps.as_mut(), mock_env(), msg.clone()).unwrap();

//         let config = CONFIG.load(deps.as_ref().storage).unwrap();

//         assert_eq!(
//             config,
//             ManagerConfig {
//                 admin: existing_config.admin,
//                 code_ids: msg.code_ids,
//                 affiliate_creation_fee: msg.affiliate_creation_fee,
//                 default_affiliate_bps: msg.default_affiliate_bps,
//                 fee_collector: msg.fee_collector,
//             }
//         );
//     }
// }

// #[cfg(test)]
// mod instantiate_strategy_tests {

//     use calc_rs::{
//         core::ContractError,
//         distributor::{Destination, Recipient},
//         manager::{
//             CreateStrategyConfig, DomainEvent, ManagerConfig, ManagerExecuteMsg, Strategy,
//             StrategyInstantiateMsg, StrategyStatus, StrategyType,
//         },
//         twap::InstantiateTwapCommand,
//     };
//     use calc_rs_test::test::CodeInfoResponse;
//     use cosmwasm_std::{
//         testing::{message_info, mock_dependencies, mock_env},
//         to_json_binary, Addr, Checksum, Coin, ContractResult, Event, Order, StdError, SubMsg,
//         SystemResult, Uint128, WasmMsg,
//     };

//     use crate::{
//         contract::execute,
//         state::{strategy_store, CONFIG, STRATEGY_COUNTER},
//     };

//     #[test]
//     fn fails_if_code_id_not_found() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         deps.querier.update_wasm(move |_| {
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&CodeInfoResponse {
//                     code_id: 3,
//                     creator: Addr::unchecked("creator"),
//                     checksum: Checksum::from_hex(
//                         "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
//                     )
//                     .unwrap(),
//                 })
//                 .unwrap(),
//             ))
//         });

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Ladder, 3)],
//                 },
//             )
//             .unwrap();

//         let msg = ManagerExecuteMsg::InstantiateStrategy {
//             owner: Addr::unchecked("owner"),
//             label: "label".to_string(),
//             strategy: CreateStrategyConfig::Twap(InstantiateTwapCommand {
//                 owner: deps.api.addr_make("owner"),
//                 exchanger_contract: deps.api.addr_make("exchanger"),
//                 scheduler_contract: deps.api.addr_make("scheduler"),
//                 swap_amount: Coin::new(1000u128, "rune"),
//                 minimum_receive_amount: Coin::new(900u128, "uruji"),
//                 maximum_slippage_bps: 100,
//                 route: None,
//                 swap_cadence: calc_rs::core::Schedule::Blocks {
//                     interval: 100,
//                     previous: None,
//                 },
//                 execution_rebate: None,
//                 minimum_distribute_amount: None,
//                 distributor_code_id: 1,
//                 affiliate_code: None,
//                 mutable_destinations: vec![Destination {
//                     shares: Uint128::new(10000),
//                     recipient: Recipient::Bank {
//                         address: Addr::unchecked("mutable_recipient"),
//                     },
//                     label: None,
//                 }],
//                 immutable_destinations: vec![],
//             }),
//         };

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("owner"), &[]),
//             msg,
//         )
//         .unwrap_err();

//         assert_eq!(
//             response,
//             ContractError::Std(StdError::generic_err(
//                 "Code ID for strategy type Twap not found"
//             ))
//         )
//     }

//     #[test]
//     fn creates_strategy_with_incremented_id() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         deps.querier.update_wasm(move |_| {
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&CodeInfoResponse {
//                     code_id: 3,
//                     creator: Addr::unchecked("creator"),
//                     checksum: Checksum::from_hex(
//                         "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
//                     )
//                     .unwrap(),
//                 })
//                 .unwrap(),
//             ))
//         });

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let msg = ManagerExecuteMsg::InstantiateStrategy {
//             owner: Addr::unchecked("owner"),
//             label: "label".to_string(),
//             strategy: CreateStrategyConfig::Twap(InstantiateTwapCommand {
//                 owner: deps.api.addr_make("owner"),
//                 exchanger_contract: deps.api.addr_make("exchanger"),
//                 scheduler_contract: deps.api.addr_make("scheduler"),
//                 swap_amount: Coin::new(1000u128, "rune"),
//                 minimum_receive_amount: Coin::new(900u128, "uruji"),
//                 maximum_slippage_bps: 100,
//                 route: None,
//                 swap_cadence: calc_rs::core::Schedule::Blocks {
//                     interval: 100,
//                     previous: None,
//                 },
//                 execution_rebate: None,
//                 minimum_distribute_amount: None,
//                 distributor_code_id: 1,
//                 affiliate_code: None,
//                 mutable_destinations: vec![Destination {
//                     shares: Uint128::new(10000),
//                     recipient: Recipient::Bank {
//                         address: Addr::unchecked("mutable_recipient"),
//                     },
//                     label: None,
//                 }],
//                 immutable_destinations: vec![],
//             }),
//         };

//         execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("owner"), &[]),
//             msg,
//         )
//         .unwrap();

//         let strategies = strategy_store()
//             .range(deps.as_ref().storage, None, None, Order::Ascending)
//             .take(2)
//             .flat_map(|result| result.map(|(_, strategy)| strategy))
//             .collect::<Vec<_>>();

//         assert_eq!(
//             strategies,
//             vec![Strategy {
//                 id: 1,
//                 owner: Addr::unchecked("owner"),
//                 label: "label".to_string(),
//                 status: StrategyStatus::Active,
//                 created_at: env.block.time.seconds(),
//                 updated_at: env.block.time.seconds(),
//                 contract_address: strategies[0].contract_address.clone(),
//                 affiliates: vec![]
//             }]
//         )
//     }

//     #[test]
//     fn adds_instantiate_strategy_msg() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         deps.querier.update_wasm(move |_| {
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&CodeInfoResponse {
//                     code_id: 3,
//                     creator: Addr::unchecked("creator"),
//                     checksum: Checksum::from_hex(
//                         "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
//                     )
//                     .unwrap(),
//                 })
//                 .unwrap(),
//             ))
//         });

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         let config = ManagerConfig {
//             admin: Addr::unchecked("admin"),
//             fee_collector: Addr::unchecked("fee_collector"),
//             affiliate_creation_fee: Coin::new(10u128, "rune"),
//             default_affiliate_bps: 2,
//             code_ids: vec![(StrategyType::Twap, 3)],
//         };

//         CONFIG.save(deps.as_mut().storage, &config).unwrap();

//         let owner = Addr::unchecked("owner");

//         let strategy = CreateStrategyConfig::Twap(InstantiateTwapCommand {
//             owner: owner.clone(),
//             exchanger_contract: deps.api.addr_make("exchanger"),
//             scheduler_contract: deps.api.addr_make("scheduler"),
//             swap_amount: Coin::new(1000u128, "rune"),
//             minimum_receive_amount: Coin::new(900u128, "uruji"),
//             maximum_slippage_bps: 100,
//             route: None,
//             swap_cadence: calc_rs::core::Schedule::Blocks {
//                 interval: 100,
//                 previous: None,
//             },
//             execution_rebate: None,
//             minimum_distribute_amount: None,
//             distributor_code_id: 1,
//             affiliate_code: None,
//             mutable_destinations: vec![Destination {
//                 shares: Uint128::new(10000),
//                 recipient: Recipient::Bank {
//                     address: Addr::unchecked("mutable_recipient"),
//                 },
//                 label: None,
//             }],
//             immutable_destinations: vec![],
//         });

//         let msg = ManagerExecuteMsg::InstantiateStrategy {
//             owner: owner.clone(),
//             label: "label".to_string(),
//             strategy: strategy.clone(),
//         };

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("owner"), &[]),
//             msg,
//         )
//         .unwrap();

//         assert_eq!(
//             response.messages[0],
//             SubMsg::new(WasmMsg::Instantiate2 {
//                 admin: Some(owner.to_string()),
//                 code_id: 3,
//                 label: "label".to_string(),
//                 msg: to_json_binary(&StrategyInstantiateMsg {
//                     fee_collector: config.fee_collector,
//                     config: strategy,
//                 })
//                 .unwrap(),
//                 funds: vec![],
//                 salt: to_json_binary(&(owner, 1, env.block.time.seconds())).unwrap(),
//             })
//         )
//     }

//     #[test]
//     fn publishes_strategy_instantiated_event() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         deps.querier.update_wasm(move |_| {
//             SystemResult::Ok(ContractResult::Ok(
//                 to_json_binary(&CodeInfoResponse {
//                     code_id: 3,
//                     creator: Addr::unchecked("creator"),
//                     checksum: Checksum::from_hex(
//                         "f7bb7b18fb01bbf425cf4ed2cd4b7fb26a019a7fc75a4dc87e8a0b768c501f00",
//                     )
//                     .unwrap(),
//                 })
//                 .unwrap(),
//             ))
//         });

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         let config = ManagerConfig {
//             admin: Addr::unchecked("admin"),
//             fee_collector: Addr::unchecked("fee_collector"),
//             affiliate_creation_fee: Coin::new(10u128, "rune"),
//             default_affiliate_bps: 2,
//             code_ids: vec![(StrategyType::Twap, 3)],
//         };

//         CONFIG.save(deps.as_mut().storage, &config).unwrap();

//         let owner = Addr::unchecked("owner");

//         let strategy = CreateStrategyConfig::Twap(InstantiateTwapCommand {
//             owner: owner.clone(),
//             exchanger_contract: deps.api.addr_make("exchanger"),
//             scheduler_contract: deps.api.addr_make("scheduler"),
//             swap_amount: Coin::new(1000u128, "rune"),
//             minimum_receive_amount: Coin::new(900u128, "uruji"),
//             maximum_slippage_bps: 100,
//             route: None,
//             swap_cadence: calc_rs::core::Schedule::Blocks {
//                 interval: 100,
//                 previous: None,
//             },
//             execution_rebate: None,
//             minimum_distribute_amount: None,
//             distributor_code_id: 1,
//             affiliate_code: None,
//             mutable_destinations: vec![Destination {
//                 shares: Uint128::new(10000),
//                 recipient: Recipient::Bank {
//                     address: Addr::unchecked("mutable_recipient"),
//                 },
//                 label: None,
//             }],
//             immutable_destinations: vec![],
//         });

//         let msg = ManagerExecuteMsg::InstantiateStrategy {
//             owner: owner.clone(),
//             label: "label".to_string(),
//             strategy: strategy.clone(),
//         };

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("owner"), &[]),
//             msg,
//         )
//         .unwrap();

//         let strategies = strategy_store()
//             .range(&mut deps.storage, None, None, Order::Ascending)
//             .flat_map(|result| result.map(|(_, strategy)| strategy))
//             .collect::<Vec<_>>();

//         assert_eq!(
//             response.events[0],
//             Event::from(DomainEvent::StrategyInstantiated {
//                 contract_address: strategies[0].contract_address.clone(),
//                 config: strategy,
//             })
//         )
//     }
// }

// #[cfg(test)]
// mod execute_strategy_tests {
//     use calc_rs::{
//         core::ContractError,
//         manager::{
//             DomainEvent, ManagerConfig, ManagerExecuteMsg, Strategy, StrategyStatus, StrategyType,
//         },
//     };
//     use cosmwasm_std::{
//         testing::{message_info, mock_dependencies, mock_env},
//         to_json_binary, Addr, Coin, Event, StdError, SubMsg, WasmMsg,
//     };

//     use crate::{
//         contract::execute,
//         state::{strategy_store, CONFIG, STRATEGY_COUNTER},
//     };

//     #[test]
//     fn fails_if_strategy_does_not_exist() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let contract_address = Addr::unchecked("non_existent_strategy");

//         let msg = ManagerExecuteMsg::ExecuteStrategy {
//             contract_address: contract_address.clone(),
//             msg: None,
//         };

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("anyone"), &[]),
//             msg,
//         )
//         .unwrap_err();

//         assert_eq!(
//             response,
//             ContractError::Std(StdError::generic_err(format!(
//                 "Strategy not found with address: {}",
//                 contract_address
//             )))
//         );
//     }

//     #[test]
//     fn updates_strategy_updated_at() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let strategy = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner"),
//             contract_address: Addr::unchecked("existing_strategy"),
//             created_at: 125654334,
//             updated_at: 125654334,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         strategy_store()
//             .save(
//                 deps.as_mut().storage,
//                 strategy.contract_address.clone(),
//                 &strategy,
//             )
//             .unwrap();

//         let msg = ManagerExecuteMsg::ExecuteStrategy {
//             contract_address: strategy.contract_address.clone(),
//             msg: None,
//         };

//         execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("anyone"), &[]),
//             msg,
//         )
//         .unwrap();

//         let strategy = strategy_store()
//             .load(deps.as_ref().storage, strategy.contract_address.clone())
//             .unwrap();

//         assert_eq!(strategy.updated_at, env.block.time.seconds());
//         assert_ne!(strategy.created_at, strategy.updated_at);
//     }

//     #[test]
//     fn sends_execute_strategy_msg() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let strategy = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner"),
//             contract_address: Addr::unchecked("existing_strategy"),
//             created_at: 125654334,
//             updated_at: 125654334,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         strategy_store()
//             .save(
//                 deps.as_mut().storage,
//                 strategy.contract_address.clone(),
//                 &strategy,
//             )
//             .unwrap();

//         let strategy_msg_payload = Some(to_json_binary(&"test message").unwrap());
//         let msg = ManagerExecuteMsg::ExecuteStrategy {
//             contract_address: strategy.contract_address.clone(),
//             msg: strategy_msg_payload.clone(),
//         };

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("anyone"), &[]),
//             msg,
//         )
//         .unwrap();

//         assert_eq!(
//             response.messages[0],
//             SubMsg::new(WasmMsg::Execute {
//                 contract_addr: strategy.contract_address.to_string(),
//                 msg: to_json_binary(&calc_rs::manager::StrategyExecuteMsg::Execute {
//                     msg: strategy_msg_payload
//                 })
//                 .unwrap(),
//                 funds: vec![],
//             })
//         );
//     }

//     #[test]
//     fn publishes_strategy_executed_event() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let strategy = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner"),
//             contract_address: Addr::unchecked("existing_strategy"),
//             created_at: 125654334,
//             updated_at: 125654334,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         strategy_store()
//             .save(
//                 deps.as_mut().storage,
//                 strategy.contract_address.clone(),
//                 &strategy,
//             )
//             .unwrap();

//         let strategy_msg_payload = Some(to_json_binary(&"test message").unwrap());
//         let msg = ManagerExecuteMsg::ExecuteStrategy {
//             contract_address: strategy.contract_address.clone(),
//             msg: strategy_msg_payload.clone(),
//         };

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("anyone"), &[]),
//             msg,
//         )
//         .unwrap();

//         assert_eq!(
//             response.events[0],
//             Event::from(DomainEvent::StrategyExecuted {
//                 contract_address: strategy.contract_address,
//             })
//         );
//     }
// }

// #[cfg(test)]
// mod update_strategy_tests {
//     use calc_rs::{
//         core::ContractError,
//         manager::{
//             DomainEvent, ManagerConfig, ManagerExecuteMsg, Strategy, StrategyConfig,
//             StrategyExecuteMsg, StrategyStatus, StrategyType,
//         },
//         twap::TwapConfig,
//     };
//     use cosmwasm_std::{
//         testing::{message_info, mock_dependencies, mock_env},
//         to_json_binary, Addr, Coin, Event, SubMsg, WasmMsg,
//     };

//     use crate::{
//         contract::execute,
//         state::{strategy_store, CONFIG, STRATEGY_COUNTER},
//     };

//     #[test]
//     fn fails_if_sender_not_owner() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let contract_address = Addr::unchecked("existing_strategy");

//         let strategy = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner"),
//             contract_address: contract_address.clone(),
//             created_at: 125654334,
//             updated_at: 125654334,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: vec![],
//         };

//         strategy_store()
//             .save(
//                 deps.as_mut().storage,
//                 strategy.contract_address.clone(),
//                 &strategy,
//             )
//             .unwrap();

//         let msg = ManagerExecuteMsg::UpdateStrategy {
//             contract_address: contract_address.clone(),
//             update: StrategyConfig::Twap(TwapConfig {
//                 owner: deps.api.addr_make("owner"),
//                 manager_contract: deps.api.addr_make("manager"),
//                 exchanger_contract: deps.api.addr_make("exchanger"),
//                 scheduler_contract: deps.api.addr_make("scheduler"),
//                 distributor_contract: deps.api.addr_make("distributor"),
//                 swap_amount: Coin::new(1000u128, "rune"),
//                 minimum_receive_amount: Coin::new(900u128, "uruji"),
//                 maximum_slippage_bps: 100,
//                 route: None,
//                 swap_cadence: calc_rs::core::Schedule::Blocks {
//                     interval: 100,
//                     previous: None,
//                 },
//                 swap_conditions: vec![],
//                 schedule_conditions: vec![],
//                 execution_rebate: None,
//             }),
//         };

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("not_owner"), &[]),
//             msg.clone(),
//         )
//         .unwrap_err();

//         assert_eq!(response, ContractError::Unauthorized {});

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&strategy.owner, &[]),
//             msg,
//         );

//         assert!(response.is_ok());
//     }

//     #[test]
//     fn updates_strategy_updated_at() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let strategy = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner"),
//             contract_address: Addr::unchecked("existing_strategy"),
//             created_at: 125654334,
//             updated_at: 125654334,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         strategy_store()
//             .save(
//                 deps.as_mut().storage,
//                 strategy.contract_address.clone(),
//                 &strategy,
//             )
//             .unwrap();

//         let update = StrategyConfig::Twap(TwapConfig {
//             owner: deps.api.addr_make("owner"),
//             manager_contract: deps.api.addr_make("manager"),
//             exchanger_contract: deps.api.addr_make("exchanger"),
//             scheduler_contract: deps.api.addr_make("scheduler"),
//             distributor_contract: deps.api.addr_make("distributor"),
//             swap_amount: Coin::new(1000u128, "rune"),
//             minimum_receive_amount: Coin::new(900u128, "uruji"),
//             maximum_slippage_bps: 100,
//             route: None,
//             swap_cadence: calc_rs::core::Schedule::Blocks {
//                 interval: 100,
//                 previous: None,
//             },
//             swap_conditions: vec![],
//             schedule_conditions: vec![],
//             execution_rebate: None,
//         });

//         let msg = ManagerExecuteMsg::UpdateStrategy {
//             contract_address: strategy.contract_address.clone(),
//             update: update.clone(),
//         };

//         execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&strategy.owner.clone(), &[]),
//             msg,
//         )
//         .unwrap();

//         let strategy = strategy_store()
//             .load(deps.as_ref().storage, strategy.contract_address.clone())
//             .unwrap();

//         assert_eq!(strategy.updated_at, env.block.time.seconds());
//         assert_ne!(strategy.created_at, strategy.updated_at);
//     }

//     #[test]
//     fn sends_update_strategy_msg() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let contract_address = Addr::unchecked("existing_strategy");

//         let strategy = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner"),
//             contract_address: contract_address.clone(),
//             created_at: 125654334,
//             updated_at: 125654334,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: vec![],
//         };

//         strategy_store()
//             .save(
//                 deps.as_mut().storage,
//                 strategy.contract_address.clone(),
//                 &strategy,
//             )
//             .unwrap();

//         let update = StrategyConfig::Twap(TwapConfig {
//             owner: deps.api.addr_make("owner"),
//             manager_contract: deps.api.addr_make("manager"),
//             exchanger_contract: deps.api.addr_make("exchanger"),
//             scheduler_contract: deps.api.addr_make("scheduler"),
//             distributor_contract: deps.api.addr_make("distributor"),
//             swap_amount: Coin::new(1000u128, "rune"),
//             minimum_receive_amount: Coin::new(900u128, "uruji"),
//             maximum_slippage_bps: 100,
//             route: None,
//             swap_cadence: calc_rs::core::Schedule::Blocks {
//                 interval: 100,
//                 previous: None,
//             },
//             swap_conditions: vec![],
//             schedule_conditions: vec![],
//             execution_rebate: None,
//         });

//         let msg = ManagerExecuteMsg::UpdateStrategy {
//             contract_address: contract_address.clone(),
//             update: update.clone(),
//         };

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("owner"), &[]),
//             msg,
//         )
//         .unwrap();

//         assert_eq!(
//             response.messages[0],
//             SubMsg::new(WasmMsg::Execute {
//                 contract_addr: contract_address.to_string(),
//                 msg: to_json_binary(&StrategyExecuteMsg::Update(update)).unwrap(),
//                 funds: vec![],
//             })
//         );
//     }

//     #[test]
//     fn publishes_strategy_updated_event() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let contract_address = Addr::unchecked("existing_strategy");

//         let strategy = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner"),
//             contract_address: contract_address.clone(),
//             created_at: 125654334,
//             updated_at: 125654334,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: vec![],
//         };

//         strategy_store()
//             .save(
//                 deps.as_mut().storage,
//                 strategy.contract_address.clone(),
//                 &strategy,
//             )
//             .unwrap();

//         let update = StrategyConfig::Twap(TwapConfig {
//             owner: deps.api.addr_make("owner"),
//             manager_contract: deps.api.addr_make("manager"),
//             exchanger_contract: deps.api.addr_make("exchanger"),
//             scheduler_contract: deps.api.addr_make("scheduler"),
//             distributor_contract: deps.api.addr_make("distributor"),
//             swap_amount: Coin::new(1000u128, "rune"),
//             minimum_receive_amount: Coin::new(900u128, "uruji"),
//             maximum_slippage_bps: 100,
//             route: None,
//             swap_cadence: calc_rs::core::Schedule::Blocks {
//                 interval: 100,
//                 previous: None,
//             },
//             swap_conditions: vec![],
//             schedule_conditions: vec![],
//             execution_rebate: None,
//         });

//         let msg = ManagerExecuteMsg::UpdateStrategy {
//             contract_address: contract_address.clone(),
//             update: update.clone(),
//         };

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("owner"), &[]),
//             msg,
//         )
//         .unwrap();

//         assert_eq!(
//             response.events[0],
//             Event::from(DomainEvent::StrategyUpdated {
//                 contract_address,
//                 update: update,
//             })
//         );
//     }
// }

// #[cfg(test)]
// mod update_strategy_status_tests {
//     use calc_rs::{
//         core::ContractError,
//         manager::{
//             DomainEvent, ManagerConfig, ManagerExecuteMsg, Strategy, StrategyExecuteMsg,
//             StrategyStatus, StrategyType,
//         },
//     };
//     use cosmwasm_std::{
//         testing::{message_info, mock_dependencies, mock_env},
//         to_json_binary, Addr, Coin, Event, SubMsg, WasmMsg,
//     };

//     use crate::{
//         contract::execute,
//         state::{strategy_store, CONFIG, STRATEGY_COUNTER},
//     };

//     #[test]
//     fn fails_if_sender_not_owner_or_strategy() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let contract_address = Addr::unchecked("existing_strategy");

//         let strategy = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner"),
//             contract_address: contract_address.clone(),
//             created_at: 125654334,
//             updated_at: 125654334,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: vec![],
//         };

//         strategy_store()
//             .save(
//                 deps.as_mut().storage,
//                 strategy.contract_address.clone(),
//                 &strategy,
//             )
//             .unwrap();

//         let msg = ManagerExecuteMsg::UpdateStrategyStatus {
//             contract_address: strategy.contract_address.clone(),
//             status: StrategyStatus::Archived,
//         };

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("not_owner"), &[]),
//             msg.clone(),
//         )
//         .unwrap_err();

//         assert_eq!(response, ContractError::Unauthorized {});

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&strategy.owner.clone(), &[]),
//             msg.clone(),
//         );

//         assert!(response.is_ok());

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&strategy.contract_address.clone(), &[]),
//             msg.clone(),
//         );

//         assert!(response.is_ok());
//     }

//     #[test]
//     fn updates_strategy_updated_at() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let strategy = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner"),
//             contract_address: Addr::unchecked("existing_strategy"),
//             created_at: 125654334,
//             updated_at: 125654334,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         strategy_store()
//             .save(
//                 deps.as_mut().storage,
//                 strategy.contract_address.clone(),
//                 &strategy,
//             )
//             .unwrap();

//         let msg = ManagerExecuteMsg::UpdateStrategyStatus {
//             contract_address: strategy.contract_address.clone(),
//             status: StrategyStatus::Archived,
//         };

//         execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&strategy.owner.clone(), &[]),
//             msg,
//         )
//         .unwrap();

//         let strategy = strategy_store()
//             .load(deps.as_ref().storage, strategy.contract_address.clone())
//             .unwrap();

//         assert_eq!(strategy.status, StrategyStatus::Archived);
//     }

//     #[test]
//     fn sends_update_strategy_msg() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let contract_address = Addr::unchecked("existing_strategy");

//         let strategy = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner"),
//             contract_address: contract_address.clone(),
//             created_at: 125654334,
//             updated_at: 125654334,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: vec![],
//         };

//         strategy_store()
//             .save(
//                 deps.as_mut().storage,
//                 strategy.contract_address.clone(),
//                 &strategy,
//             )
//             .unwrap();

//         let new_status = StrategyStatus::Archived;

//         let msg = ManagerExecuteMsg::UpdateStrategyStatus {
//             contract_address: strategy.contract_address.clone(),
//             status: new_status.clone(),
//         };

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("owner"), &[]),
//             msg,
//         )
//         .unwrap();

//         assert_eq!(
//             response.messages[0],
//             SubMsg::new(WasmMsg::Execute {
//                 contract_addr: contract_address.to_string(),
//                 msg: to_json_binary(&StrategyExecuteMsg::UpdateStatus(new_status)).unwrap(),
//                 funds: vec![],
//             })
//         );
//     }

//     #[test]
//     fn publishes_strategy_updated_event() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         STRATEGY_COUNTER.save(deps.as_mut().storage, &0).unwrap();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let contract_address = Addr::unchecked("existing_strategy");

//         let strategy = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner"),
//             contract_address: contract_address.clone(),
//             created_at: 125654334,
//             updated_at: 125654334,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: vec![],
//         };

//         strategy_store()
//             .save(
//                 deps.as_mut().storage,
//                 strategy.contract_address.clone(),
//                 &strategy,
//             )
//             .unwrap();

//         let new_status = StrategyStatus::Archived;

//         let msg = ManagerExecuteMsg::UpdateStrategyStatus {
//             contract_address: strategy.contract_address.clone(),
//             status: new_status.clone(),
//         };

//         let response = execute(
//             deps.as_mut(),
//             env.clone(),
//             message_info(&Addr::unchecked("owner"), &[]),
//             msg,
//         )
//         .unwrap();

//         assert_eq!(
//             response.events[0],
//             Event::from(DomainEvent::StrategyStatusUpdated {
//                 contract_address,
//                 status: new_status,
//             })
//         );
//     }
// }

// #[cfg(test)]
// mod add_affiliate_tests {
//     use calc_rs::{
//         core::ContractError,
//         manager::{Affiliate, ManagerConfig, ManagerExecuteMsg, StrategyType},
//     };
//     use cosmwasm_std::{
//         testing::{message_info, mock_dependencies, mock_env},
//         Addr, Coin,
//     };

//     use crate::{
//         contract::execute,
//         state::{AFFILIATES, CONFIG},
//     };

//     #[test]
//     fn fails_when_affiliate_already_exists() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         CONFIG
//             .save(
//                 deps.as_mut().storage,
//                 &ManagerConfig {
//                     admin: Addr::unchecked("admin"),
//                     fee_collector: Addr::unchecked("fee_collector"),
//                     affiliate_creation_fee: Coin::new(10u128, "rune"),
//                     default_affiliate_bps: 2,
//                     code_ids: vec![(StrategyType::Twap, 3)],
//                 },
//             )
//             .unwrap();

//         let affiliate = Affiliate {
//             code: "affiliate_code".to_string(),
//             address: Addr::unchecked("affiliate_address"),
//             bps: 2,
//         };

//         AFFILIATES
//             .save(deps.as_mut().storage, affiliate.code.clone(), &affiliate)
//             .unwrap();

//         let response = execute(
//             deps.as_mut(),
//             env,
//             message_info(&Addr::unchecked("sender"), &[Coin::new(10u128, "rune")]),
//             ManagerExecuteMsg::AddAffiliate {
//                 code: affiliate.code.clone(),
//                 address: Addr::unchecked("not_affiliate_address"),
//                 bps: affiliate.bps,
//             },
//         )
//         .unwrap_err();

//         assert_eq!(
//             response,
//             ContractError::generic_err(format!(
//                 "Affiliate code {} already exists with a different address",
//                 affiliate.code
//             ))
//         );
//     }

//     #[test]
//     fn fails_when_deposit_not_provided() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let config = ManagerConfig {
//             admin: Addr::unchecked("admin"),
//             fee_collector: Addr::unchecked("fee_collector"),
//             affiliate_creation_fee: Coin::new(10u128, "rune"),
//             default_affiliate_bps: 2,
//             code_ids: vec![(StrategyType::Twap, 3)],
//         };

//         CONFIG.save(deps.as_mut().storage, &config).unwrap();

//         let affiliate = Affiliate {
//             code: "affiliate_code".to_string(),
//             address: Addr::unchecked("affiliate_address"),
//             bps: 2,
//         };

//         let response = execute(
//             deps.as_mut(),
//             env,
//             message_info(&Addr::unchecked("sender"), &[Coin::new(9u128, "rune")]),
//             ManagerExecuteMsg::AddAffiliate {
//                 code: affiliate.code.clone(),
//                 address: affiliate.address.clone(),
//                 bps: affiliate.bps,
//             },
//         )
//         .unwrap_err();

//         assert_eq!(
//             response,
//             ContractError::generic_err(format!(
//                 "Must include at least {:?} to create an affiliate",
//                 config.affiliate_creation_fee
//             ))
//         );
//     }

//     #[test]
//     fn fails_when_setting_bps_above_10() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let config = ManagerConfig {
//             admin: Addr::unchecked("admin"),
//             fee_collector: Addr::unchecked("fee_collector"),
//             affiliate_creation_fee: Coin::new(10u128, "rune"),
//             default_affiliate_bps: 2,
//             code_ids: vec![(StrategyType::Twap, 3)],
//         };

//         CONFIG.save(deps.as_mut().storage, &config).unwrap();

//         let affiliate = Affiliate {
//             code: "affiliate_code".to_string(),
//             address: Addr::unchecked("affiliate_address"),
//             bps: 20,
//         };

//         let response = execute(
//             deps.as_mut(),
//             env,
//             message_info(&Addr::unchecked("sender"), &[Coin::new(10u128, "rune")]),
//             ManagerExecuteMsg::AddAffiliate {
//                 code: affiliate.code.clone(),
//                 address: affiliate.address.clone(),
//                 bps: affiliate.bps,
//             },
//         )
//         .unwrap_err();

//         assert_eq!(
//             response,
//             ContractError::generic_err(format!(
//                 "Affiliate fee basis points cannot exceed 10 (0.1%)",
//             ))
//         );
//     }

//     #[test]
//     fn fails_when_non_admin_setting_bps_above_default() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let config = ManagerConfig {
//             admin: Addr::unchecked("admin"),
//             fee_collector: Addr::unchecked("fee_collector"),
//             affiliate_creation_fee: Coin::new(10u128, "rune"),
//             default_affiliate_bps: 2,
//             code_ids: vec![(StrategyType::Twap, 3)],
//         };

//         CONFIG.save(deps.as_mut().storage, &config).unwrap();

//         let affiliate = Affiliate {
//             code: "affiliate_code".to_string(),
//             address: Addr::unchecked("affiliate_address"),
//             bps: 5,
//         };

//         let response = execute(
//             deps.as_mut(),
//             env,
//             message_info(&Addr::unchecked("sender"), &[Coin::new(10u128, "rune")]),
//             ManagerExecuteMsg::AddAffiliate {
//                 code: affiliate.code.clone(),
//                 address: affiliate.address.clone(),
//                 bps: affiliate.bps,
//             },
//         )
//         .unwrap_err();

//         assert_eq!(
//             response,
//             ContractError::generic_err(format!(
//                 "Only the admin can create affiliates with more than the default bps ({})",
//                 config.default_affiliate_bps
//             ))
//         );
//     }

//     #[test]
//     fn creates_affiliate_with_default_bps() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let config = ManagerConfig {
//             admin: Addr::unchecked("admin"),
//             fee_collector: Addr::unchecked("fee_collector"),
//             affiliate_creation_fee: Coin::new(10u128, "rune"),
//             default_affiliate_bps: 2,
//             code_ids: vec![(StrategyType::Twap, 3)],
//         };

//         CONFIG.save(deps.as_mut().storage, &config).unwrap();

//         let affiliate = Affiliate {
//             code: "affiliate_code".to_string(),
//             address: Addr::unchecked("affiliate_address"),
//             bps: config.default_affiliate_bps,
//         };

//         execute(
//             deps.as_mut(),
//             env,
//             message_info(&Addr::unchecked("sender"), &[Coin::new(10u128, "rune")]),
//             ManagerExecuteMsg::AddAffiliate {
//                 code: affiliate.code.clone(),
//                 address: affiliate.address.clone(),
//                 bps: affiliate.bps,
//             },
//         )
//         .unwrap();

//         assert_eq!(
//             AFFILIATES
//                 .load(deps.as_ref().storage, affiliate.code.clone())
//                 .unwrap(),
//             affiliate
//         );
//     }

//     #[test]
//     fn admin_creates_affiliate_with_higher_than_default_bps() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let config = ManagerConfig {
//             admin: Addr::unchecked("admin"),
//             fee_collector: Addr::unchecked("fee_collector"),
//             affiliate_creation_fee: Coin::new(10u128, "rune"),
//             default_affiliate_bps: 2,
//             code_ids: vec![(StrategyType::Twap, 3)],
//         };

//         CONFIG.save(deps.as_mut().storage, &config).unwrap();

//         let affiliate = Affiliate {
//             code: "affiliate_code".to_string(),
//             address: Addr::unchecked("affiliate_address"),
//             bps: 5,
//         };

//         execute(
//             deps.as_mut(),
//             env,
//             message_info(&config.admin, &[Coin::new(10u128, "rune")]),
//             ManagerExecuteMsg::AddAffiliate {
//                 code: affiliate.code.clone(),
//                 address: affiliate.address.clone(),
//                 bps: affiliate.bps,
//             },
//         )
//         .unwrap();

//         assert_eq!(
//             AFFILIATES
//                 .load(deps.as_ref().storage, affiliate.code.clone())
//                 .unwrap(),
//             affiliate
//         );
//     }
// }

// #[cfg(test)]
// mod fetch_strategies_tests {
//     use super::*;
//     use cosmwasm_std::{
//         from_json,
//         testing::{mock_dependencies, mock_env},
//         Addr,
//     };

//     use crate::{contract::query, state::strategy_store};

//     #[test]
//     fn returns_empty_list_when_no_strategies_exist() {
//         let deps = mock_dependencies();
//         let env = mock_env();

//         let strategies = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Strategies {
//                 owner: None,
//                 status: None,
//                 start_after: None,
//                 limit: None,
//             },
//         )
//         .unwrap();

//         assert_eq!(from_json::<Vec<Strategy>>(strategies).unwrap(), vec![]);
//     }

//     #[test]
//     fn returns_strategies_in_reverse_creation_order() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let strategy1 = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner1"),
//             contract_address: Addr::unchecked("strategy1"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         let strategy2 = Strategy {
//             id: 2,
//             owner: Addr::unchecked("owner2"),
//             contract_address: Addr::unchecked("strategy2"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Archived,
//             affiliates: Vec::new(),
//         };

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy1.contract_address.clone(),
//                 &strategy1,
//             )
//             .unwrap();

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy2.contract_address.clone(),
//                 &strategy2,
//             )
//             .unwrap();

//         let strategies = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Strategies {
//                 owner: None,
//                 status: None,
//                 start_after: None,
//                 limit: None,
//             },
//         )
//         .unwrap();

//         assert_eq!(
//             from_json::<Vec<Strategy>>(strategies).unwrap(),
//             vec![strategy2, strategy1]
//         );
//     }

//     #[test]
//     fn returns_strategies_by_owner() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let strategy1 = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner1"),
//             contract_address: Addr::unchecked("strategy1"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         let strategy2 = Strategy {
//             id: 2,
//             owner: Addr::unchecked("owner2"),
//             contract_address: Addr::unchecked("strategy2"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Archived,
//             affiliates: Vec::new(),
//         };

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy1.contract_address.clone(),
//                 &strategy1,
//             )
//             .unwrap();

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy2.contract_address.clone(),
//                 &strategy2,
//             )
//             .unwrap();

//         let strategies = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Strategies {
//                 owner: Some(strategy1.owner.clone()),
//                 status: None,
//                 start_after: None,
//                 limit: None,
//             },
//         )
//         .unwrap();

//         assert_eq!(
//             from_json::<Vec<Strategy>>(strategies).unwrap(),
//             vec![strategy1]
//         );
//     }

//     #[test]
//     fn returns_strategies_by_status() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let strategy1 = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner1"),
//             contract_address: Addr::unchecked("strategy1"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         let strategy2 = Strategy {
//             id: 2,
//             owner: strategy1.owner.clone(),
//             contract_address: Addr::unchecked("strategy2"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Archived,
//             affiliates: Vec::new(),
//         };

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy1.contract_address.clone(),
//                 &strategy1,
//             )
//             .unwrap();

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy2.contract_address.clone(),
//                 &strategy2,
//             )
//             .unwrap();

//         let strategies = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Strategies {
//                 owner: None,
//                 status: Some(StrategyStatus::Active),
//                 start_after: None,
//                 limit: None,
//             },
//         )
//         .unwrap();

//         assert_eq!(
//             from_json::<Vec<Strategy>>(strategies).unwrap(),
//             vec![strategy1]
//         );
//     }

//     #[test]
//     fn returns_strategies_by_owner_and_status() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let strategy1 = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner1"),
//             contract_address: Addr::unchecked("strategy1"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         let strategy2 = Strategy {
//             id: 2,
//             owner: strategy1.owner.clone(),
//             contract_address: Addr::unchecked("strategy2"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Archived,
//             affiliates: Vec::new(),
//         };

//         let strategy3 = Strategy {
//             id: 3,
//             owner: Addr::unchecked("owner2"),
//             contract_address: Addr::unchecked("strategy3"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy1.contract_address.clone(),
//                 &strategy1,
//             )
//             .unwrap();

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy2.contract_address.clone(),
//                 &strategy2,
//             )
//             .unwrap();

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy3.contract_address.clone(),
//                 &strategy3,
//             )
//             .unwrap();

//         let strategies = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Strategies {
//                 owner: Some(strategy1.owner.clone()),
//                 status: Some(StrategyStatus::Active),
//                 start_after: None,
//                 limit: None,
//             },
//         )
//         .unwrap();

//         assert_eq!(
//             from_json::<Vec<Strategy>>(strategies).unwrap(),
//             vec![strategy1]
//         );
//     }

//     #[test]
//     fn returns_strategies_up_to_limit() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let strategy1 = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner1"),
//             contract_address: Addr::unchecked("strategy1"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         let strategy2 = Strategy {
//             id: 2,
//             owner: strategy1.owner.clone(),
//             contract_address: Addr::unchecked("strategy2"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Archived,
//             affiliates: Vec::new(),
//         };

//         let strategy3 = Strategy {
//             id: 3,
//             owner: Addr::unchecked("owner2"),
//             contract_address: Addr::unchecked("strategy3"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy1.contract_address.clone(),
//                 &strategy1,
//             )
//             .unwrap();

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy2.contract_address.clone(),
//                 &strategy2,
//             )
//             .unwrap();

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy3.contract_address.clone(),
//                 &strategy3,
//             )
//             .unwrap();

//         let strategies = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Strategies {
//                 owner: None,
//                 status: None,
//                 start_after: None,
//                 limit: Some(2),
//             },
//         )
//         .unwrap();

//         assert_eq!(
//             from_json::<Vec<Strategy>>(strategies).unwrap(),
//             vec![strategy3, strategy2]
//         );
//     }

//     #[test]
//     fn returns_strategies_from_start_after() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let strategy1 = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner1"),
//             contract_address: Addr::unchecked("strategy1"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds() - 5,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         let strategy2 = Strategy {
//             id: 2,
//             owner: Addr::unchecked("owner3"),
//             contract_address: Addr::unchecked("strategy2"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds() - 4,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         let strategy3 = Strategy {
//             id: 3,
//             owner: Addr::unchecked("owner2"),
//             contract_address: Addr::unchecked("strategy3"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds() - 3,
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy1.contract_address.clone(),
//                 &strategy1,
//             )
//             .unwrap();

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy2.contract_address.clone(),
//                 &strategy2,
//             )
//             .unwrap();

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy3.contract_address.clone(),
//                 &strategy3,
//             )
//             .unwrap();

//         let strategies = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Strategies {
//                 owner: None,
//                 status: None,
//                 start_after: Some(strategy3.updated_at),
//                 limit: None,
//             },
//         )
//         .unwrap();

//         assert_eq!(
//             from_json::<Vec<Strategy>>(strategies).unwrap(),
//             vec![strategy2, strategy1]
//         );
//     }
// }

// #[cfg(test)]
// mod fetch_strategy_tests {
//     use super::*;
//     use cosmwasm_std::{
//         from_json,
//         testing::{mock_dependencies, mock_env},
//         Addr,
//     };

//     use crate::{contract::query, state::strategy_store};

//     #[test]
//     fn returns_none_when_strategy_does_not_exist() {
//         let deps = mock_dependencies();
//         let env = mock_env();

//         let contract_address = Addr::unchecked("non_existent_strategy");

//         let response = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Strategy {
//                 address: contract_address.clone(),
//             },
//         )
//         .unwrap_err();

//         assert_eq!(
//             response,
//             StdError::generic_err(format!("Strategy not found: {}", contract_address))
//         );
//     }

//     #[test]
//     fn returns_strategy_when_it_exists() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let strategy = Strategy {
//             id: 1,
//             owner: Addr::unchecked("owner"),
//             contract_address: Addr::unchecked("existing_strategy"),
//             created_at: env.block.time.seconds(),
//             updated_at: env.block.time.seconds(),
//             label: "Test Strategy".to_string(),
//             status: StrategyStatus::Active,
//             affiliates: Vec::new(),
//         };

//         strategy_store()
//             .save(
//                 &mut deps.storage,
//                 strategy.contract_address.clone(),
//                 &strategy,
//             )
//             .unwrap();

//         let response = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Strategy {
//                 address: strategy.contract_address.clone(),
//             },
//         )
//         .unwrap();

//         assert_eq!(from_json::<Strategy>(response).unwrap(), strategy);
//     }
// }

// #[cfg(test)]
// mod fetch_affiliate_tests {
//     use super::*;
//     use cosmwasm_std::{
//         from_json,
//         testing::{mock_dependencies, mock_env},
//         Addr,
//     };

//     #[test]
//     fn returns_none_when_affiliate_does_not_exist() {
//         let deps = mock_dependencies();
//         let env = mock_env();

//         let response = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Affiliate {
//                 code: "non_existent_affiliate".to_string(),
//             },
//         )
//         .unwrap_err();

//         assert_eq!(
//             response,
//             StdError::generic_err("Affiliate not found with code: non_existent_affiliate")
//         );
//     }

//     #[test]
//     fn returns_affiliate_when_it_exists() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let affiliate = Affiliate {
//             code: "affiliate_code".to_string(),
//             address: Addr::unchecked("affiliate_address"),
//             bps: 2,
//         };

//         AFFILIATES
//             .save(deps.as_mut().storage, affiliate.code.clone(), &affiliate)
//             .unwrap();

//         let response = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Affiliate {
//                 code: affiliate.code.clone(),
//             },
//         )
//         .unwrap();

//         assert_eq!(from_json::<Affiliate>(response).unwrap(), affiliate);
//     }
// }

// #[cfg(test)]
// mod fetch_affiliates_tests {
//     use super::*;

//     use cosmwasm_std::{
//         from_json,
//         testing::{mock_dependencies, mock_env},
//         Addr,
//     };

//     #[test]
//     fn returns_empty_list_when_no_affiliates_exist() {
//         let deps = mock_dependencies();
//         let env = mock_env();

//         let response = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Affiliates {
//                 start_after: None,
//                 limit: None,
//             },
//         )
//         .unwrap();

//         assert_eq!(
//             from_json::<Vec<Addr>>(response).unwrap(),
//             Vec::<Addr>::new()
//         );
//     }

//     #[test]
//     fn returns_affiliates_up_to_limit() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let affiliate1 = Affiliate {
//             code: "affiliate1".to_string(),
//             address: Addr::unchecked("affiliate1"),
//             bps: 2,
//         };
//         let affiliate2 = Affiliate {
//             code: "affiliate2".to_string(),
//             address: Addr::unchecked("affiliate2"),
//             bps: 3,
//         };
//         let affiliate3 = Affiliate {
//             code: "affiliate3".to_string(),
//             address: Addr::unchecked("affiliate3"),
//             bps: 4,
//         };

//         AFFILIATES
//             .save(deps.as_mut().storage, "affiliate1".to_string(), &affiliate1)
//             .unwrap();

//         AFFILIATES
//             .save(deps.as_mut().storage, "affiliate2".to_string(), &affiliate2)
//             .unwrap();

//         AFFILIATES
//             .save(deps.as_mut().storage, "affiliate3".to_string(), &affiliate3)
//             .unwrap();

//         let response = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Affiliates {
//                 start_after: None,
//                 limit: Some(2),
//             },
//         )
//         .unwrap();

//         assert_eq!(
//             from_json::<Vec<Affiliate>>(response).unwrap(),
//             vec![affiliate1, affiliate2]
//         );
//     }

//     #[test]
//     fn returns_affiliates_from_start_after() {
//         let mut deps = mock_dependencies();
//         let env = mock_env();

//         let affiliate1 = Affiliate {
//             code: "affiliate1".to_string(),
//             address: Addr::unchecked("affiliate1"),
//             bps: 2,
//         };
//         let affiliate2 = Affiliate {
//             code: "affiliate2".to_string(),
//             address: Addr::unchecked("affiliate2"),
//             bps: 3,
//         };
//         let affiliate3 = Affiliate {
//             code: "affiliate3".to_string(),
//             address: Addr::unchecked("affiliate3"),
//             bps: 4,
//         };

//         AFFILIATES
//             .save(deps.as_mut().storage, "affiliate1".to_string(), &affiliate1)
//             .unwrap();

//         AFFILIATES
//             .save(deps.as_mut().storage, "affiliate2".to_string(), &affiliate2)
//             .unwrap();

//         AFFILIATES
//             .save(deps.as_mut().storage, "affiliate3".to_string(), &affiliate3)
//             .unwrap();

//         let response = query(
//             deps.as_ref(),
//             env.clone(),
//             ManagerQueryMsg::Affiliates {
//                 start_after: Some(affiliate1.address.clone()),
//                 limit: None,
//             },
//         )
//         .unwrap();

//         assert_eq!(
//             from_json::<Vec<Affiliate>>(response).unwrap(),
//             vec![affiliate2, affiliate3]
//         );
//     }
// }
