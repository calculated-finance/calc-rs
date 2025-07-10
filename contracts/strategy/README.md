# Strategy Contract

## High-Level Behaviour

The `strategy` contract is the on-chain runtime for a declarative trading strategy. It executes a logical plan, structured as a tree of `Action`s, separating the user's desired actions from the contract's execution logic.

The contract manages the lifecycle of a `Strategy` entity, which encapsulates an `Action` tree and its state. It transitions the `Strategy` through a state machine to ensure safe, atomic, and predictable execution.

### The Strategy Domain Model

The logical definition of a strategy is composed of several key domain models from the `calc-rs` package:

- **`Action`**: A recursive enum that defines the building blocks of a strategy. An `Action` can be:
  - `Swap`: Executes a token swap.
  - `Distribute`: Distributes funds to multiple destinations.
  - `LimitOrder`: A stateful action that places and manages a limit order on a DEX.
  - `Schedule`: Executes another `Action` based on a recurring `Cadence`.
  - `Conditional`: Executes another `Action` only when a specific `Condition` is met.
  - `Many`: A container for executing multiple `Action`s in sequence.

- **`Condition`**: An enum for creating reactive strategies. A `Conditional` action triggers only if its `Condition` is met. Conditions can be based on time (`TimestampElapsed`), block height (`BlocksCompleted`), market state (`CanSwap`, `LimitOrderFilled`), or balances (`BalanceAvailable`).

- **`Cadence`**: An enum used by the `Schedule` action to define recurring execution, based on a block interval, time duration, or a cron expression.

### Contract State Machine

The contract transitions a `Strategy` through `Committed`, `Active`, and `Executable` states to handle stateful operations atomically (e.g., waiting for a limit order to fill). An `Execute` message initiates the state transitions, generating `CosmosMsg`s for the action. After the sub-messages complete, a `Commit` message returns the strategy to the `Committed` state, updating its internal logic (e.g., setting the next execution time for a scheduled action).

## Instantiate Message

Initializes a new strategy contract.

- **Authorization:** Can be called by any address. The `info.sender` is designated as the `manager` for the new strategy.
- **Logic:**
  1.  The incoming `StrategyInstantiateMsg` (a type alias for `Strategy<Indexed>`) is received.
  2.  The `escrowed()` method is called on the strategy's `Action` tree to determine which denoms are required for its operations. These are stored in the `ESCROWED` state.
  3.  The `init()` method is called, which validates the strategy's size and recursively validates the `Action` tree.
  4.  The fully initialized strategy is saved to the `CONFIG` state with a `Committed` status.
  5.  An `Execute` sub-message is dispatched to immediately start the strategy's execution cycle.

## Execute Messages

The `strategy` contract exposes the following execute messages:

### `Execute`

The main entry point for running the strategy's logic.

- **Authorization:** `manager` contract or the `strategy` contract itself.
- **Logic:**
  1.  The `Committed` strategy is loaded from `CONFIG` and transitioned to `Active`.
  2.  `prepare_to_execute` is called on the `Active` strategy. This recursively traverses the `Action` tree, evaluates any `Condition`s, and generates the `CosmosMsg`s required to perform the strategy's logic for the current block.
  3.  The `Active` strategy state is saved to `ACTIVE_STRATEGY` in storage. This acts as a temporary, "in-flight" version of the strategy.
  4.  The generated messages are dispatched as sub-messages. A `Commit` message is scheduled to run after all of the strategy generated sub-messages complete.

### `Update`

Updates the strategy's configuration with a new `Action` tree.

- **Authorization:** `manager` contract only.
- **Logic:**
  1.  The contract calls `prepare_to_cancel` on the current strategy. This generates messages to unwind any stateful operations (e.g., cancelling active limit orders). The `Active` strategy state is saved to `ACTIVE_STRATEGY` in storage.
  2.  If unwind messages are generated, they are executed, and the contract recursively calls `Update` to try again once the state is clear.
  3.  If no unwind is needed, the new strategy is initialized via `init`, which validates the `Action` tree and performs any necessary setup. The new configuration is saved.
  4.  The contract immediately calls the `Execute` message to begin execution of the newly updated strategy.

### `Withdraw`

Allows the strategy `owner` to withdraw funds from the contract.

- **Authorization:** `owner` or the `strategy` contract itself (for 2 stage withdrawals).
- **Logic:**
  1.  Checks that the requested denoms for withdrawal are not part of the strategy's `escrowed` funds. The `Active` strategy state is saved to `ACTIVE_STRATEGY` in storage.
  2.  Calls `prepare_to_withdraw` on the strategy to generate messages that release any funds held in stateful actions.
  3.  If unwind messages are needed, they are executed, and the contract recursively calls `Withdraw` to try again.
  4.  If no unwind is needed, the contract queries its own balances for the desired denoms and sends them to the `owner` via a `BankMsg::Send`.

### `UpdateStatus`

Changes the strategy's operational status.

- **Authorization:** `manager` contract only.
- **Logic:**
  - `Active`: Calls `prepare_to_execute` and runs the strategy's execution logic.
  - `Paused` | `Archived`: Calls `prepare_to_cancel` to unwind any active state. The statuses themselves are primarily for off-chain filtering and do not introduce unique on-chain logic beyond pausing execution.
  - The `Active` strategy state is saved to `ACTIVE_STRATEGY` in storage.

### `Commit`

Commits the result of an execution, finalizing the state transition. This enables stateful strategy actions to verify the changes they made were successful.

- **Authorization:** `strategy` contract itself only.
- **Logic:**
  1.  Called as a reply from `Execute`, `Update`, `Withdraw`, and `UpdateStatus` message's sub-messages.
  2.  Loads the `ACTIVE_STRATEGY` from storage.
  3.  Calls `prepare_to_commit` on it. This updates the internal state of the `Action` tree (e.g., advancing a `Schedule` to its next occurrence).
  4.  The now-updated strategy is transitioned back to the `Committed` state and saved over the previous version in `CONFIG`.
  5.  The temporary `ACTIVE_STRATEGY` is cleared from storage.

### `Clear`

A utility function to clear the re-entrancy guard.

- **Authorization:** `strategy` contract itself or the `owner`.
- **Logic:** Removes the `STATE` item from storage. This is called as the final sub-message in any top-level execute flow to ensure the re-entrancy guard is reset, allowing the contract to process a new message.

## Query Messages

The `strategy` contract exposes the following query messages:

### `Config`

This message is used to query the strategy's configuration.

- **Returns:** The strategy's configuration, including the `manager`, `strategy`, and `escrowed` denoms.

### `Statistics`

This message is used to query the strategy's statistics.

- **Returns:** The strategy's statistics
  - `outgoing`: The total amount of debit transactions made by the strategy. Includes swaps and limit orders, but not distributions or withdrawals.
  - `distributions`: The total amount of funds distributed by the strategy to other addresses.

### `Balances`

This message is used to query the strategy's balances.

- **Returns:** The strategy's balances, including the balances of all denoms held in other protocols by the strategy.
