use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hasher},
    vec,
};

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    instantiate2_address, to_json_binary, Addr, Binary, Coin, Coins, CosmosMsg, Deps, DepsMut, Env,
    Event, MessageInfo, StdError, StdResult, Storage, SubMsg, WasmMsg,
};

use crate::{
    actions::{action::Action, operation::Operation},
    condition::Condition,
    constants::PROCESS_PAYLOAD_REPLY_ID,
    core::Contract,
    manager::Affiliate,
    statistics::Statistics,
};

#[cw_serde]
pub struct StrategyConfig {
    pub manager: Addr,
    pub owner: Addr,
    pub nodes: Vec<Node>,
    pub denoms: HashSet<String>,
    pub escrowed: HashSet<String>,
}

#[cw_serde]
pub enum StrategyOperation {
    Execute,
    Withdraw(HashSet<String>),
    Cancel,
}

#[cw_serde]
pub enum StrategyExecuteMsg {
    Init(Vec<Node>),
    Execute,
    Withdraw(Vec<Coin>),
    Update(Vec<Node>),
    Cancel,
    Process {
        operation: StrategyOperation,
        previous: Option<u16>,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum StrategyQueryMsg {
    #[returns(StrategyConfig)]
    Config {},
    #[returns(Statistics)]
    Statistics {},
    #[returns(Vec<Coin>)]
    Balances(HashSet<String>),
}

#[cw_serde]
pub struct Strategy<S> {
    pub owner: Addr,
    pub affiliates: Vec<Affiliate>,
    pub nodes: Vec<Node>,
    pub state: S,
}

impl<S> Strategy<S> {
    pub fn size(&self) -> usize {
        self.nodes.iter().map(|n| n.size()).sum::<usize>() + 1
    }

    pub fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        let mut denoms = HashSet::new();

        for node in self.nodes.iter() {
            let node_denoms = node.denoms(deps, env)?;
            denoms.extend(node_denoms);
        }

        Ok(denoms)
    }

    pub fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        let mut escrowed = HashSet::new();

        for node in self.nodes.iter() {
            let node_escrowed = node.escrowed(deps, env)?;
            escrowed.extend(node_escrowed);
        }

        Ok(escrowed)
    }

    pub fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        let mut balances = Coins::default();

        for node in self.nodes.iter() {
            let node_balances = node.balances(deps, env, denoms)?;

            for balance in node_balances {
                balances.add(balance)?;
            }
        }

        Ok(balances)
    }
}

#[cw_serde]
#[derive(Default)]
pub struct StrategyMsgPayload {
    pub statistics: Statistics,
    pub events: Vec<Event>,
}

impl StrategyMsgPayload {
    pub fn decorated_events(&self, decorator: &str) -> Vec<Event> {
        self.events
            .clone()
            .into_iter()
            .map(|mut event| {
                event.ty = format!("calc/{}/{}", event.ty, decorator);
                event
            })
            .collect()
    }
}

#[cw_serde]
pub struct StrategyMsg {
    msg: CosmosMsg,
    payload: StrategyMsgPayload,
}

impl StrategyMsg {
    pub fn with_payload(msg: CosmosMsg, payload: StrategyMsgPayload) -> Self {
        Self { msg, payload }
    }

    pub fn without_payload(msg: CosmosMsg) -> Self {
        Self {
            msg,
            payload: StrategyMsgPayload::default(),
        }
    }
}

impl From<StrategyMsg> for SubMsg {
    fn from(msg: StrategyMsg) -> Self {
        SubMsg::reply_always(msg.msg, PROCESS_PAYLOAD_REPLY_ID)
            .with_payload(to_json_binary(&msg.payload).unwrap())
    }
}

#[cw_serde]
pub struct Indexable;

pub struct Instantiable {
    pub contract_address: Addr,
    label: String,
    salt: Binary,
    code_id: u64,
}

pub struct Updatable {
    pub contract_address: Addr,
}

#[cw_serde]
pub struct Indexed {
    pub contract_address: Addr,
}

impl Strategy<Indexable> {
    pub fn add_index<F>(
        self,
        deps: &mut DepsMut,
        env: &Env,
        code_id: u64,
        label: String,
        save: F,
    ) -> StdResult<Strategy<Instantiable>>
    where
        F: FnOnce(&mut dyn Storage, &Strategy<Instantiable>) -> StdResult<()>,
    {
        let salt_data = to_json_binary(&(self.owner.to_string(), self.clone(), env.block.height))?;
        let mut hash = DefaultHasher::new();
        hash.write(salt_data.as_slice());
        let salt = hash.finish().to_le_bytes();

        let contract_address = deps.api.addr_humanize(
            &instantiate2_address(
                deps.querier
                    .query_wasm_code_info(code_id)?
                    .checksum
                    .as_slice(),
                &deps.api.addr_canonicalize(env.contract.address.as_str())?,
                &salt,
            )
            .map_err(|e| {
                StdError::generic_err(format!("Failed to instantiate contract address: {e}"))
            })?,
        )?;

        let instantiable_strategy = Strategy {
            owner: self.owner.clone(),
            affiliates: self.affiliates.clone(),
            nodes: self.nodes.clone(),
            state: Instantiable {
                contract_address,
                label,
                salt: Binary::from(salt),
                code_id,
            },
        };

        save(deps.storage, &instantiable_strategy)?;

        Ok(instantiable_strategy)
    }

    pub fn update_index<F>(
        self,
        deps: &mut DepsMut,
        contract_address: Addr,
        save: F,
    ) -> StdResult<Strategy<Updatable>>
    where
        F: FnOnce(&mut dyn Storage) -> StdResult<()>,
    {
        let indexed_strategy = Strategy {
            owner: self.owner,
            affiliates: self.affiliates,
            nodes: self.nodes,
            state: Updatable { contract_address },
        };

        save(deps.storage)?;

        Ok(indexed_strategy)
    }
}

impl Strategy<Instantiable> {
    pub fn instantiate_msg(
        self,
        info: MessageInfo,
        affiliates: Vec<Affiliate>,
    ) -> StdResult<CosmosMsg> {
        Ok(WasmMsg::Instantiate2 {
            admin: Some(self.owner.to_string()),
            code_id: self.state.code_id,
            label: self.state.label,
            salt: self.state.salt,
            msg: to_json_binary(&Strategy {
                owner: self.owner,
                affiliates,
                nodes: self.nodes,
                state: Indexed {
                    contract_address: self.state.contract_address.clone(),
                },
            })?,
            funds: info.funds,
        }
        .into())
    }
}

impl Strategy<Updatable> {
    pub fn update_msg(self, info: MessageInfo) -> StdResult<CosmosMsg> {
        Ok(Contract(self.state.contract_address.clone()).call(
            to_json_binary(&StrategyExecuteMsg::Update(self.nodes))?,
            info.funds,
        ))
    }
}

#[cw_serde]
pub enum Node {
    Action {
        action: Action,
        index: u16,
        next: Option<u16>,
    },
    Condition {
        condition: Condition,
        index: u16,
        on_success: u16,
        on_fail: Option<u16>,
    },
}

impl Node {
    pub fn size(&self) -> usize {
        match self {
            Node::Action { action, .. } => action.size(),
            Node::Condition { condition, .. } => condition.size(),
        }
    }

    pub fn index(&self) -> u16 {
        match self {
            Node::Action { index, .. } => index.clone(),
            Node::Condition { index, .. } => index.clone(),
        }
    }

    pub fn next_index(&self, deps: Deps, env: &Env) -> Option<u16> {
        match self {
            Node::Action { next, .. } => next.clone(),
            Node::Condition {
                condition,
                on_fail,
                on_success,
                ..
            } => {
                if condition.is_satisfied(deps, env).unwrap_or(false) {
                    Some(on_success.clone())
                } else {
                    on_fail.clone()
                }
            }
        }
    }
}

impl Operation<Node> for Node {
    fn init(self, deps: Deps, env: &Env, affiliates: &[Affiliate]) -> StdResult<Node> {
        match self {
            Node::Action {
                action,
                index,
                next,
            } => Ok(Node::Action {
                action: action.init(deps, env, affiliates)?,
                index,
                next,
            }),
            Node::Condition {
                condition,
                index,
                on_success,
                on_fail,
            } => Ok(Node::Condition {
                condition: condition.init(deps, env, affiliates)?,
                index,
                on_success,
                on_fail,
            }),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Node) {
        match self {
            Node::Action {
                action,
                index,
                next,
            } => {
                let (messages, events, action) = action.execute(deps, env);
                (
                    messages,
                    events,
                    Node::Action {
                        action,
                        index,
                        next,
                    },
                )
            }
            Node::Condition {
                condition,
                index,
                on_success,
                on_fail,
            } => {
                let (messages, events, condition) = condition.execute(deps, env);
                (
                    messages,
                    events,
                    Node::Condition {
                        condition,
                        index,
                        on_success,
                        on_fail,
                    },
                )
            }
        }
    }

    fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Node::Action { action, .. } => action.denoms(deps, env),
            Node::Condition { condition, .. } => condition.denoms(deps, env),
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Node::Action { action, .. } => action.escrowed(deps, env),
            Node::Condition { condition, .. } => condition.escrowed(deps, env),
        }
    }

    fn commit(self, deps: Deps, env: &Env) -> StdResult<Node> {
        Ok(match self {
            Node::Action {
                action,
                index,
                next,
            } => Node::Action {
                action: action.commit(deps, env)?,
                index,
                next,
            },
            Node::Condition {
                condition,
                index,
                on_success,
                on_fail,
            } => Node::Condition {
                condition: condition.commit(deps, env)?,
                index,
                on_success,
                on_fail,
            },
        })
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        match self {
            Node::Action { action, .. } => action.balances(deps, env, denoms),
            Node::Condition { condition, .. } => condition.balances(deps, env, denoms),
        }
    }

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Node)> {
        match self {
            Node::Action {
                action,
                index,
                next,
            } => {
                let (messages, events, action) = action.withdraw(deps, env, desired)?;
                Ok((
                    messages,
                    events,
                    Node::Action {
                        action,
                        index,
                        next,
                    },
                ))
            }
            Node::Condition {
                condition,
                index,
                on_success,
                on_fail,
            } => {
                let (messages, events, condition) = condition.withdraw(deps, env, desired)?;
                Ok((
                    messages,
                    events,
                    Node::Condition {
                        condition,
                        index,
                        on_success,
                        on_fail,
                    },
                ))
            }
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Node)> {
        match self {
            Node::Action {
                action,
                index,
                next,
            } => {
                let (messages, events, action) = action.cancel(deps, env)?;
                Ok((
                    messages,
                    events,
                    Node::Action {
                        action,
                        index,
                        next,
                    },
                ))
            }
            Node::Condition {
                condition,
                index,
                on_success,
                on_fail,
            } => {
                let (messages, events, condition) = condition.cancel(deps, env)?;
                Ok((
                    messages,
                    events,
                    Node::Condition {
                        condition,
                        index,
                        on_success,
                        on_fail,
                    },
                ))
            }
        }
    }
}
