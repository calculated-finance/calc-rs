# Strategy Contract

## High-Level Behaviour

The `strategy` contract is the on-chain runtime environment for executing declarative trading strategies. It manages the complete lifecycle of a strategy, from initialization through execution, updates, and withdrawal.

Each strategy contract is an isolated execution environment that owns and manages its own funds, executes its defined actions autonomously, and maintains statistics about its operations. The contract separates strategy definition (the _what_) from execution logic (the _how_), enabling strategies to be expressed declaratively.

## Key Features

- **Atomic Execution:** Two-phase commit pattern ensures strategy state consistency
- **State Machine Management:** Transitions through Committed → Active → Executable states
- **Reentrancy Protection:** Guards against recursive calls and state corruption
- **Stateful Operations:** Handles stateful operations like limit orders and scheduled actions
- **Fund Isolation:** Each strategy manages its own isolated funds
- **Statistics Tracking:** Comprehensive tracking of debits, credits, and distributions
- **Dynamic Updates:** Hot-swapping of strategy logic with proper state unwinding
- **Flexible Withdrawals:** Selective fund withdrawal with escrowed balance protection

## Strategy Domain Model

The strategy execution model is built around several key components:

### Action Types

- **`Swap`:** Execute token swaps across multiple DEX protocols
- **`Distribute`:** Send funds to multiple recipients with share based allocations
- **`LimitOrder`:** Place and manage static or dynamic limit orders
- **`Schedule`:** Execute actions on recurring schedules (time/block/cron/price-based)
- **`Conditional`:** Execute actions only when specific conditions are met
- **`Many`:** Execute multiple actions in sequence

### Condition Types

- **Time-based:** `TimestampElapsed`, `BlocksCompleted`
- **Market-based:** `CanSwap`, `LimitOrderFilled`, `OraclePrice`
- **Balance-based:** `BalanceAvailable`, `StrategyBalanceAvailable`
- **Strategy-based:** `StrategyStatus`
- **Logical:** `Not`, `Composite` (AND/OR combinations)

## Contract State Machine

The strategy contract implements a state machine to handle stateful operations:

### State Transitions

1. **Committed:** Strategy is at rest, ready for execution
2. **Active:** Strategy is preparing for execution, generating messages
3. **Executable:** Strategy has generated messages and is executing them
4. **Committable:** Execution complete, ready to commit state changes

### Execution Flow

```
Committed → prepare_to_execute() → Active → execute() → Executable
    ↑                                                        ↓
    ← commit() ← Committable ← sub-messages complete ←————————
```

This pattern ensures that:

- All strategy actions execute atomically
- State is never left in an inconsistent state
- Complex stateful operations (like limit orders) are properly managed
- Recursive execution is prevented

## Instantiate Message

```rust
pub struct Strategy<Indexed> {
    pub owner: Addr,
    pub action: Action,
    pub state: Indexed { contract_address: Addr },
}
```

Initializes a new strategy contract instance.

- **Authorization:** Can be called by any address (typically the manager contract)
- **Parameters:** A fully configured strategy with indexed state
- **Logic:**
  1. Validates contract address matches the strategy's expected address
  2. Analyzes the action tree to determine required and escrowed denominations
  3. Initializes the strategy through validation and setup
  4. Saves the strategy configuration in committed state
  5. Immediately triggers first execution cycle

## Execute Messages

### `Execute`

Triggers the main strategy execution cycle.

```rust
StrategyExecuteMsg::Execute
```

- **Authorization:** Manager contract or self-call only
- **Logic:**
  1. Loads committed strategy from storage
  2. Transitions to active state via `activate()`
  3. Calls `prepare_to_execute()` to analyze action tree and generate messages
  4. Saves active strategy state for state machine tracking
  5. Dispatches generated messages with commit callback
  6. Automatically schedules `Commit` message after sub-message completion

### `Update(Strategy<Indexed>)`

Updates the strategy with a new action definition.

```rust
StrategyExecuteMsg::Update(new_strategy)
```

- **Authorization:** Manager contract only
- **Logic:**
  1. **Phase 1 - Unwind:** Calls `prepare_to_cancel()` to generate cleanup messages for stateful actions
  2. **Phase 2 - Conditional:** If cleanup needed, executes cleanup and recursively calls update
  3. **Phase 3 - Replace:** If no cleanup needed, initializes new strategy and replaces current
  4. **Phase 4 - Execute:** Immediately executes the new strategy

This ensures safe hot-swapping of strategy logic without losing funds or corrupting state.

### `Withdraw(HashSet<String>)`

Withdraws funds from the strategy contract.

```rust
StrategyExecuteMsg::Withdraw(desired_denoms)
```

- **Authorization:** Strategy owner or self-call
- **Parameters:** Set of denominations to withdraw (empty = all non-escrowed)
- **Logic:**
  1. Validates requested denoms are not escrowed (protected from withdrawal)
  2. Calls `prepare_to_withdraw()` to release funds from stateful actions
  3. If release needed, executes release and recursively calls withdraw
  4. If no release needed, queries contract balances and sends to owner
  5. Protects funds escrowed for CALC and affiliate fee disbursements

### `UpdateStatus(StrategyStatus)`

Changes the strategy's operational status.

```rust
pub enum StrategyStatus {
    Active,    // Strategy executes normally
    Paused,    // Strategy execution suspended
    Archived,  // Same as paused (used for filtering)
}
```

- **Authorization:** Manager contract only
- **Logic:**
  - **Active:** Prepares and executes strategy normally
  - **Paused/Archived:** Calls `prepare_to_cancel()` to unwind active state
  - Status is primarily used for manager-level filtering and control

### `Commit`

Finalizes strategy execution and commits state changes.

```rust
StrategyExecuteMsg::Commit
```

- **Authorization:** Self-call only (automatic after sub-message completion)
- **Logic:**
  1. Loads active strategy from temporary storage
  2. Calls `prepare_to_commit()` to finalize state updates
  3. Updates internal action state (e.g., advancing schedule timing)
  4. Transitions back to committed state and saves to permanent storage
  5. Clears temporary active strategy state

### `Clear`

Utility function to reset reentrancy protection.

```rust
StrategyExecuteMsg::Clear
```

- **Authorization:** Self-call or strategy owner
- **Logic:** Removes the execution state guard to allow new message processing

## Query Messages

### `Config`

Returns the complete strategy configuration.

```rust
pub struct StrategyConfig {
    pub manager: Addr,           // Manager contract address
    pub strategy: Strategy<Committed>, // Current strategy definition
    pub denoms: HashSet<String>, // All denominations used by strategy
    pub escrowed: HashSet<String>, // Denominations locked for operations
}
```

### `Statistics`

Returns execution statistics and performance metrics.

```rust
pub struct Statistics {
    pub debited: Vec<Coin>,                    // Total outgoing transactions
    pub credited: Vec<(Recipient, Vec<Coin>)>, // Total distributions by recipient
}
```

Tracks:

- **Debited:** All outgoing transactions (swaps, limit orders, etc.)
- **Credited:** All distributions to external addresses
- **Performance:** Success/failure rates for different action types

### `Balances(HashSet<String>)`

Returns strategy balances across all holdings.

- **Parameters:** Set of denominations to query (empty = all tracked denoms)
- **Returns:** `Vec<Coin>` with complete balance information
- **Sources:**
  - Direct contract balances
  - Balances held in external protocols (i.e. pending limit orders)

## State Management

### Storage Layout

- **`CONFIG`:** Primary strategy configuration and committed state
- **`ACTIVE_STRATEGY`:** Temporary state during execution cycles
- **`DENOMS`:** Set of all denominations used by the strategy
- **`ESCROWED`:** Set of denominations protected from withdrawal
- **`STATS`:** Cumulative execution statistics
- **`STATE`:** Reentrancy protection guard

### Reentrancy Protection

The contract implements sophisticated reentrancy protection:

1. **State Guard:** Prevents multiple concurrent executions
2. **Message Validation:** Rejects recursive calls to same message type
3. **Clear Mechanism:** Automatic cleanup of guard state
4. **Self-Call Detection:** Allows legitimate self-calls while blocking recursion

### Error Handling and Recovery

- **Reply Mechanism:** All sub-messages use reply handlers for error tracking
- **State Recovery:** Failed executions don't corrupt strategy state
- **Partial Execution:** Individual action failures don't prevent other actions
- **Debugging Support:** Comprehensive error attributes for troubleshooting

## Integration Patterns

### Manager Integration

The strategy contract integrates closely with the manager contract:

- Manager instantiates strategies with proper configuration
- Manager controls strategy lifecycle (active/paused/archived)
- Manager can update strategy definitions
- Manager masters strategy status & label

### Scheduler Integration

Strategies work with the scheduler for automation:

- Scheduled actions create triggers in scheduler contract
- Scheduler executes strategies when conditions are met
- Rebate mechanisms incentivize keeper participation

## Security Considerations

- **Fund Isolation:** Each strategy contract holds its own funds separately
- **Authorization:** Strict access control for sensitive operations
- **State Consistency:** Two-phase commit prevents state corruption
- **Reentrancy Protection:** Multiple layers of protection against recursive attacks
- **Validation:** Comprehensive input validation and size limits
- **Recovery:** Graceful handling of external protocol failures
