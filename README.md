# CALC Protocol

A decentralized framework for creating, managing, and automating on-chain trading strategies built on CosmWasm.

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg)](https://www.rust-lang.org)
[![CosmWasm](https://img.shields.io/badge/CosmWasm-2.2+-green.svg)](https://cosmwasm.com)

## Overview

The CALC protocol is a decentralized framework for creating, managing, and automating on-chain trading strategies. It is built around three core contracts that provide a clear separation of concerns:

- **Strategy:** The runtime environment for a single, declarative trading strategy
- **Manager:** A factory and registry for creating and managing multiple strategy contracts
- **Scheduler:** A decentralized automation engine that executes strategies based on predefined conditions

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) 1.70+
- [Docker](https://docs.docker.com/get-docker/) (for contract optimization)
- [Bun](https://bun.sh/) or [Node.js](https://nodejs.org/) (for TypeScript tooling)

### Building the Contracts

1. **Clone the repository:**

   ```bash
   git clone https://github.com/calculated-finance/calc-rs.git
   cd calc-rs
   ```

2. **Build all contracts:**

   ```bash
   cargo build --release
   ```

3. **Generate optimized WASM binaries:**

   ```bash
   bun compile
   ```

4. **Generate contract schemas and TypeScript types:**
   ```bash
   ./scripts/schema.sh
   bun run types
   ```

### Running Tests

```bash
# Run all tests
cargo test

# Run tests for a specific package
cargo test -p calc-rs
cargo test -p manager
cargo test -p scheduler
cargo test -p strategy
```

## Repository Structure

```
calc-rs/
├── contracts/              # CosmWasm smart contracts
│   ├── manager/            # Strategy factory and registry
│   ├── scheduler/          # Automation engine
│   └── strategy/           # Strategy runtime
├── packages/               # Rust libraries
│   ├── calc-rs/            # Core domain logic and types
│   └── calc-rs-test/       # Testing utilities and harness
├── scripts/                # Build and development scripts
├── artifacts/              # Compiled WASM binaries
└── target/                 # Rust build artifacts
```

## How It Works

The protocol enables users to define trading strategies as a tree of `Action`s. This declarative approach separates the user's desired actions (the _what_) from the contract's execution logic (the _how_).

### 1. Defining a Strategy

A strategy is defined using a recursive `Action` enum, which can represent:

- **`Swap`:** Execute token swaps across different DEXs
- **`Distribute`:** Distribute funds to multiple destinations
- **`LimitOrder`:** Place and manage limit orders on decentralized exchanges
- **`Schedule`:** Execute actions on a recurring basis (time-based, block-based, price-based, or cron-like)
- **`Conditional`:** Execute actions only when specific conditions are met
- **`Many`:** Execute multiple actions in sequence

### 2. Instantiating a Strategy

Strategies are instantiated on-chain via the `manager` contract. The `manager` acts as a factory, creating a new, isolated `strategy` contract for each unique strategy definition.

### 3. Executing a Strategy

Strategies can be executed in two ways:

- **Manually:** Anyone can call the `ExecuteStrategy` message on the `manager` contract to trigger a strategy's execution
- **Automatically:** The `scheduler` contract automates execution. Users create `Triggers` that link a `Condition` to a strategy. Keepers are incentivised to monitor the `scheduler` and execute these triggers when their conditions are met

### 4. State Management

The `strategy` contract is stateful and uses a two-phase commit process to manage state transitions. This ensures that the execution of all actions is atomic and that the strategy is always in a consistent state, even when interacting with external protocols.

## Core Features

- **Declarative Strategies:** Define strategies by composing modular `Action` blocks, separating logic from execution
- **Automated Execution:** Decentralized automation through the `scheduler` contract with executor incentives
- **Isolated State:** Each strategy runs in its own contract with isolated state and funds
- **Centralized Management:** The `manager` contract provides a central registry for strategy discovery and management
- **Flexible Conditions:** Support for time-based, event-based, and market condition-based triggers & conditions
- **Affiliate Support:** Built-in support for affiliate fees and revenue sharing
- **Multi-DEX Integration:** Support for swaps across multiple decentralized exchanges

## Action Types

### Basic Actions

- **Swap:** Execute token swaps with configurable routes and slippage protection
- **LimitOrder:** Place limit orders on DEXs that support them
- **Distribute:** Send tokens to multiple addresses with percentage-based allocation

### Composite Actions

- **Schedule:** Recurring execution based on:
  - Time intervals (every N seconds/minutes/hours)
  - Block intervals (every N blocks)
  - Cron expressions for complex scheduling
- **Conditional:** Execute actions when conditions are satisfied:
  - Time-based conditions
  - Balance thresholds
  - Market conditions
  - External price feeds
- **Many:** Combine multiple actions into a single execution sequence

## Development

### Project Structure

The repository is organized as a Rust workspace with multiple packages:

- **`packages/calc-rs`:** Core domain logic, types, and shared functionality
- **`packages/calc-rs-test`:** Testing utilities, fixtures, and integration test harness
- **`contracts/manager`:** Factory contract for creating and managing strategies
- **`contracts/scheduler`:** Automation engine for trigger-based execution
- **`contracts/strategy`:** Runtime environment for individual strategies

### Development Scripts

- **`./scripts/compile.sh`:** Build optimized WASM binaries using cosmwasm/optimizer
- **`./scripts/schema.sh`:** Generate JSON schemas for all contracts
- **`bun run types`:** Generate TypeScript type definitions from schemas

### Testing

The project includes comprehensive test coverage:

```bash
# Run all tests
cargo test

# Run integration tests
cargo test -p calc-rs-test

# Run contract-specific tests
cargo test -p manager
cargo test -p scheduler
cargo test -p strategy
```

### Code Quality

- **Linting:** `cargo clippy` for Rust linting
- **Formatting:** `cargo fmt` for consistent code formatting
- **Documentation:** `cargo doc --open` to build and view documentation

## Architecture

### Contract Interactions

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│   Manager   │    │  Scheduler  │    │  Strategy   │
│  (Factory)  │    │(Automation) │    │ (Runtime)   │
└─────┬───────┘    └─────┬───────┘    └─────┬───────┘
      │                  │                  │
      │ InstantiateStrategy                 │
      ├─────────────────────────────────────▶
      │                  │                  │
      │ ExecuteStrategy  │                  │
      ├─────────────────────────────────────▶
      │                  │                  │
      │                  │ Create Trigger   │
      │                  ◀─────────────────
      │                  │                  │
      │                  │ Execute Trigger  │
      │                  ├─────────────────▶
      │                  │                  │
```

### State Machine

Each strategy contract follows a state machine pattern:

1. **Committed:** Strategy is ready for execution
2. **Active:** Strategy is currently executing actions
3. **Executable:** Strategy has completed execution and is ready to be saved

This ensures atomic execution and consistent state management.

## API Reference

### Manager Contract

- **InstantiateStrategy:** Create a new strategy contract
- **ExecuteStrategy:** Manually execute a strategy
- **UpdateStrategy:** Update an existing strategy (owner only)
- **Query:** Retrieve strategy information and status

### Scheduler Contract

- **Create:** Register a new trigger
- **Execute:** Execute triggers when conditions are met
- **Query:** Retrieve trigger information and status

### Strategy Contract

- **Execute:** Run the strategy's action tree
- **Update:** Modify the strategy configuration
- **Withdraw:** Retrieve funds from the strategy
- **Query:** Get strategy state and execution history

## Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/my-new-feature`
3. Make your changes and add tests
4. Run the test suite: `cargo test`
5. Ensure code quality: `cargo clippy && cargo fmt`
6. Commit your changes: `git commit -am 'Add some feature'`
7. Push to the branch: `git push origin feature/my-new-feature`
8. Submit a pull request

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Built with [CosmWasm](https://cosmwasm.com/)
- Inspired by decentralized finance automation needs
- Special thanks to the Rujira & Thorchain teams for their contributions and support
