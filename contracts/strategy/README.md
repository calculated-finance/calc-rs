# Strategy Contract

## High-Level Behaviour

The `strategy` contract is the on-chain runtime environment for executing declarative trading strategies using a directed acyclic graph (DAG) execution model. It manages the complete lifecycle of a strategy, from initialization through execution, updates, and withdrawal.

Each strategy contract is an isolated execution environment that owns and manages its own funds, executes its defined operations autonomously, and maintains statistics about its operations. The contract implements a graph-based execution engine where strategies are represented as a collection of interconnected nodes that execute sequentially with conditional branching.

## Key Features

- **DAG-Based Execution:** Strategies are represented as directed acyclic graphs with action and condition nodes
- **Sequential Processing:** Ensures fresh balance queries between operations for accurate execution
- **Conditional Branching:** Condition nodes enable complex control flow based on runtime evaluation
- **Operation Polymorphism:** Unified operation interface supporting swaps, limit orders, distributions, and more
- **Cycle Prevention:** Built-in validation ensures strategies cannot create infinite execution loops
- **Fund Isolation:** Each strategy manages its own isolated funds with denomination tracking
- **Dynamic Updates:** Hot-swapping of strategy logic with proper cleanup of existing state

## Strategy Domain Model

The strategy execution model is built around a graph of interconnected nodes:

### Node Types

#### Action Nodes

Action nodes represent concrete operations that modify state or generate blockchain messages:

- **`Swap`:** Execute token swaps across multiple DEX protocols
- **`Distribute`:** Send funds to multiple recipients with share-based allocations
- **`LimitOrder`:** Place and manage static or dynamic limit orders

#### Condition Nodes

Condition nodes provide branching logic and control flow:

- **Time-based:** `TimestampElapsed`, `BlocksCompleted`
- **Market-based:** `CanSwap`, `LimitOrderFilled`, `OraclePrice`
- **Balance-based:** `BalanceAvailable`, `StrategyBalanceAvailable`
- **Schedule-based:** `Schedule` for recurring execution patterns

### Graph Structure

```
Node 0: Condition(PriceCheck)
    ├─ on_success: Node 2 (Swap)
    └─ on_failure: Node 1 (Distribute)

Node 1: Action(Distribute)
    └─ next: None

Node 2: Action(Swap)
    └─ next: Node 3 (LimitOrder)

Node 3: Action(LimitOrder)
    └─ next: None
```

Each node contains:

- **Index:** Unique position in the strategy graph
- **Operation:** The actual business logic to execute
- **Edges:** References to subsequent nodes (next, on_success, on_failure)

## Execution Model

### Graph Traversal

The strategy contract executes nodes sequentially following the graph edges:

1. **Linear Execution:** Action nodes execute and proceed to their `next` node
2. **Conditional Branching:** Condition nodes evaluate and follow `on_success` or `on_failure` edges
3. **Message Generation:** When a node generates blockchain messages, execution pauses for external calls
4. **Continuation:** After external messages complete, execution resumes from the next node
5. **Termination:** Execution completes when reaching a node with no outgoing edges

### State Management

Each node operation follows the Operation trait pattern:

```rust
pub trait Operation<T>: Send + Sync + Clone {
    fn init(self, deps: Deps, env: &Env, affiliates: &[Affiliate]) -> StdResult<T>;
    fn execute(self, deps: Deps, env: &Env) -> (Vec<CosmosMsg>, T);
    fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>>;
    fn commit(self, deps: Deps, env: &Env) -> StdResult<T>;
    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins>;
    fn withdraw(self, deps: Deps, env: &Env, desired: &HashSet<String>) -> StdResult<(Vec<CosmosMsg>, T)>;
    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, T)>;
}
```

### Cycle Prevention

The contract validates strategy graphs during initialization using topological sorting:

1. **Graph Analysis:** Builds adjacency list and calculates in-degrees for all nodes
2. **Kahn's Algorithm:** Performs topological sort to detect cycles
3. **Validation:** Rejects strategies that contain cycles to prevent infinite execution
4. **Error Reporting:** Provides clear error messages for invalid graph structures

## Instantiate Message

```rust
pub struct StrategyInstantiateMsg {
    pub contract_address: Addr,
    pub owner: Addr,
    pub nodes: Vec<Node>,
    pub affiliates: Vec<Affiliate>,
}
```

Initializes a new strategy contract instance.

- **Authorization:** Can be called by any address (typically the manager contract)
- **Parameters:** Complete strategy graph with owner and affiliate configuration
- **Logic:**
  1. Validates contract address matches deployment address
  2. Stores manager, owner, and affiliate information
  3. Initializes strategy through graph validation and node setup
  4. Automatically triggers first execution cycle

## Execute Messages

### `Init(Vec<Node>)`

Initializes the strategy graph with validation and setup.

```rust
StrategyExecuteMsg::Init(nodes)
```

- **Authorization:** Self-call only (triggered automatically after instantiation)
- **Logic:**
  1. Analyzes all nodes to determine required denominations
  2. Validates graph structure and prevents cycles
  3. Initializes each node through the Operation trait
  4. Saves graph to storage with proper indexing
  5. Triggers initial execution cycle

### `Execute`

Triggers the main strategy execution cycle.

```rust
StrategyExecuteMsg::Execute
```

- **Authorization:** Manager contract only
- **Logic:**
  1. Creates internal message to start processing from node 0
  2. Begins graph traversal with Execute operation mode

### `Update(Vec<Node>)`

Updates the strategy with a new graph definition.

```rust
StrategyExecuteMsg::Update(new_nodes)
```

- **Authorization:** Manager contract only
- **Logic:**
  1. **Phase 1 - Cancel:** Executes existing strategy in Cancel mode to clean up state
  2. **Phase 2 - Replace:** Initializes new strategy graph
  3. **Phase 3 - Execute:** Immediately begins execution of new strategy

This ensures safe hot-swapping of strategy logic without losing funds or corrupting state.

### `Withdraw(Vec<Coin>)`

Withdraws specified amounts from the strategy contract.

```rust
StrategyExecuteMsg::Withdraw(amounts)
```

- **Authorization:** Strategy owner only
- **Parameters:** Specific coin amounts to withdraw
- **Logic:**
  1. Validates requested amounts against available balances
  2. Processes affiliate fee distributions
  3. Sends remaining funds to strategy owner

### `Cancel`

Cancels all active strategy operations and cleans up state.

```rust
StrategyExecuteMsg::Cancel
```

- **Authorization:** Manager contract only
- **Logic:**
  1. Executes strategy graph in Cancel mode
  2. Generates cleanup messages for stateful operations
  3. Unwinds any pending or active positions

### `Process { operation, previous }`

Internal message for graph traversal and node execution.

```rust
StrategyExecuteMsg::Process {
    operation: StrategyOperation,
    previous: Option<u16>,
}
```

- **Authorization:** Self-call only
- **Parameters:**
  - `operation`: Execute, Withdraw, or Cancel mode
  - `previous`: Index of previously processed node (for continuation)
- **Logic:**
  1. **State Transition:** Commits previous node state if applicable
  2. **Node Loading:** Determines next node to process based on graph edges
  3. **Execution Loop:** Processes nodes sequentially until external messages are needed
  4. **Message Generation:** When external calls are required, executes them and their replies before continuing
  5. **Completion:** Continues until reaching graph termination

The Process message implements the core graph traversal logic, handling both sequential execution and conditional branching.

## Query Messages

### `Config`

Returns the complete strategy configuration.

```rust
pub struct StrategyConfig {
    pub manager: Addr,              // Manager contract address
    pub owner: Addr,                // Strategy owner address
    pub nodes: Vec<Node>,           // Complete strategy graph
    pub denoms: HashSet<String>,    // All denominations used by strategy
}
```

### `Balances(HashSet<String>)`

Returns strategy balances across all holdings.

- **Parameters:** Set of denominations to query (empty = all tracked denoms)
- **Returns:** `Vec<Coin>` with complete balance information
- **Sources:**
  - Direct contract balances
  - Balances held in external protocols (i.e. limit orders)

## State Management

### Storage Layout

- **`MANAGER`:** Address of the managing contract
- **`OWNER`:** Strategy owner address
- **`AFFILIATES`:** Fee distribution configuration
- **`DENOMS`:** Set of all denominations used by the strategy
- **`NODES`:** Map of node index to Node data

### Node Storage

Nodes are stored in a Map keyed by their index:

```rust
pub struct NodeStore {
    store: Map<u16, Node>,
}
```

Key operations:

- **`init`:** Validates and stores complete graph
- **`load`:** Retrieves individual nodes by index
- **`save`:** Updates node state after execution
- **`get_next`:** Determines next node based on current node and operation mode

### Graph Validation

The NodeStore implements comprehensive validation:

1. **Index Consistency:** Ensures node indices match their array positions
2. **Reference Validation:** Verifies all edge references point to valid nodes
3. **Cycle Detection:** Uses topological sorting to prevent infinite loops
4. **Size Limits:** Enforces maximum strategy size constraints

## Integration Patterns

### Manager Integration

The strategy contract integrates with the manager contract:

- Manager instantiates strategies with proper configuration
- Manager controls strategy lifecycle (execute/update/cancel)
- Manager can update strategy definitions
- Strategy reports back execution status and statistics

### Operation Integration

All operations implement the unified Operation trait:

- Consistent interface for initialization, execution, and cleanup
- Polymorphic handling of different operation types
- Standardized balance and denomination reporting
- Unified state management across operation types

## Security Considerations

- **Fund Isolation:** Each strategy contract holds its own funds separately
- **Authorization:** Strict access control with separate owner/manager roles
- **Cycle Prevention:** Graph validation prevents infinite execution loops and hanging pointers
- **State Consistency:** Operation trait ensures consistent state transitions
- **Size Limits:** Prevents gas exhaustion through strategy size constraints
