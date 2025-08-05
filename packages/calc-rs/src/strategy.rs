use std::{collections::HashSet, vec};

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    to_json_binary, Addr, Coin, Coins, CosmosMsg, Deps, Env, Event, StdResult, SubMsg,
};

use crate::{
    actions::{action::Action, operation::Operation},
    condition::Condition,
    constants::PROCESS_PAYLOAD_REPLY_ID,
    manager::Affiliate,
    statistics::Statistics,
};

#[cw_serde]
pub struct StrategyConfig {
    pub manager: Addr,
    pub owner: Addr,
    pub nodes: Vec<Node>,
    pub denoms: HashSet<String>,
}

#[cw_serde]
pub enum StrategyOperation {
    Execute,
    Withdraw(HashSet<String>),
    Cancel,
}

#[cw_serde]
pub struct StrategyInstantiateMsg {
    pub contract_address: Addr,
    pub owner: Addr,
    pub affiliates: Vec<Affiliate>,
    pub nodes: Vec<Node>,
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
