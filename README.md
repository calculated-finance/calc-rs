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

### Advanced Control Flow Patterns

**Disconnected Subtrees & Convergent Branching**

Strategies can have multiple independent branches that don't need to connect. Branches can also converge on common downstream nodes to ensure certain actions always execute. Actions that are connected directly without conditional logic will be executed sequentially regardless of the outcome of each action in the sequence.

```
    ┌────────────────────┐          ┌────────────────────┐
    │  If condition met  ├── then ──┤   execute action   │
    └──────────┬─────────┘          └──────────┬─────────┘
               │                               │
             then                            then
               │                               │
    ┌──────────┴─────────┐          ┌──────────┴─────────┐          ┌────────────────────┐
    │   execute action   │          │  If condition met  ├── then ──┤   execute action   │
    └──────────┬─────────┘          └──────────┬─────────┘          └──────────┬─────────┘
               │                               │                               │
             then                              │                             then
               │                               │                               │
    ┌──────────┴─────────┐                     │                    ┌──────────┴─────────┐
    ┤   execute action   │                     │                    │   execute action   │
    └────────────────────┘                     │                    └──────────┬─────────┘
                                               │                               │
                                               │                             then
                                               │                               │
                                               │                    ┌──────────┴─────────┐
                                             else ──────────────────┤   execute action   │
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
