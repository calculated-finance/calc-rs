# Scheduler Contract

## High-Level Behaviour

The `scheduler` contract is a decentralized automation engine that enables execution of arbitrary smart contract messages when predefined conditions are met. It acts as a public utility, allowing anyone to register a `Trigger` that links a `Condition` to a contract execution message. Keepers are incentivised to execute triggers by receiving execution rebates, creating a decentralized automation system.

The contract maintains an indexed collection of `Triggers`, each containing:

- A `Condition` that determines when the trigger should execute
- A message payload to execute on the target contract
- An optional execution rebate to incentivize keepers
- Optional executor restrictions for access control
- Optional jitter for execution timing randomization (only useful when combined with executor restrictions)

## Key Features

- **Condition-based Execution:** Supports multiple condition types including time-based, block-based, and fin-limit-order-based
- **Keeper Incentivization:** Execution rebates reward keepers for monitoring and executing triggers
- **Limit Order Integration:** Special handling for DEX limit orders with automatic order placement and withdrawal
- **Efficient Querying:** Multi-indexed storage enables efficient filtering by condition type, timestamp, block height, and DEX pairs
- **Duplicate Prevention:** Deterministic trigger IDs based on message content prevent duplicate triggers
- **Error Handling:** Robust error handling with reply mechanisms for failed executions

- **Authorization:** Can be called by any address

## Execute Messages

### `Create(CreateTriggerMsg)`

Creates or updates a trigger.

```rust
pub struct CreateTriggerMsg {
    pub condition: Condition,
    pub msg: Binary,
    pub contract_address: Addr,
    pub executors: Vec<Addr>,
    pub jitter: Option<Duration>,
}
```

- **Authorization:** Can be called by any address
- **Parameters:**
  - `condition`: The condition that must be met for execution
  - `msg`: The message to execute on the target contract
  - `contract_address`: The target contract address
  - `executors`: Optional list of addresses allowed to execute this trigger (empty = anyone can execute)
  - `jitter`: Optional timing randomization for execution
- **Funds:** Any funds sent are stored as execution rebate for the keeper
- **Logic:**
  1. Generates a unique trigger ID based on the message content hash
  2. If a trigger with the same ID exists, it's deleted and its rebate refunded
  3. For `LimitOrderFilled` conditions, automatically places the limit order on the DEX
  4. Saves the new trigger to indexed storage

### `Execute(Vec<Uint64>)`

Executes a list of triggers by their IDs.

- **Authorization:** Can be called by any address (subject to executor restrictions)
- **Parameters:**
  - List of trigger IDs to execute
- **Logic:**
  1. For each trigger ID:
     - Loads the trigger from storage
     - Checks if the condition is satisfied
     - If satisfied, deletes the trigger from storage
     - For limit orders, withdraws the order and sends filled amount as rebate
     - Executes the trigger's message on the target contract
     - Sends any execution rebate to the caller (keeper)
  2. Swallows any downstream contract execution failures and logs them

## Query Messages

### `Filtered`

```rust
Filtered {
    filter: ConditionFilter,
    limit: Option<usize>,
}
```

Returns triggers that match the specified filter criteria.

- **Filters:**
  - `Timestamp { start, end }`: Triggers with timestamp conditions in the given range
  - `BlockHeight { start, end }`: Triggers with block height conditions in the given range
  - `LimitOrder { pair_address, price_range, start_after }`: Limit order triggers for a specific DEX pair
- **Returns:** `Vec<Trigger>` (limited to 30 by default)

### `CanExecute(Uint64)`

Checks if a trigger can be executed (condition is satisfied).

- **Parameters:** Trigger ID
- **Returns:** `bool` indicating if the trigger's condition is met

## Condition Types

The scheduler supports various condition types:

### Time-based Conditions

- **`TimestampElapsed(Timestamp)`**: Executes after a specific timestamp
- **`BlocksCompleted(u64)`**: Executes after a specific block height

### Market Conditions

- **`CanSwap(Swap)`**: Executes when a swap is possible with minimum rate requirements

## Limit Order Integration

The scheduler provides special handling for DEX limit orders:

1. **Order Placement:** When creating a `LimitOrderFilled` trigger, the scheduler automatically places the limit order on the specified DEX pair
2. **Order Monitoring:** The trigger remains active until the order is filled
3. **Order Withdrawal:** Upon execution, the scheduler withdraws the filled order and sends proceeds as rebate
4. **Efficient Querying:** Limit order triggers are indexed by pair address and price for efficient keeper queries

## Storage and Indexing

The contract uses indexed storage for efficient querying:

- **Primary Storage:** Triggers stored by ID
- **Timestamp Index:** Enables querying by timestamp conditions
- **Block Height Index:** Enables querying by block height conditions
- **Limit Order Pair Index:** Enables querying by DEX pair
- **Limit Order Price Index:** Enables querying by DEX pair and price range

## Error Handling

- **Execution Errors:** Failed message executions are caught via reply mechanism and logged
- **Condition Evaluation Errors:** Conditions that cannot be evaluated are deleted from storage

## Executor Economics

Executors are incentivised via:

- **Execution Rebates:** Funds deposited when creating triggers
- **Limit Order Proceeds:** Filled amounts from limit order execution
- **Gas Efficiency:** Batch execution of multiple triggers reduces per-trigger gas costs
