use calc_rs::{
    constants::MAX_STRATEGY_SIZE,
    manager::Affiliate,
    operation::Operation,
    strategy::{Node, StrategyOperation},
};
use cosmwasm_std::{Addr, Deps, DepsMut, Env, Order, StdError, StdResult, Storage};
use cw_storage_plus::{Item, Map};

pub const MANAGER: Item<Addr> = Item::new("manager");
pub const OWNER: Item<Addr> = Item::new("owner");
pub const AFFILIATES: Item<Vec<Affiliate>> = Item::new("affiliates");

pub struct NodeStore {
    store: Map<u16, Node>,
}

impl NodeStore {
    pub fn init(&self, deps: DepsMut, env: &Env, nodes: Vec<Node>) -> StdResult<()> {
        let affiliates = AFFILIATES.load(deps.storage)?;
        let mut strategy_size = 0;

        let node_count = nodes.len();
        let final_index = node_count.saturating_sub(1) as u16;
        let mut in_degrees = vec![0usize; node_count];
        let mut adj_list = vec![Vec::new(); node_count];

        // Use Kahn's algorithm to ensure no cycles in the strategy
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
                        if next > final_index {
                            return Err(StdError::generic_err(format!(
                                "Next node index {next} exceeds total node count {node_count}"
                            )));
                        }

                        let next_index = next as usize;
                        adj_list[current_index].push(next_index);
                        in_degrees[next_index] += 1;
                    }
                }
                Node::Condition {
                    on_success,
                    on_failure,
                    ..
                } => {
                    if on_failure.is_none() && on_success.is_none() {
                        return Err(StdError::generic_err(
                            "Condition node must have at least one branch defined",
                        ));
                    }

                    if let Some(on_success) = on_success {
                        if on_success > final_index {
                            return Err(StdError::generic_err(format!(
                                "On success node index {on_success} exceeds total node count {node_count}"
                            )));
                        }

                        let on_success_index = on_success as usize;
                        adj_list[current_index].push(on_success_index);
                        in_degrees[on_success_index] += 1;
                    }

                    if let Some(on_failure) = on_failure {
                        if on_failure > final_index {
                            return Err(StdError::generic_err(format!(
                                "On fail node index {on_failure} exceeds total node count {node_count}"
                            )));
                        }

                        let on_failure_index = on_failure as usize;
                        adj_list[current_index].push(on_failure_index);
                        in_degrees[on_failure_index] += 1;
                    }
                }
            }

            let initialised_node = node.init(deps.as_ref(), env, &affiliates)?;
            self.save(deps.storage, &initialised_node)?;

            strategy_size += initialised_node.size();
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
    ) -> StdResult<Node> {
        if operation != &StrategyOperation::Execute {
            return self.load(deps.storage, current.index() + 1);
        }

        if let Some(next) = current.next_index(deps, env) {
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
