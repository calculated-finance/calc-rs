# Strategy Contract

## High-Level Behaviour

The `strategy` contract is the on-chain runtime environment for a declarative, automated trading strategy. It interprets and executes a logical plan defined by a user, which is structured as a tree of `Action`s. This approach separates the *what* (the user's desired actions) from the *how* (the contract's execution logic).

The core of the system is the `Strategy` domain entity, which encapsulates an `Action` tree and its current state. The `strategy` contract's primary role is to manage the lifecycle of this `Strategy` entity, transitioning it through a well-defined state machine to ensure safe, atomic, and predictable execution of potentially complex and long-running operations.

### The Strategy Domain Model

The logical definition of a strategy is composed of several key domain models from the `calc-rs` package:

- **`Action`**: A recursive enum that defines the building blocks of a strategy. An `Action` can be:
    - `Swap`: Executes a token swap.
    - `Distribute`: Distributes funds to multiple destinations.
    - `LimitOrder`: A stateful action that places and manages a limit order on a DEX.
    - `Schedule`: Executes another `Action` based on a recurring `Cadence`.
    - `Conditional`: Executes another `Action` only when a specific `Condition` is met.
    - `Many`: A container for executing multiple `Action`s in sequence.
    
    This recursive structure allows for the creation of highly complex and customized strategies.

- **`Condition`**: An enum that enables reactive strategies. A `Conditional` action will only trigger if its `Condition` is satisfied. Conditions can be based on time (`TimestampElapsed`), block height (`BlocksCompleted`), market state (`CanSwap`, `LimitOrderFilled`), or balances (`BalanceAvailable`).

- **`Cadence`**: An enum used by the `Schedule` action to define recurring execution. Cadence can be based on a block interval, a time duration, or a cron expression.

### Contract State Machine

To safely execute the `Action` tree, the contract transitions the `Strategy` through several states (e.g., `Committed`, `Active`, `Executable`). This ensures that stateful operations (like waiting for a limit order to fill) are handled atomically. An `Execute` message triggers a transition from `Committed` to `Active` and then to `Executable`, where the contract generates the necessary `CosmosMsg`s to perform the action. Upon successful execution of the sub-messages, a `Commit` message transitions the strategy back to a stable `Committed` state with its internal logic updated (e.g., a scheduled action's next execution time is set).

## Execute Messages

The `strategy` contract exposes the following execute messages:

### `Update`

This message is used to update the strategy's configuration. It can only be called by the `manager` contract. The update process is designed to be safe and atomic, ensuring that the strategy is in a consistent state before and after the update.

- **Authorization:** Only the `manager` contract can call this message.
- **Process:**
    1. The contract first prepares to cancel the existing strategy, unwinding any stateful actions.
    2. If there are no stateful actions to unwind, the contract proceeds with the update.
    3. Any newly escrowed denoms are accumulated.
    4. The new strategy is initialized.
    5. The new strategy is executed immediately after initialization.
    6. If there are stateful actions to unwind, the contract unwinds them and then re-runs the update process.

### `Execute`

This message is used to execute the strategy's trading logic. It can be called by the `manager` contract or the `strategy` contract itself.

- **Authorization:** Only the `manager` contract or the `strategy` contract itself can call this message.
- **Process:**
    1. The contract prepares to execute the strategy.
    2. The strategy is executed, and the `ACTIVE_STRATEGY` is saved to the store.

### `Withdraw`

This message is used to withdraw funds from the strategy. It can be called by the strategy's `owner` or the `strategy` contract itself.

- **Authorization:** Only the strategy's `owner` or the `strategy` contract itself can call this message.
- **Process:**
    1. The contract checks that the desired denoms are not escrowed.
    2. The contract prepares to withdraw the funds.
    3. If there are no stateful actions to unwind, the contract withdraws the funds.
    4. If there are stateful actions to unwind, the contract unwinds them and then re-runs the withdrawal process.

### `UpdateStatus`

This message is used to update the strategy's status. It can only be called by the `manager` contract.

- **Authorization:** Only the `manager` contract can call this message.
- **Process:**
    1. The contract updates the strategy's status to `Active`, `Paused`, or `Archived`.
    2. If the status is `Active`, the contract prepares to execute the strategy.
    3. If the status is `Paused` or `Archived`, the contract prepares to cancel the strategy.

### `Commit`

This message is used to commit the active strategy to the store. It can only be called by the `strategy` contract itself.

- **Authorization:** Only the `strategy` contract itself can call this message.
- **Process:**
    1. The contract prepares to commit the active strategy.
    2. The active strategy is committed to the store, and the `ACTIVE_STRATEGY` is removed from the store.

### `Clear`

This message is used to clear the contract's state. It can be called by the `strategy` contract itself or the strategy's `owner`.

- **Authorization:** Only the `strategy` contract itself or the strategy's `owner` can call this message.
- **Process:**
    1. The contract removes the `STATE` from the store.

## Query Messages

The `strategy` contract exposes the following query messages:

### `Config`

This message is used to query the strategy's configuration.

- **Returns:** The strategy's configuration, including the `manager`, `strategy`, and `escrowed` denoms.

### `Statistics`

This message is used to query the strategy's statistics.

- **Returns:** The strategy's statistics, including the number of trades, volume, and profit and loss.

### `Balances`

This message is used to query the strategy's balances.

- **Returns:** The strategy's balances, including the balances of all denoms held by the strategy.
