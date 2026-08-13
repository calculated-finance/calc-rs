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
    pub withdrawals: Vec<Coin>,
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
    Execute {},
    Withdraw(Vec<Coin>),
    Update(Vec<Node>),
    Cancel {},
    Process {
        operation: StrategyOperation,
        previous: Option<u16>,
    },
    ProcessWithoutCommit {
        operation: StrategyOperation,
        previous: u16,
    },
    ProcessAt {
        operation: StrategyOperation,
        next: Option<u16>,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum StrategyQueryMsg {
    #[returns(StrategyConfig)]
    Config {},
    #[returns(Vec<Addr>)]
    ExternalNodeReferences {},
    #[returns(Vec<Coin>)]
    Balances {},
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

    pub fn external(&self) -> Option<&calc_node_interface::ExternalNode> {
        match self {
            Node::Action {
                action: Action::External(external),
                ..
            }
            | Node::Condition {
                condition: Condition::External(external),
                ..
            } => Some(external),
            _ => None,
        }
    }

    pub fn external_mut(&mut self) -> Option<&mut calc_node_interface::ExternalNode> {
        match self {
            Node::Action {
                action: Action::External(external),
                ..
            }
            | Node::Condition {
                condition: Condition::External(external),
                ..
            } => Some(external),
            _ => None,
        }
    }

    pub fn next_index(&self, deps: Deps, env: &Env) -> StdResult<Option<u16>> {
        self.next_index_with_external(deps, env, |_| {
            Err(cosmwasm_std::StdError::generic_err(
                "External condition traversal must be handled by the strategy adapter",
            ))
        })
    }

    pub fn next_index_with_external<F>(
        &self,
        deps: Deps,
        env: &Env,
        external_is_satisfied: F,
    ) -> StdResult<Option<u16>>
    where
        F: FnOnce(&calc_node_interface::ExternalNode) -> StdResult<bool>,
    {
        match self {
            Node::Action { next, .. } => Ok(*next),
            Node::Condition {
                condition,
                on_success,
                on_failure,
                ..
            } => {
                let satisfied = match condition {
                    Condition::External(external) => external_is_satisfied(external)?,
                    _ => condition.is_satisfied(deps, env).unwrap_or(false),
                };
                Ok(if satisfied { *on_success } else { *on_failure })
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

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Node)> {
        match self {
            Node::Action {
                action,
                index,
                next,
            } => {
                let (messages, action) = action.execute(deps, env)?;
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
                let (messages, condition) = condition.execute(deps, env)?;
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

#[cfg(test)]
mod tests {
    use calc_node_interface::ExternalNode;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Addr, Binary,
    };

    use super::*;

    #[test]
    fn external_condition_traversal_requires_strategy_adapter() {
        let deps = mock_dependencies();
        let node = Node::Condition {
            condition: Condition::External(ExternalNode {
                contract_address: Addr::unchecked("external-node"),
                config: Binary::default(),
            }),
            index: 0,
            on_success: Some(1),
            on_failure: Some(2),
        };

        assert!(node.next_index(deps.as_ref(), &mock_env()).is_err());
        assert_eq!(
            node.next_index_with_external(deps.as_ref(), &mock_env(), |_| Ok(true))
                .unwrap(),
            Some(1)
        );
        assert_eq!(
            node.next_index_with_external(deps.as_ref(), &mock_env(), |_| Ok(false))
                .unwrap(),
            Some(2)
        );
    }
}
