use calc_node_interface::{NodeExecuteMsg, NodeQueryMsg};
use calc_rs::{
    conditions::condition::Condition,
    constants::MAX_STRATEGY_SIZE,
    manager::{Affiliate, ManagerQueryMsg, NodeStatus, RegisteredNode},
    operation::Operation,
    strategy::{Node, StrategyOperation},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, CosmosMsg, Deps, DepsMut, Env, Order, StdError, StdResult, Storage,
    WasmMsg,
};
use cw_storage_plus::{Item, Map};

pub const MANAGER: Item<Addr> = Item::new("manager");
pub const OWNER: Item<Addr> = Item::new("owner");
pub const AFFILIATES: Item<Vec<Affiliate>> = Item::new("affiliates");
pub const DEPOSITS: Item<Vec<Coin>> = Item::new("deposits");
pub const WITHDRAWALS: Item<Vec<Coin>> = Item::new("withdrawals");
pub const REVISION: Item<u64> = Item::new("revision");

#[cw_serde]
pub struct PendingExternal {
    pub operation: StrategyOperation,
    pub node_index: u16,
}

pub const PENDING_EXTERNAL: Item<PendingExternal> = Item::new("pending_external");

pub struct NodeStore {
    store: Map<u16, Node>,
}

impl NodeStore {
    pub fn init(
        &self,
        deps: DepsMut,
        env: &Env,
        mut nodes: Vec<Node>,
    ) -> StdResult<Vec<CosmosMsg>> {
        let affiliates = AFFILIATES.load(deps.storage)?;
        let manager = MANAGER.load(deps.storage)?;
        let revision = REVISION.may_load(deps.storage)?.unwrap_or(1);
        let mut strategy_size = 0;
        let mut register_messages = vec![];

        let node_count = nodes.len();
        let final_index = node_count.saturating_sub(1) as u16;
        let mut in_degrees = vec![0usize; node_count];
        let mut adj_list = vec![Vec::new(); node_count];

        self.store.clear(deps.storage);

        for (i, mut node) in nodes.drain(..).enumerate() {
            let current_index = i;

            if node.index() != current_index as u16 {
                return Err(StdError::generic_err(format!(
                    "Node index mismatch: expected {}, got {}",
                    current_index,
                    node.index()
                )));
            }

            match &node {
                Node::Action { next, .. } => {
                    if let Some(next) = next {
                        if *next > final_index {
                            return Err(StdError::generic_err(format!(
                                "Next node index {next} exceeds total node count {node_count}"
                            )));
                        }
                        let next_index = *next as usize;
                        adj_list[current_index].push(next_index);
                        in_degrees[next_index] += 1;
                    }
                }
                Node::Condition {
                    condition,
                    on_success,
                    on_failure,
                    ..
                } => {
                    if on_failure.is_none() && on_success.is_none() {
                        match condition {
                            Condition::Schedule(_) => {}
                            _ => {
                                return Err(StdError::generic_err(
                                    "Condition nodes must have at least one branch defined",
                                ));
                            }
                        }
                    }

                    for (branch, name) in [(on_success, "success"), (on_failure, "fail")] {
                        if let Some(branch) = branch {
                            if *branch > final_index {
                                return Err(StdError::generic_err(format!(
                                    "On {name} node index {branch} exceeds total node count {node_count}"
                                )));
                            }
                            let branch_index = *branch as usize;
                            adj_list[current_index].push(branch_index);
                            in_degrees[branch_index] += 1;
                        }
                    }
                }
            }

            if let Some(external) = node.external().cloned() {
                let registered = deps.querier.query_wasm_smart::<RegisteredNode>(
                    &manager,
                    &ManagerQueryMsg::Node {
                        address: external.contract_address.clone(),
                    },
                )?;
                if registered.status == NodeStatus::Disabled {
                    return Err(StdError::generic_err(format!(
                        "Disabled node cannot be registered: {}",
                        external.contract_address
                    )));
                }
                strategy_size += registered.size_weight as usize;
                register_messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: external.contract_address.to_string(),
                    msg: to_json_binary(&NodeExecuteMsg::Register {
                        strategy_revision: revision,
                        node_index: node.index(),
                        config: external.config,
                    })?,
                    funds: vec![],
                }));
                node.external_mut().expect("external node").config = Default::default();
                self.save(deps.storage, &node)?;
            } else {
                let initialised_node = node.init(deps.as_ref(), env, &affiliates)?;
                strategy_size += initialised_node.size();
                self.save(deps.storage, &initialised_node)?;
            }
        }

        if strategy_size > MAX_STRATEGY_SIZE {
            return Err(StdError::generic_err(format!(
                "Strategy size exceeds maximum limit of {MAX_STRATEGY_SIZE}"
            )));
        }

        let mut queue = Vec::new();
        for (i, &degree) in in_degrees.iter().enumerate() {
            if degree == 0 {
                queue.push(i);
            }
        }

        let mut processed_count = 0;
        while let Some(current) = queue.pop() {
            processed_count += 1;
            for &neighbor in &adj_list[current] {
                in_degrees[neighbor] -= 1;
                if in_degrees[neighbor] == 0 {
                    queue.push(neighbor);
                }
            }
        }

        if processed_count != node_count {
            return Err(StdError::generic_err(
                "Strategy contains a cycle that could cause infinite recursion",
            ));
        }

        Ok(register_messages)
    }

    pub fn save(&self, storage: &mut dyn Storage, node: &Node) -> StdResult<()> {
        self.store.save(storage, node.index(), node)
    }

    pub fn load(&self, storage: &dyn Storage, index: u16) -> StdResult<Node> {
        self.store.load(storage, index)
    }

    pub fn all(&self, storage: &dyn Storage) -> StdResult<Vec<Node>> {
        self.store
            .range(storage, None, None, Order::Ascending)
            .map(|result| result.map(|(_, node)| node))
            .collect()
    }

    pub fn get_next(
        &self,
        deps: Deps,
        env: &Env,
        operation: &StrategyOperation,
        current: &Node,
    ) -> StdResult<Node> {
        if operation != &StrategyOperation::Execute {
            return self.load(deps.storage, current.index() + 1);
        }

        let next = match current {
            Node::Action { next, .. } => *next,
            Node::Condition {
                condition,
                on_success,
                on_failure,
                ..
            } => {
                let satisfied = match condition {
                    Condition::External(external) => deps
                        .querier
                        .query_wasm_smart::<bool>(
                            &external.contract_address,
                            &NodeQueryMsg::IsSatisfied {
                                strategy: env.contract.address.clone(),
                                strategy_revision: REVISION.load(deps.storage)?,
                                node_index: current.index(),
                            },
                        )
                        .unwrap_or(false),
                    _ => condition.is_satisfied(deps, env).unwrap_or(false),
                };
                if satisfied {
                    *on_success
                } else {
                    *on_failure
                }
            }
        };

        if let Some(next) = next {
            return self.load(deps.storage, next);
        }

        Err(StdError::generic_err(
            "No next node found for the current node",
        ))
    }
}

pub const NODES: NodeStore = NodeStore {
    store: Map::new("nodes"),
};

pub const PATH: Item<Vec<String>> = Item::new("path");
