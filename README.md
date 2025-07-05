# CALC Protocol Smart Contracts

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

This repository contains the core smart contracts for the CALC protocol, a decentralized application for creating, managing, and executing complex, automated trading strategies on the Rujira blockchain.

## Overview

The CALC protocol provides a powerful and flexible framework for DeFi users to build sophisticated, conditional trading strategies. The system is designed around a composable, modular architecture that allows for a high degree of customization and extensibility.

At its core, a user defines a **Strategy** as a tree of **Actions** (e.g., swap tokens, place a limit order) that are gated by **Conditions** (e.g., time, price, balance). These strategies are then executed automatically by the protocol's interconnected smart contracts, enabling complex workflows like Time-Weighted Average Price (TWAP) orders, Dollar-Cost Averaging (DCA), and dynamic portfolio rebalancing.

## Architecture

The protocol follows a hub-and-spoke model, with a central `Manager` contract orchestrating the lifecycle of various modular, special-purpose contracts.

```
+-----------------+       +------------------+
|      User       |------>|     Manager      |
+-----------------+       +--------+---------+
                                   |
                                   v
+-----------------+       +--------+---------+       +------------------+
|    Scheduler    |<----->|     Strategy     |<----->|    Exchanger     |
+-----------------+       +------------------+       +------------------+
        ^                          |
        |                          v
+-----------------+       +-----------------+
| External Keeper |       |   Recipients    |
+-----------------+       +-----------------+
```

### Contract Responsibilities

- **Manager (`manager`)**: The central hub of the protocol.
  - **Factory**: Instantiates new `Strategy` contracts using a predictable address (`instantiate2`).
  - **Registry**: Maintains a directory of all strategies, indexed by owner and status for efficient querying.
  - **Lifecycle Control**: Manages the high-level status of strategies (e.g., `Active`, `Paused`, `Archived`).
  - **Affiliate Management**: Handles the registration and fee configuration for protocol affiliates.

- **Strategy (`strategy`)**: The brain of an individual user's automated workflow.
  - **Execution Engine**: Holds the core logic for a single strategy, defined as a tree of `Action`s.
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

The power and flexibility of the CALC protocol come from its composable `Action` and `Condition` system.

- **`Condition`**: A simple, readable enum that defines a specific on-chain state that must be true for an action to proceed. Examples include:
  - `TimestampElapsed(timestamp)`
  - `BlocksCompleted(height)`
  - `BalanceAvailable { ... }`
  - `LimitOrderFilled { ... }`

- **`Action`**: The core building block of a strategy. It is a recursive enum that can represent either a single operation or a complex, nested group of operations.
  - **Leaf Actions**: These are simple, atomic operations like `Swap`, `SetOrder` (for limit orders), or `DistributeTo` (for sending funds).
  - **Composite Actions**: The `Behaviour` and `Crank` actions are composites.
    - `Behaviour`: Groups a `Vec<Action>` together, executing them based on a `Threshold` (`All` or `Any`). This allows for creating complex, parallel, or sequential workflows.
    - `Crank`: Wraps another `Action` and associates it with a `Schedule`. This is the key to creating recurring actions like DCA or TWAP orders.

This recursive, composite structure allows for building strategies of arbitrary depth and complexity, enabling highly sophisticated and adaptive automated trading.

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
