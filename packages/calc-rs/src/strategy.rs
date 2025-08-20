use std::vec;

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Coin, Coins, CosmosMsg, Deps, Env, StdResult};

use crate::{
    actions::action::Action,
    conditions::condition::Condition,
    manager::Affiliate,
    operation::{Operation, StatefulOperation},
};

#[cw_serde]
pub struct StrategyConfig {
    pub manager: Addr,
    pub owner: Addr,
    pub nodes: Vec<Node>,
}

#[cw_serde]
pub enum StrategyOperation {
    Execute,
    Cancel,
}

impl StrategyOperation {
    pub fn as_str(&self) -> &str {
        match self {
            StrategyOperation::Execute => "execute",
            StrategyOperation::Cancel => "cancel",
        }
    }
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
    Config,
    #[returns(Vec<Coin>)]
    Balances,
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
        on_success: Option<u16>,
        on_failure: Option<u16>,
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
            Node::Action { index, .. } => *index,
            Node::Condition { index, .. } => *index,
        }
    }

    pub fn next_index(&self, deps: Deps, env: &Env) -> Option<u16> {
        match self {
            Node::Action { next, .. } => *next,
            Node::Condition {
                condition,
                on_failure,
                on_success,
                ..
            } => {
                if condition.is_satisfied(deps, env).unwrap_or(false) {
                    *on_success
                } else {
                    *on_failure
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
                on_failure,
            } => Ok(Node::Condition {
                condition: condition.init(deps, env, affiliates)?,
                index,
                on_success,
                on_failure,
            }),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<CosmosMsg>, Node) {
        match self {
            Node::Action {
                action,
                index,
                next,
            } => {
                let (messages, action) = action.execute(deps, env);
                (
                    messages,
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
                on_failure,
            } => {
                let (messages, condition) = condition.execute(deps, env);
                (
                    messages,
                    Node::Condition {
                        condition,
                        index,
                        on_success,
                        on_failure,
                    },
                )
            }
        }
    }
}

impl StatefulOperation<Node> for Node {
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
                on_failure,
            } => Node::Condition {
                condition: condition.commit(deps, env)?,
                index,
                on_success,
                on_failure,
            },
        })
    }

    fn balances(&self, deps: Deps, env: &Env) -> StdResult<Coins> {
        match self {
            Node::Action { action, .. } => action.balances(deps, env),
            Node::Condition { .. } => Ok(Coins::default()),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Node)> {
        match self {
            Node::Action {
                action,
                index,
                next,
            } => {
                let (messages, action) = action.cancel(deps, env)?;
                Ok((
                    messages,
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
                on_failure,
            } => {
                let (messages, condition) = condition.cancel(deps, env)?;
                Ok((
                    messages,
                    Node::Condition {
                        condition,
                        index,
                        on_success,
                        on_failure,
                    },
                ))
            }
        }
    }
}
