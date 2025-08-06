# CALC Protocol

A decentralized framework for creating, managing, and automating on-chain trading strategies built on CosmWasm.

[![License](https://img.shields.io/badge/License-BSL%201.1-orange.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg)](https://www.rust-lang.org)
[![CosmWasm](https://img.shields.io/badge/CosmWasm-2.2+-green.svg)](https://cosmwasm.com)

## Overview

The CALC protocol is a decentralized framework for creating, managing, and automating on-chain trading strategies. It is built around three core contracts that provide a clear separation of concerns:

- **Strategy:** The runtime environment for a single, declarative trading strategy ([docs](contracts/strategy/README.md))
- **Manager:** A factory and registry for creating and managing strategies ([docs](contracts/manager/README.md))
- **Scheduler:** A decentralized automation engine that executes on-chain actions based on triggers ([docs](contracts/scheduler/README.md))

## What Are Strategies?

Think of a strategy as a programmable decision tree that lives on the blockchain. You create a flowchart-like structure of conditions and actions that execute automatically when triggered. Each strategy runs in its own smart contract with its own isolated funds.

### Simple Strategy Examples

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

Strategies can have multiple independent branches that don't need to connect, enabling totally distinct execution paths within the same strategy. Multiple branches can also converge on a common downstream node to ensure certain actions always execute.

```
     ┌────────────────────┐          ┌────────────────────┐
     │  if condition met  ├── then ──┤   execute action   │
     └──────────┬─────────┘          └──────────┬─────────┘
                │                               │
              else                            then
                │                               │
     ┌──────────┴─────────┐          ┌──────────┴─────────┐          ┌────────────────────┐
     │  if condition met  │          │  if condition met  ├── then ──┤   execute action   │
     └──────────┬─────────┘          └──────────┬─────────┘          └──────────┬─────────┘
                │                               │                               │
              then                              │                             then
                │                               │                               │
     ┌──────────┴─────────┐                     │                    ┌──────────┴─────────┐
     │   execute action   │                     │                    │   execute action   │
     └────────────────────┘                     │                    └──────────┬─────────┘
                                                │                               │
                                                │                             then
                                                │                               │
                                                │                    ┌──────────┴─────────┐
                                              else ──────────────────┤   execute action   │
                                                                     └────────────────────┘
```

**Logical AND/OR/NOT Conditions**

Condition nodes can be combined in the following ways to create logical AND, OR, and NOT conditions:

```
    A AND B:                      A OR B:                             NOT A:
   ┌────────────────────┐        ┌────────────────────┐              ┌────────────────────┐
   │   if condition A   │        │   if condition A   ├────┐         │   if condition A   │
   └──────────┬─────────┘        └──────────┬─────────┘    │         └──────────┬─────────┘
              │                             │              │                    │
            then                          else             │                  else
              │                             │              │                    │
   ┌──────────┴─────────┐        ┌──────────┴─────────┐    │         ┌──────────┴─────────┐
   │   if condition B   │        │   if condition B   │   then       │   execute action   │
   └──────────┬─────────┘        └──────────┬─────────┘    │         └────────────────────┘
              │                             │              │
            then                          then             │
              │                             │              │
   ┌──────────┴─────────┐        ┌──────────┴─────────┐    │
   │   execute action   │        │   execute action   ├────┘
   └────────────────────┘        └────────────────────┘
```

### Node types

Strategies contain a set of nodes, each representing a specific action or condition. Nodes can be:

- **Condition nodes:** Check if a condition is met and control the flow of execution
- **Action nodes:** Execute an action and pass control to the next node

**Condition nodes** can be:

- `TimestampElapsed`: Check if a specific time has passed
- `BlocksCompleted`: Check if a specific block height has been reached
- `Schedule`: Check if a time/block/cron/price schedule is ready
- `CanSwap`: Check if market conditions would allow a swap
- `FinLimitOrderFilled`: Check if a limit order was filled
- `BalanceAvailable`: Check if a specific balance is available at a given address
- `StrategyStatus`: Check if another CALC strategy is in a specific status (Active/Paused)
- `OraclePrice`: Check if the current USD price of an assert is above or below a threshold

**Action nodes** can be:

- `Swap`: Execute a swap between two assets under certain market conditions
- `LimitOrder`: Place a limit order with specific parameters
- `Distribute`: Transfer funds to another address, execute another contract with funds, or execute a thorchain `MsgDeposit` with a memo

## Fees

The base CALC automation fee is 25 bps on any funds withdrawn or distributed from a strategy. CALC takes _**no fees**_ on swaps or limit orders executed by strategies, meaning you can set up recurring trading strategies without worrying about losing all your margins to fees.

### Affiliates

Any number of affiliate addresses with custom fee rates can be provided to receive a share of the fees generated by a strategy. Up to 10 bps of the CALC base fee will be instead distributed to affiliates, with any further affiliate bps added on top of the base fee. See [Fee Calculation](contracts/manager/README.md#fee-calculation-examples) examples for a more detailed breakdown of protocol and affiliate fee interactions.

## API Reference

### [Manager Contract](contracts/manager/README.md)

- `Instantiate` Create a new strategy contract with DAG validation
- `Execute` Manually trigger strategy execution
- `Update` Update an existing strategy with new DAG structure (owner only)
- `UpdateStatus` Change strategy status (Active/Paused)
- `UpdateLabel` Change strategy label (1-100 characters)
- `Query` Retrieve strategy information & manager config

### [Scheduler Contract](contracts/scheduler/README.md)

- `Create` Register a new trigger linking conditions to contract execution
- `Execute` Execute triggers when their conditions are satisfied
- `Query` Retrieve triggers to execute and check execution eligibility

### [Strategy Contract](contracts/strategy/README.md)

- `Init` Initialize strategy graph with validation and node setup
- `Execute` Run the strategy's DAG traversal and node execution
- `Update` Replace strategy graph with a new DAG structure (owner only)
- `Withdraw` Retrieve funds from the strategy with affiliate fee processing
- `Cancel` Cancel all active operations and clean up state
- `Process` Internal message for graph traversal and node execution
- `Query` Get strategy configuration and balance information

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
- Special thanks to the CacaoSwap, Rujira, Thorchain & NAMI teams for their contributions and support
