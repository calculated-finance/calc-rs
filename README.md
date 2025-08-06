# CALC Protocol

A decentralized framework for creating, managing, and automating on-chain trading strategies built on CosmWasm.

[![License](https://img.shields.io/badge/License-BSL%201.1-orange.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg)](https://www.rust-lang.org)
[![CosmWasm](https://img.shields.io/badge/CosmWasm-2.2+-green.svg)](https://cosmwasm.com)

## Overview

The CALC protocol is a decentralized framework for creating, managing, and automating on-chain trading strategies. It is built around three core contracts that provide a clear separation of concerns:

- **Strategy:** The runtime environment for a single, declarative trading strategy
- **Manager:** A factory and registry for creating and managing multiple strategy contracts
- **Scheduler:** A decentralized automation engine that executes strategies based on predefined conditions

## What Are Strategies?

Think of a strategy as a programmable decision tree that lives on the blockchain. You create a flowchart-like structure of conditions and actions that execute automatically when triggered. Each strategy runs in its own smart contract with its own isolated funds.

### Simple Strategy Examples

**Automated Market Making:**

```
                                ┌────────────────────────┐
                                │     Every 5 blocks     │
                                └────────────┬───────────┘
                                ┌────────────┴───────────┐
                                │   Claim & reset sell   │
                                │  limit order at 1bps   │
                                │    above ask price     │
                                └────────────┬───────────┘
                                ┌────────────┴───────────┐
                                │   Claim & reset buy    │
                                │  limit order at 1bps   │
                                │    below bid price     │
                                └────────────────────────┘
```

**TWAP execution:**

```
                                ┌────────────────────────┐
                                │       Every hour       │
                                └────────────┬───────────┘
                                ┌────────────┴───────────┐
                                │ Swap 100 USDC for RUNE │
                                │  with max 2% slippage  │
                                └────────────┬───────────┘
                                ┌────────────┴───────────┐
                                │ Send 50% RUNE to bank  │
                                │    and 50% to other    │
                                │    trading strategy    │
                                └────────────────────────┘
```

**GRID bot:**

```
                                ┌────────────────────────┐
                                │    Every 50 blocks     │
                                └────────────┬───────────┘
                                ┌────────────┴───────────┐
                                │ Try swap 100 RUNE into │
                                │   at least 110 RUJI    │
                                └────────────┬───────────┘
                                ┌────────────┴───────────┐
                                │ Try swap 100 RUJI into │
                                │   at least 110 RUNE    │
                                └────────────────────────┘
```

### How Execution Actually Works

1. **Single Path Traversal:** Each execution follows one path through the graph from start to finish
2. **Conditional Branching:** Conditions evaluate to true/false and branch accordingly
3. **Message Execution:** When actions generate blockchain messages, they are executed immediately
4. **Automatic Continuation:** After messages complete, execution resumes from the next node
5. **State Persistence:** Each node's state is saved individually and survives between executions

### Advanced Control Flow Patterns

**Convergent Branching:**
Multiple paths can converge on the same downstream node, enabling sophisticated decision trees:

```
    ┌────────────────────┐          ┌────────────────────┐
    │  If condition met  ├── then ──┤   execute action   │
    └──────────┬─────────┘          └──────────┬─────────┘
               │                               │
               │                             then
               │                               │
               │                    ┌──────────┴─────────┐          ┌────────────────────┐
               │                    │  If condition met  ├── then ──┤   execute action   │
               │                    └──────────┬─────────┘          └──────────┬─────────┘
               │                               │                               │
               │                               │                             then
               │                               │                               │
               │                               │                    ┌──────────┴─────────┐
               │                               │                    │   execute action   │
               │                               │                    └──────────┬─────────┘
               │                               │                               │
               │                               │                             then
               │                               │                               │
               │                               │                    ┌──────────┴─────────┐
              else ───────────────────────── else ──────────────────┤   execute action   │
                                                                    └────────────────────┘
```

**Scheduling:**
The Schedule condition supports multiple cadence types:

- **Time intervals:** Execute every N seconds
- **Block intervals:** Execute every N blocks
- **Cron expressions:** "0 9 \* \* MON" (every Monday at 9 AM)
- **Price triggers:** Execute every time a limit order is filled

**Dynamic Limit Orders:**
Limit orders can use the following pricing strategies:

- **Fixed pricing:** Always place orders at $5.00
- **Dynamic offset:** Place orders 2% above current market price, only resetting order if price moves >1% from current order price

### Key Limitations & Realities

- **Trigger-Based Execution:** Strategies don't run continuously - they must be triggered manually or via the scheduler
- **Single Execution Path:** Each trigger follows one path through the graph, then stops
- **Deterministic Only:** Can only use on-chain data (token balances, DEX prices, oracle feeds)

### The Power of Automation

Despite constraints, strategies enable:

- **Systematic Trading:** Execute pre-defined logic without human supervision
- **24/7 Operation:** Automated triggers through decentralized keeper networks
- **Complex Logic:** Combine simple building blocks into sophisticated decision trees
- **Risk Management:** Built-in slippage protection, balance checks, and error handling
- **Composability:** Strategies can monitor and interact with other strategies

Think of CALC strategies as "smart standing orders with branching logic" rather than high-frequency trading algorithms. They excel at systematic, rule-based trading that adapts to market conditions.

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
   bun types
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

The protocol enables users to define trading strategies as directed acyclic graphs (DAGs) of interconnected nodes. This declarative approach separates the user's desired actions (the _what_) from the contract's execution logic (the _how_), providing a flexible framework for creating on chain trading strategies.

### 1. Defining a Strategy

A strategy is defined as a DAG (Directed Acyclic Graph) of interconnected nodes:

#### Node Types

**Action Nodes** execute concrete operations:

- **`Swap`:** Execute a token swap across multiple DEX protocols with slippage protection
- **`Distribute`:** Send funds to multiple recipients with share-based allocations
- **`LimitOrder`:** Place and manage static or dynamic limit orders

**Condition Nodes** provide branching logic:

- **Time-based:** `TimestampElapsed`, `BlocksCompleted`, `Schedule` for temporal execution
- **Market-based:** `CanSwap`, `LimitOrderFilled`, `OraclePrice` for market conditions
- **Balance-based:** `BalanceAvailable`, `StrategyBalanceAvailable` for fund checks

#### Graph Structure

Each node contains an index and references to subsequent nodes. Action nodes have a `next` field, while condition nodes have `on_success` and `on_failure` edges, enabling branching logic:

```
Node 0: Condition(PriceCheck)
    ├─ on_success: Node 2 (Swap)
    └─ on_failure: Node 1 (Distribute)

Node 1: Action(Distribute) → next: None
Node 2: Action(Swap) → next: Node 3 (LimitOrder)
Node 3: Action(LimitOrder) → next: None
```

### 2. Instantiating a Strategy

Strategies are instantiated on-chain via the `manager` contract, which acts as a factory:

1. **Validation:** The manager validates the strategy graph structure and prevents cycles
2. **Deployment:** Creates a new isolated `strategy` contract using deterministic addresses
3. **Initialization:** The strategy contract validates the graph and initializes all nodes
4. **Auto-execution:** Immediately begins the first execution cycle

### 3. Executing a Strategy

#### Sequential Graph Traversal

The strategy contract executes nodes sequentially following the graph edges:

1. **Linear Execution:** Action nodes execute and proceed to their `next` node
2. **Conditional Branching:** Condition nodes evaluate and follow `on_success` or `on_failure` edges
3. **Fresh Balances:** Each operation queries fresh balances for accurate execution
4. **Message Generation:** When external calls are needed, execution pauses and resumes after completion
5. **Termination:** Execution completes when a terminal node is reached (i.e. an action with no `next` pointer, or a condition with `None` for its relevant `on_success` or `on_failure` pointer)

#### Execution Triggers

Strategies can be executed in multiple ways:

- **Manual:** Anyone can call `Execute` on the `manager` contract to trigger execution
- **Automated:** The `scheduler` contract enables automated execution via `Triggers`
- **Conditional:** Keepers monitor conditions and execute triggers when satisfied
- **Self-triggering:** Strategies can schedule their own re-execution via time-based conditions

### 4. State Management and Updates

The strategy contract implements sophisticated state management:

#### Operation Lifecycle

Each operation follows a standardized lifecycle through the Operation trait:

- **`init`:** Initialize the operation with validation and setup
- **`execute`:** Generate blockchain messages and update state
- **`commit`:** Finalize state changes after successful execution
- **`cancel`:** Clean up state and unwind positions

#### Hot-swapping Strategies

The contract supports dynamic strategy updates through a three-phase process:

1. **Cancel Phase:** Existing strategy executes in Cancel mode to clean up state
2. **Replace Phase:** New strategy graph is initialized and validated
3. **Execute Phase:** New strategy immediately begins execution

This enables safe updates without losing funds or corrupting state.

#### Fund Isolation

Each strategy contract manages its own isolated funds:

- **Denomination Tracking:** Automatically tracks all tokens used by the strategy
- **Balance Queries:** Real-time balance reporting across all holdings

## Core Features

- **DAG-Based Execution:** Strategies are represented as directed acyclic graphs with action and condition nodes
- **Sequential Processing:** Ensures fresh balance queries between operations for accurate execution
- **Conditional Branching:** Condition nodes enable complex control flow based on runtime evaluation
- **Operation Polymorphism:** Unified operation interface supporting swaps, limit orders, distributions, and more
- **Cycle Prevention:** Built-in validation ensures strategies cannot create infinite execution loops
- **Automated Execution:** Decentralized automation through the `scheduler` contract with keeper incentives
- **Fund Isolation:** Each strategy runs in its own contract with isolated state and funds
- **Hot-swapping:** Dynamic strategy updates with safe state transitions
- **Centralized Management:** The `manager` contract provides a central registry for strategy discovery and management
- **Affiliate Support:** Built-in support for affiliate fees and revenue sharing
- **Multi-DEX Integration:** Support for swaps across multiple decentralized exchanges

## Node Types and Operations

### Action Nodes

Action nodes represent concrete operations that modify state or generate blockchain messages:

- **Swap:** Execute token swaps across multiple DEX protocols with configurable routes and slippage protection
- **LimitOrder:** Place and manage static or dynamic limit orders on supported DEXs
- **Distribute:** Send tokens to multiple addresses with percentage-based allocation and affiliate fee integration

### Condition Nodes

Condition nodes provide branching logic and control flow based on runtime evaluation:

#### Time-based Conditions

- **TimestampElapsed:** Check if a specific timestamp has passed
- **BlocksCompleted:** Check if a certain number of blocks have elapsed
- **Schedule:** Complex scheduling with cron-like expressions for recurring execution

#### Market-based Conditions

- **CanSwap:** Verify if a swap is possible with current liquidity
- **LimitOrderFilled:** Check if a limit order has been filled
- **OraclePrice:** Compare current price against oracle data

#### Balance-based Conditions

- **BalanceAvailable:** Check if sufficient balance is available for operations
- **StrategyBalanceAvailable:** Check balances across all strategy holdings

#### Logical Conditions

- **Not:** Logical negation of other conditions for complex logic

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

Each strategy contract implements a sophisticated execution model:

1. **Graph Validation:** Strategy graphs are validated during initialization using topological sorting to prevent cycles
2. **Sequential Traversal:** Nodes execute sequentially following graph edges with conditional branching
3. **Fresh Balance Queries:** Each operation queries current balances for accurate execution
4. **Message Pausing:** When external calls are needed, execution pauses and resumes after completion
5. **State Persistence:** Node state updates are saved individually for optimal storage and recovery

The execution engine ensures atomic operations and consistent state management across all strategy types.

## API Reference

### Manager Contract

- **Instantiate:** Create a new strategy contract with DAG validation
- **Execute:** Manually trigger strategy execution
- **Update:** Update an existing strategy with new DAG structure (owner only)
- **UpdateStatus:** Change strategy status (Active/Paused/Archived)
- **Query:** Retrieve strategy information, statistics, and registry data

### Scheduler Contract

- **Create:** Register a new trigger linking conditions to contract execution
- **Execute:** Execute triggers when their conditions are satisfied
- **Query:** Retrieve trigger information and check execution eligibility

### Strategy Contract

- **Init:** Initialize strategy graph with validation and node setup (auto-called)
- **Execute:** Run the strategy's DAG traversal and node execution
- **Update:** Replace strategy with new DAG via three-phase hot-swap process
- **Withdraw:** Retrieve funds from the strategy with affiliate fee processing
- **Cancel:** Cancel all active operations and clean up state
- **Process:** Internal message for graph traversal and node execution
- **Query:** Get strategy configuration, statistics, and balance information

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

This project is licensed under the Business Source License 1.1 (BSL). See the [LICENSE](LICENSE) file for details.

Summary:

- Non-commercial use only until August 5, 2028
- On August 5, 2028, the license automatically converts to Apache License, Version 2.0
- For more information, see https://mariadb.com/bsl11

## Acknowledgments

- Built with [CosmWasm](https://cosmwasm.com/)
- Inspired by decentralized finance automation needs
- Special thanks to the Rujira & Thorchain teams for their contributions and support
