# CALC Protocol Smart Contracts

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

This repository contains the core smart contracts for the CALC protocol, a decentralized application for creating, managing, and executing complex, automated trading strategies on the Rujira blockchain.

## Overview

The CALC protocol provides a framework for building automated trading strategies on the Rujira blockchain. It features a composable, modular architecture designed for customization and extensibility.

At its core, a user defines a **Strategy** as a series of interconnected **Actions**, which are the fundamental units of work. These Actions, implemented via the `Operation` trait, can be atomic operations or complex, nested behaviors. Each Action is gated by **Conditions** (e.g., time, price, balance), ensuring that operations only proceed when specific criteria are met. Strategies are then executed automatically by the protocol's interconnected smart contracts, enabling workflows such as Time-Weighted Average Price (TWAP) orders, Dollar-Cost Averaging (DCA), and dynamic portfolio rebalancing.

## Architecture

The protocol follows a hub-and-spoke model, with a central `Manager` contract orchestrating the lifecycle of various modular, special-purpose contracts.

```
+-------------------+       +------------------+
|       User        |------>|     Manager      |
+-------------------+       +--------+---------+
                                     |
                                     v
+-------------------+       +--------+---------+       +------------------+
|     Scheduler     |<----->|     Strategy     |<----->|    Exchanger     |
+---------+---------+       +--------+---------+       +------------------+
          ^                          |
          |                          v
+---------+---------+       +--------+--------+
|      Executor     |       |   Recipients    |
+-------------------+       +-----------------+
```

### Contract Responsibilities

- **Manager (`manager`)**: The central hub of the protocol.
  - **Factory**: Instantiates new `Strategy` contracts using a predictable address (`instantiate2`).
  - **Registry**: Maintains a directory of all strategies, indexed by owner and status for efficient querying.
  - **Lifecycle Control**: Manages the high-level status of strategies (e.g., `Active`, `Paused`, `Archived`).
  - **Affiliate Management**: Handles the registration and fee configuration for protocol affiliates.

- **Strategy (`strategy`)**: Manages an individual user's automated workflow.
  - **Execution Engine**: Contains the core logic for a single strategy, defined as a tree of `Action`s.
  - **State Machine**: Manages the state of the user's strategy, updating it as actions are executed.
  - **Fund Custody**: Holds the funds required for the strategy's operations.
  - **Composability**: Interacts with other modules like the `Scheduler` and `Exchanger` to execute its defined actions.

- **Scheduler (`scheduler`)**: The protocol's decentralized cron job service.
  - **Trigger Registration**: Allows `Strategy` contracts to register `Triggers`—a message to be executed when a specific set of on-chain `Condition`s are met.
  - **Condition Checking**: When prompted by an external keeper, it checks if any registered triggers are ready to be executed.
  - **Execution**: Dispatches the stored message for any valid trigger. This is the mechanism that enables time-based and recurring actions.

- **Exchanger (`exchanger`)**: A DEX aggregator that provides a unified interface for token swaps.
  - **Abstraction**: Hides the complexity of interacting with various underlying DEX protocols (e.g., FIN, THORChain).
  - **Quote Aggregation**: Provides a query to find the best possible swap rate across all integrated liquidity sources.
  - **Smart Routing**: Executes swaps against the optimal liquidity source to ensure the best execution price.

### Core Concepts: Actions & Conditions

The CALC protocol utilizes a composable `Action` and `Condition` system.

- **`Condition`**: A simple, readable enum that defines a specific on-chain state that must be true for an action to proceed. Examples include:
  - `TimestampElapsed(timestamp)`: Checks if a specific timestamp has been reached.
  - `BlocksCompleted(height)`: Verifies if a certain block height has been surpassed.
  - `BalanceAvailable { ... }`: Ensures a minimum balance of a specific token is available.
  - `LimitOrderFilled { ... }`: Confirms if a previously placed limit order has been filled.
  - `StrategyStatus { ... }`: Checks the current lifecycle status of another strategy.
  - `Compound { ... }`: Combines multiple conditions with logical `AND` (`All`) or `OR` (`Any`) operators.

- **`Action`**: The core building block of a strategy. It is a versatile enum that implements the `Operation` trait, providing a consistent interface for initialization, execution, and state management.
  - **`Check(Condition)`**: Evaluates a given `Condition`.
  - **`Crank(Schedule)`**: Manages scheduled executions, allowing for recurring actions based on block height, time, or cron expressions.
  - **`Perform(Swap)`**: Executes a token swap via the `Exchanger` contract.
  - **`Set(Order)`**: Places or modifies a limit order on a FIN market.
  - **`DistributeTo(Recipients)`**: Distributes funds to predefined recipients.
  - **`Exhibit(Behaviour)`**: A powerful composite action that groups multiple `Action`s.

- **`Behaviour`**: A composite `Action` that groups a vector of other `Action`s. It includes a `Threshold` (`All` or `Any`), determining whether all nested actions must succeed or if any one is sufficient for the `Behaviour` to be considered successful. This supports complex sequential or conditional workflows.

### Control Flow Paradigms

The CALC protocol employs several control flow paradigms to enable sophisticated automated strategies:

1.  **Event-Driven Execution**: The system responds to external messages (e.g., `ExecuteStrategy` from the Manager) and internal events (e.g., a `Scheduler` trigger firing).
2.  **Conditional Gating**: `Condition`s are extensively used to ensure actions only proceed when specific criteria are met, preventing unwanted or invalid operations.
3.  **Hierarchical Composition**: The `Action::Exhibit(Behaviour)` construct allows for building complex, nested execution flows. A `Behaviour` can contain other `Behaviour`s, creating a tree-like structure of operations.
4.  **Stateful Operations**: Strategies maintain their internal state (`StrategyConfig`, `Statistics`), which is updated by executed actions.
5.  **Inter-Contract Communication**: Strategies interact with other contracts (Exchanger, FIN market, Scheduler) using `SubMsg`s, enabling asynchronous operations and handling of replies for result processing (e.g., updating statistics after a swap).
6.  **Lifecycle Management**: The Manager contract provides clear lifecycle states for strategies (Active, Paused, Archived), and state transitions can trigger specific actions (e.g., canceling open orders when archiving).

This combination of modular `Action`s, robust `Condition`s, and flexible control flow mechanisms allows for the creation of highly sophisticated and adaptive automated trading strategies.

## Getting Started

### Prerequisites

- [Rust & Cargo](https://www.rust-lang.org/tools/install)
- [CosmWasm](https://docs.cosmwasm.com/docs/1.0/getting-started/installation)
- [Node.js](https://nodejs.org/en/download/)
- [bun](https://bun.sh/docs/installation)

### Build & Test

1.  **Install dependencies and build contracts:**

    ```bash
    cargo build
    ```

2.  **Run unit and integration tests:**

    ```bash
    cargo test
    ```

3.  **Generate TypeScript types from contract schemas:**
    This command is used by the testing environment and any off-chain clients to ensure type safety.

    ```bash
    bun types
    ```

4.  **Compile contracts to Wasm:**
    This produces the optimized `.wasm` files ready for deployment.
    ```bash
    bun compile
    ```

## License

This project is licensed under the Apache License 2.0. See the [LICENSE](./LICENSE) file for details.
