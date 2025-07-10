# CALC Protocol

## Overview

The CALC protocol is a decentralized framework for creating, managing, and automating on-chain trading strategies. It is built around three core contracts that provide a clear separation of concerns:

- **Strategy:** The runtime environment for a single, declarative trading strategy.
- **Manager:** A factory and registry for creating and managing multiple strategy contracts.
- **Scheduler:** A decentralized automation engine that executes strategies based on predefined conditions.

## How It Works

The protocol enables users to define trading strategies as a tree of `Action`s. This declarative approach separates the user's desired actions (the _what_) from the contract's execution logic (the _how_).

### 1. Defining a Strategy

A strategy is defined using a recursive `Action` enum, which can represent:

- **`Swap`:** A token swap.
- **`Distribute`:** Distributing funds to multiple destinations.
- **`LimitOrder`:** Placing and managing a limit order on a DEX.
- **`Schedule`:** Executing another `Action` on a recurring basis.
- **`Conditional`:** Executing another `Action` only when a specific `Condition` is met.
- **`Many`:** A container for executing multiple `Action`s in sequence.

### 2. Instantiating a Strategy

Strategies are instantiated on-chain via the `manager` contract. The `manager` acts as a factory, creating a new, isolated `strategy` contract for each unique strategy definition.

### 3. Executing a Strategy

Strategies can be executed in two ways:

- **Manually:** Anyone can call the `ExecuteStrategy` message on the `manager` contract to trigger a strategy's execution.
- **Automatically:** The `scheduler` contract automates execution. Users create `Triggers` that link a `Condition` to a strategy. Keepers are incentivized to monitor the `scheduler` and execute these triggers when their conditions are met, creating a decentralised and reliable automation system.

### 4. State Management

The `strategy` contract is stateful and uses a two-phase commit process to manage state transitions. This ensures that the execution of all actions is atomic and that the strategy is always in a consistent state, even when interacting with external protocols.

## Core Features

- **Declarative Strategies:** Define strategies by composing modular `Action` blocks, separating the logic from the execution, enabling complex trading strategies to be built easily.
- **Automated Execution:** The `scheduler` contract provides decentralized automation, incentivizing third-party keepers to execute strategies when their conditions are met.
- **Isolated State:** Each strategy is its own contract, ensuring its state and funds are isolated from others.
- **Centralized Management:** The `manager` contract acts as a central registry, providing a single point for instantiating and discovering strategies.
- **Flexible Conditions:** Strategies can be executed based on time, events, or other conditions, allowing for complex trading strategies that adapt to market conditions.
