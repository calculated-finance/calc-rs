use std::collections::HashSet;

use calc_rs::{
    actions::operation::Operation,
    constants::MAX_STRATEGY_SIZE,
    manager::Affiliate,
    statistics::Statistics,
    strategy::{Node, StrategyOperation},
};
use cosmwasm_std::{Addr, Deps, DepsMut, Env, Order, StdError, StdResult, Storage};
use cw_storage_plus::{Item, Map};

pub const MANAGER: Item<Addr> = Item::new("manager");
pub const OWNER: Item<Addr> = Item::new("owner");
pub const AFFILIATES: Item<Vec<Affiliate>> = Item::new("affiliates");
pub const DENOMS: Item<HashSet<String>> = Item::new("denoms");
pub const STATS: Item<Statistics> = Item::new("stats");

pub struct NodeStore {
    store: Map<u16, Node>,
}

impl NodeStore {
    pub fn init(&self, deps: DepsMut, env: &Env, nodes: Vec<Node>) -> StdResult<()> {
        let affiliates = AFFILIATES.load(deps.storage)?;
        let mut strategy_size = 0;

        let node_count = nodes.len();
        let mut in_degrees = vec![0usize; node_count];
        let mut adj_list = vec![Vec::new(); node_count];

        for (i, node) in nodes.into_iter().enumerate() {
            let current_index = i;

            if node.index() != current_index as u16 {
                return Err(StdError::generic_err(format!(
                    "Node index mismatch: expected {}, got {}",
                    current_index,
                    node.index()
                )));
            }

            match node {
                Node::Action { next, .. } => {
                    if let Some(next) = next {
                        let next_index = next.clone() as usize;
                        if next_index < node_count {
                            adj_list[current_index].push(next_index);
                            in_degrees[next_index] += 1;
                        }
                    }
                }
                Node::Condition {
                    on_success,
                    on_fail,
                    ..
                } => {
                    let on_success_index = on_success.clone() as usize;
                    if on_success_index < node_count {
                        adj_list[current_index].push(on_success_index);
                        in_degrees[on_success_index] += 1;
                    }

                    if let Some(on_fail) = on_fail {
                        let on_fail_index = on_fail.clone() as usize;
                        if on_fail_index < node_count {
                            adj_list[current_index].push(on_fail_index);
                            in_degrees[on_fail_index] += 1;
                        }
                    }
                }
            }

            strategy_size += node.size();

            let initialised_node = node.init(deps.as_ref(), env, &affiliates)?;
            self.save(deps.storage, &initialised_node)?;
        }

        if strategy_size > MAX_STRATEGY_SIZE {
            return Err(StdError::generic_err(format!(
                "Strategy size exceeds maximum limit of {}",
                MAX_STRATEGY_SIZE
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

        Ok(())
    }

    pub fn save(&self, storage: &mut dyn Storage, node: &Node) -> StdResult<()> {
        self.store.save(storage, node.index(), node)
    }

    pub fn load(&self, storage: &dyn Storage, index: u16) -> StdResult<Node> {
        self.store.load(storage, index)
    }

    pub fn all(&self, storage: &dyn Storage) -> StdResult<Vec<Node>> {
        Ok(self
            .store
            .prefix(())
            .range(storage, None, None, Order::Ascending)
            .flat_map(|r| r.map(|(_, action)| action))
            .collect::<Vec<_>>())
    }

    pub fn get_next(
        &self,
        deps: Deps,
        env: &Env,
        operation: &StrategyOperation,
        current: &Node,
    ) -> StdResult<Option<Node>> {
        if operation != &StrategyOperation::Execute {
            return Ok(self
                .load(deps.storage, current.index() + 1)
                .map_or(None, |node| Some(node)));
        }

        let next = current.next_index(deps, env);

        if let Some(index) = next {
            if let Some(node) = self.store.may_load(deps.storage, index)? {
                return Ok(Some(node));
            }
        }

        Ok(None)
    }
}

pub const NODES: NodeStore = NodeStore {
    store: Map::new("nodes"),
};
