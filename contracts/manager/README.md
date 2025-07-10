# Manager Contract

## High-Level Behaviour

The `manager` contract acts as a factory and registry for `strategy` contracts. Its primary responsibilities are to instantiate, update, and manage the lifecycle of strategies on behalf of users. It provides a centralized point of control and discovery for all strategies within the CALC protocol.

The contract maintains a registry of all strategies it has instantiated, tracking their status, owner, and other metadata. This allows for efficient querying and filtering of strategies, as well as enforcing authorization rules for management operations.

## Instantiate Message

Initializes a new manager contract.

- **Authorization:** Can be called by any address. The `info.sender` is not used for authorization.
- **Logic:**
  1.  The incoming `ManagerConfig` is saved to the `CONFIG` state.
  2.  The `STRATEGY_COUNTER` is initialized to 0.

## Execute Messages

The `manager` contract exposes the following execute messages:

### `InstantiateStrategy`

Instantiates a new `strategy` contract.

- **Authorization:** Can be called by any address.
- **Logic:**
  1.  The `ManagerConfig` is loaded from `CONFIG`.
  2.  The provided affiliates are combined with the CALC fee affiliate.
  3.  The strategy is processed to include the affiliates in relevant actions including distributions and thorchain swap routes.
  4.  The strategy is added to the index via a new `StrategyHandle` being saved to the `strategy_store`.
  5.  A `WasmMsg::Instantiate2` message is created to instantiate the new `strategy` contract.

### `ExecuteStrategy`

Executes a `strategy` contract.

- **Authorization:** Can be called by any address.
- **Logic:**
  1.  The `StrategyHandle` is loaded from the `strategy_store`.
  2.  The strategy's status is checked to ensure it is `Active`.
  3.  The `updated_at` timestamp of the `StrategyHandle` is updated.
  4.  A `StrategyExecuteMsg::Execute` message is sent to the `strategy` contract.

### `UpdateStrategy`

Updates a `strategy` contract.

- **Authorization:** Only the `owner` of the strategy can call this message.
- **Logic:**
  1.  The `StrategyHandle` is loaded from the `strategy_store`.
  2.  The `owner` of the strategy is checked against the `info.sender`.
  3.  The strategy is processed to include the affiliates.
  4.  The `updated_at` timestamp of the `StrategyHandle` is updated.
  5.  A `StrategyExecuteMsg::Update` message is sent to the `strategy` contract.

### `UpdateStrategyStatus`

Updates the status of a `strategy` contract.

- **Authorization:** Only the `owner` of the strategy can call this message.
- **Logic:**
  1.  The `StrategyHandle` is loaded from the `strategy_store`.
  2.  The `owner` of the strategy is checked against the `info.sender`.
  3.  The `status` and `updated_at` timestamp of the `StrategyHandle` are updated.
  4.  A `StrategyExecuteMsg::UpdateStatus` message is sent to the `strategy` contract.

## Query Messages

The `manager` contract exposes the following query messages:

### `Config`

- **Returns:** The `ManagerConfig`, containing the `strategy_code_id` and `fee_collector` address.

### `Strategy`

- **Returns:** The `StrategyHandle` for a given strategy address.

### `Strategies`

- **Returns:** A list of `StrategyHandle`s, with optional filtering by `owner` and `status`.
