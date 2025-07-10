# Scheduler Contract

## High-Level Behaviour

The `scheduler` contract is a decentralized automation engine that enables execution of `strategy` contracts when their predefined conditions are met. It acts as a public utility, allowing anyone to register a `Trigger` and have it executed by a third-party keeper. This enables time-based and event-driven automation for any strategy managed by a `manager` contract.

The contract maintains a list of `Triggers`, each containing a `Condition` and a reference to the `strategy` contract to be executed. Keepers can query the scheduler for executable triggers, and are incentivized to execute them by receiving a rebate.

## Instantiate Message

Initializes a new scheduler contract.

- **Authorization:** Can be called by any address.
- **Logic:**
  1.  The incoming `SchedulerInstantiateMsg` is received.
  2.  The `manager` address is saved to the `MANAGER` state.
  3.  The `TRIGGER_COUNTER` is initialized to 0.

## Execute Messages

The `scheduler` contract exposes the following execute messages:

### `Create`

Creates a new `Trigger`.

- **Authorization:** Can be called by any address.
- **Logic:**
  1.  A new `Trigger` is created with the provided `Condition`.
  2.  The `info.sender` is set as the `owner` of the trigger.
  3.  Any funds sent with the message are stored as an `execution_rebate`.
  4.  The new trigger is saved to the `TRIGGERS` store.

### `Execute`

Executes a list of `Trigger`s.

- **Authorization:** Can be called by any address.
- **Logic:**
  1.  The contract iterates through the provided list of trigger IDs.
  2.  For each trigger, the `condition` is checked using `is_satisfied`.
  3.  If the condition is met, the trigger is deleted from the `TRIGGERS` store.
  4.  A `ManagerExecuteMsg::ExecuteStrategy` message is sent to the `manager` contract to execute the strategy.
  5.  If the trigger has an `execution_rebate`, it is sent to the `info.sender`.

## Query Messages

The `scheduler` contract exposes the following query messages:

### `Owned`

- **Returns:** A list of `Trigger`s owned by the specified address.

### `Filtered`

- **Returns:** A list of `Trigger`s that match the filter.

### `CanExecute`

- **Returns:** A boolean indicating whether the trigger's condition is met.
