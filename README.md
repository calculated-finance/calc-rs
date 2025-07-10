# CALC Protocol

## Overview

The CALC protocol is a decentralized framework for creating, managing, and automating on-chain trading strategies. It provides a powerful and flexible set of tools for users to build and deploy sophisticated trading logic that can interact with various DeFi protocols.

The protocol is built around three core contracts:

- **Strategy:** The runtime environment for a single, declarative trading strategy.
- **Manager:** A factory and registry for creating and managing multiple strategy contracts.
- **Scheduler:** A decentralized automation engine that executes strategies based on predefined conditions.

This modular architecture allows for a clear separation of concerns, making the system both robust and extensible.

## How It Works

At its core, the CALC protocol enables users to define complex trading strategies as a tree of `Action`s. These actions are the building blocks of a strategy and can be combined in various ways to create a wide range of trading logic.

### 1. Defining a Strategy

A strategy is defined using a recursive `Action` enum, which can represent:

- **`Swap`:** A simple token swap.
- **`Distribute`:** Distributing funds to multiple destinations.
- **`LimitOrder`:** Placing and managing a limit order on a DEX.
- **`Schedule`:** Executing another `Action` on a recurring basis (e.g., every 100 blocks).
- **`Conditional`:** Executing another `Action` only when a specific `Condition` is met (e.g., when a certain price is reached).
- **`Many`:** A container for executing multiple `Action`s in sequence.

This declarative approach allows users to specify _what_ they want to do, while the protocol handles the _how_.

### 2. Instantiating a Strategy

Once a strategy is defined, it is instantiated on-chain via the `manager` contract. The `manager` acts as a factory, creating a new `strategy` contract for each unique strategy. This isolates the state and execution of each strategy, ensuring that they do not interfere with one another.

### 3. Executing a Strategy

Strategies can be executed in two ways:

- **Manually:** Anyone can call the `ExecuteStrategy` message on the `manager` contract, which will then trigger the execution of the specified `strategy` contract.
- **Automatically:** The `scheduler` contract can be used to automate the execution of strategies. Users can create `Triggers` that will execute a strategy when a specific `Condition` is met. Keepers are incentivized to monitor the `scheduler` and execute these triggers, creating a decentralized and reliable automation system.

### 4. State Management

The `strategy` contract is stateful, with a well-defined lifecycle that ensures the safe and atomic execution of all actions. It uses a two-phase commit process to manage state transitions, ensuring that the strategy is always in a consistent state, even when interacting with external protocols.

## What It Enables

The CALC protocol empowers users to:

- **Create sophisticated trading strategies:** The flexible `Action` system allows for the creation of complex and customized trading logic, from simple DCA bots to advanced, multi-protocol yield farming strategies.
- **Automate their trading:** The `scheduler` contract provides a decentralized and reliable way to automate the execution of strategies, freeing users from the need to manually monitor and execute their trades.
- **Build on a robust and extensible platform:** The modular architecture of the protocol makes it easy to extend and build upon, allowing developers to create new actions, conditions, and even new types of strategies.

By providing a powerful and flexible framework for on-chain automation, the CALC protocol aims to unlock a new wave of innovation in the DeFi space.
