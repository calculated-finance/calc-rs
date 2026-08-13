use calc_node_interface::{NodeDetailsResponse, NodeExecuteMsg, NodeQueryMsg};
use calc_rs::manager::{ManagerQueryMsg, Strategy};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Coin, CosmosMsg, Deps, DepsMut, Env, MessageInfo,
    Response, StdError, StdResult,
};
use cw_storage_plus::{Item, Map};

#[cw_serde]
pub struct InstantiateMsg {}

#[cw_serde]
pub struct MigrateMsg {}

#[cw_serde]
pub struct MockNodeConfig {
    pub satisfied: bool,
    pub messages: Vec<CosmosMsg>,
    pub balances: Vec<Coin>,
    pub fail_execute: bool,
    pub fail_cancel: bool,
    pub fail_commit: bool,
    pub malformed_response: bool,
}

impl Default for MockNodeConfig {
    fn default() -> Self {
        Self {
            satisfied: true,
            messages: vec![],
            balances: vec![],
            fail_execute: false,
            fail_cancel: false,
            fail_commit: false,
            malformed_response: false,
        }
    }
}

#[cw_serde]
pub struct MockNodeState {
    pub config: Binary,
    pub execute_count: u64,
    pub cancel_count: u64,
    pub commit_count: u64,
}

const MANAGER: Item<Addr> = Item::new("manager");
const NODES: Map<(&Addr, u64, u16), MockNodeState> = Map::new("nodes");

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    _msg: InstantiateMsg,
) -> StdResult<Response> {
    MANAGER.save(deps.storage, &info.sender)?;
    Ok(Response::new())
}

#[entry_point]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> StdResult<Response> {
    Ok(Response::new())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: NodeExecuteMsg,
) -> StdResult<Response> {
    match msg {
        NodeExecuteMsg::Register {
            strategy_revision,
            node_index,
            config,
        } => {
            let _: Strategy = deps.querier.query_wasm_smart(
                MANAGER.load(deps.storage)?,
                &ManagerQueryMsg::Strategy {
                    address: info.sender.clone(),
                },
            )?;
            let _: MockNodeConfig = cosmwasm_std::from_json(config.clone())?;
            let key = (&info.sender, strategy_revision, node_index);
            if NODES.has(deps.storage, key) {
                return Err(StdError::generic_err("Node is already registered"));
            }
            NODES.save(
                deps.storage,
                key,
                &MockNodeState {
                    config,
                    execute_count: 0,
                    cancel_count: 0,
                    commit_count: 0,
                },
            )?;
            Ok(Response::new())
        }
        NodeExecuteMsg::Execute {
            strategy_revision,
            node_index,
        } => {
            let mut state =
                NODES.load(deps.storage, (&info.sender, strategy_revision, node_index))?;
            let config: MockNodeConfig = cosmwasm_std::from_json(state.config.clone())?;
            if config.fail_execute {
                return Err(StdError::generic_err("Mock execute failure"));
            }
            state.execute_count += 1;
            NODES.save(
                deps.storage,
                (&info.sender, strategy_revision, node_index),
                &state,
            )?;
            if config.malformed_response {
                Ok(Response::new().set_data(Binary::new(b"malformed".to_vec())))
            } else {
                Ok(Response::new().set_data(to_json_binary(&config.messages)?))
            }
        }
        NodeExecuteMsg::Commit {
            strategy_revision,
            node_index,
        } => {
            let mut state =
                NODES.load(deps.storage, (&info.sender, strategy_revision, node_index))?;
            let config: MockNodeConfig = cosmwasm_std::from_json(state.config.clone())?;
            if config.fail_commit {
                return Err(StdError::generic_err("Mock commit failure"));
            }
            state.commit_count += 1;
            NODES.save(
                deps.storage,
                (&info.sender, strategy_revision, node_index),
                &state,
            )?;
            Ok(Response::new())
        }
        NodeExecuteMsg::Cancel {
            strategy_revision,
            node_index,
        } => {
            let mut state =
                NODES.load(deps.storage, (&info.sender, strategy_revision, node_index))?;
            let config: MockNodeConfig = cosmwasm_std::from_json(state.config.clone())?;
            if config.fail_cancel {
                return Err(StdError::generic_err("Mock cancel failure"));
            }
            state.cancel_count += 1;
            NODES.save(
                deps.storage,
                (&info.sender, strategy_revision, node_index),
                &state,
            )?;
            Ok(Response::new().set_data(to_json_binary(&config.messages)?))
        }
    }
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: NodeQueryMsg) -> StdResult<Binary> {
    let (strategy, revision, index) = match &msg {
        NodeQueryMsg::Details {
            strategy,
            strategy_revision,
            node_index,
        }
        | NodeQueryMsg::IsSatisfied {
            strategy,
            strategy_revision,
            node_index,
        }
        | NodeQueryMsg::Balances {
            strategy,
            strategy_revision,
            node_index,
        } => (strategy, *strategy_revision, *node_index),
    };
    let state = NODES.load(deps.storage, (strategy, revision, index))?;
    let config: MockNodeConfig = cosmwasm_std::from_json(state.config.clone())?;

    match msg {
        NodeQueryMsg::Details { .. } => to_json_binary(&NodeDetailsResponse {
            config: state.config,
        }),
        NodeQueryMsg::IsSatisfied { .. } => to_json_binary(&config.satisfied),
        NodeQueryMsg::Balances { .. } => to_json_binary(&config.balances),
    }
}
