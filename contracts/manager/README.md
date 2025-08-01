# Manager Contract

## High-Level Behaviour

The `manager` contract serves as the central factory, registry, and orchestrator for all strategy contracts within the CALC protocol. It acts as the primary gateway for users to create, manage, execute, and fetch trading strategies.

The contract manages the strategy lifecycle, from initial instantiation through execution and eventual archival. It automatically handles affiliate fee integration, validates strategy parameters, and provides efficient querying capabilities through multi-indexed storage. The manager serves as both a smart contract factory and a centralized registry, enabling efficient strategy management at scale.

## Key Features

- **Strategy Factory:** Deploys new strategy contracts using deterministic instantiate2_address addresses
- **Registry & Discovery:** Maintains searchable registry with multi-indexed storage for efficient queries
- **Affiliate Management:** Automatic integration of affiliate fees with CALC protocol base fees
- **Lifecycle Management:** Complete strategy status management (Active/Paused/Archived)
- **Access Control:** Owner-based authorization for sensitive operations
- **Metadata Tracking:** Comprehensive tracking of creation times, updates, and labels
- **Validation:** Input validation for strategy parameters, labels, and affiliate configurations

## Affiliate Fee System

The manager implements a sophisticated affiliate fee system:

- **Base Protocol Fee:** 25 basis points (0.25%) on all distributions
- **Affiliate Allocation:** First 10 bps can reduce protocol fee, additional bps add to total
- **Maximum Affiliate Fees:** 200 basis points (2%) total affiliate fees allowed
- **Automatic Integration:** Affiliate fees automatically added to distribute actions

### Fee Calculation Examples

```
Example 1: 5 bps affiliate fee
- Protocol fee: 20 bps (25 - 5)
- Affiliate fee: 5 bps
- Total fee: 25 bps

Example 2: 15 bps affiliate fee
- Protocol fee: 15 bps (25 - 10, maximum reduction)
- Affiliate fee: 15 bps
- Total fee: 30 bps

Example 3: 50 bps affiliate fee
- Protocol fee: 15 bps (25 - 10, maximum reduction)
- Affiliate fee: 50 bps
- Total fee: 65 bps
```

## Contract Configuration

```rust
pub struct ManagerConfig {
    pub fee_collector: Addr,    // Address receiving protocol fees
    pub strategy_code_id: u64,  // Code ID for strategy contract instantiation
}
```

## Strategy Registry

Each strategy is tracked with comprehensive metadata:

```rust
pub struct StrategyHandle {
    pub id: u64,                    // Unique sequential identifier
    pub owner: Addr,                // Strategy owner (can update/manage)
    pub contract_address: Addr,     // Deployed strategy contract address
    pub created_at: u64,            // Block timestamp of creation
    pub updated_at: u64,            // Block timestamp of last update
    pub label: String,              // Human-readable strategy name (1-100 chars)
    pub status: StrategyStatus,     // Current operational status
    pub affiliates: Vec<Affiliate>, // Affiliate fee configuration
}
```

### Strategy Status Types

```rust
pub enum StrategyStatus {
    Active,    // Strategy executes normally
    Paused,    // Strategy execution suspended, can be reactivated
    Archived,  // Strategy permanently disabled
}
```

## Instantiate Message

```rust
ManagerConfig {
    fee_collector: Addr,
    strategy_code_id: u64,
}
```

Initializes a new manager contract instance.

- **Authorization:** Can be called by any address
- **Parameters:**
  - `fee_collector`: Address that will receive protocol fees from strategies
  - `strategy_code_id`: Code ID of the strategy contract for CREATE2 deployment
- **Logic:**
  1. Saves configuration to storage
  2. Initializes strategy counter to 0
  3. Sets up indexed storage for strategy registry

## Execute Messages

### `InstantiateStrategy`

Creates and deploys a new strategy contract.

```rust
InstantiateStrategy {
    label: String,                    // Strategy display name (1-100 characters)
    affiliates: Vec<Affiliate>,       // Affiliate fee configuration
    strategy: Strategy<Json>,         // Strategy definition with actions
}
```

- **Authorization:** Can be called by any address (caller becomes strategy owner)
- **Validation:**
  - Label must be 1-100 characters
  - Strategy owner address must be valid
  - Total affiliate fees cannot exceed 200 bps
- **Logic:**
  1. **Validation:** Validates input parameters and affiliate fee limits
  2. **Fee Integration:** Combines user affiliates with protocol base fee affiliate
  3. **Strategy Processing:** Integrates affiliates into strategy actions (distributions, swaps)
  4. **Address Generation:** Uses CREATE2 for deterministic contract address
  5. **Registry Update:** Saves strategy metadata to indexed storage
  6. **Contract Deployment:** Dispatches WasmMsg::Instantiate2 to deploy strategy

### `ExecuteStrategy`

Triggers execution of an existing strategy contract.

```rust
ExecuteStrategy {
    contract_address: Addr,  // Address of strategy to execute
}
```

- **Authorization:** Can be called by any address (typically keepers or automation)
- **Logic:**
  1. **Registry Lookup:** Loads strategy metadata from registry
  2. **Status Validation:** Ensures strategy status is Active
  3. **Timestamp Update:** Updates strategy's last execution timestamp
  4. **Execution Call:** Dispatches StrategyExecuteMsg::Execute to strategy contract
  5. **Fund Forwarding:** Forwards any sent funds to strategy execution

### `UpdateStrategy`

Updates an existing strategy with new action configuration.

```rust
UpdateStrategy {
    contract_address: Addr,      // Strategy contract to update
    update: Strategy<Json>,      // New strategy definition
}
```

- **Authorization:** Strategy owner only
- **Logic:**
  1. **Owner Verification:** Validates caller is strategy owner
  2. **Affiliate Preservation:** Maintains existing affiliate configuration
  3. **Strategy Processing:** Integrates affiliates into new strategy actions
  4. **Registry Update:** Updates strategy metadata and timestamp
  5. **Contract Update:** Dispatches StrategyExecuteMsg::Update to strategy contract

### `UpdateStrategyStatus`

Changes the operational status of a strategy.

```rust
UpdateStrategyStatus {
    contract_address: Addr,       // Strategy contract to update
    status: StrategyStatus,       // New status (Active/Paused/Archived)
}
```

- **Authorization:** Strategy owner only
- **Logic:**
  1. **Owner Verification:** Validates caller is strategy owner
  2. **Registry Update:** Updates status and timestamp in registry
  3. **Contract Notification:** Dispatches StrategyExecuteMsg::UpdateStatus to strategy
  4. **State Management:** Strategy contract handles state transitions appropriately

## Query Messages

### `Config`

Returns the current manager configuration.

```rust
Config {} -> ManagerConfig
```

**Returns:**

- `fee_collector`: Current protocol fee recipient address
- `strategy_code_id`: Code ID used for strategy deployment

### `Strategy`

Retrieves detailed information about a specific strategy.

```rust
Strategy { address: Addr } -> StrategyHandle
```

**Parameters:**

- `address`: Contract address of the strategy

**Returns:** Complete strategy metadata including ownership, status, timestamps, and affiliate configuration

### `Strategies`

Lists strategies with optional filtering and pagination.

```rust
Strategies {
    owner: Option<Addr>,           // Filter by strategy owner
    status: Option<StrategyStatus>, // Filter by operational status
    start_after: Option<u64>,      // Pagination cursor (timestamp)
    limit: Option<u16>,            // Result limit (max 30, default 30)
} -> Vec<StrategyHandle>
```

**Filtering Options:**

- **By Owner:** Returns all strategies owned by specific address
- **By Status:** Returns all strategies with specific status (Active/Paused/Archived)
- **Combined:** Owner + Status for precise filtering
- **No Filter:** Returns all strategies (paginated)

**Ordering:** Results ordered by `updated_at` timestamp in descending order (newest first)

## Storage Architecture

### Multi-Indexed Registry

The manager uses indexing for efficient strategy queries:

```rust
// Primary storage: contract_address -> StrategyHandle
strategies: IndexedMap<Addr, StrategyHandle>

// Indexes for efficient querying:
updated_at: UniqueIndex<String, StrategyHandle>                    // All strategies by update time
owner_updated_at: UniqueIndex<(Addr, String), StrategyHandle>      // By owner + update time
status_updated_at: UniqueIndex<(u8, String), StrategyHandle>       // By status + update time
owner_status_updated_at: UniqueIndex<(Addr, u8, String), StrategyHandle> // Combined filtering
```

### Cursor-Based Pagination

- **Timestamp Cursors:** Uses formatted timestamps for deterministic pagination
- **Efficient Iteration:** Indexed storage enables fast lookups
- **Consistent Ordering:** Guaranteed ordering across paginated queries

## Integration Patterns

### Factory Pattern

The manager implements the factory pattern for strategy deployment:

1. **Deterministic Addresses:** `instantiate2_address` ensures predictable contract addresses
2. **Standardized Deployment:** All strategies follow same instantiation pattern
3. **Registry Integration:** Automatic registration upon successful deployment

### Registry Pattern

Comprehensive strategy registry with rich metadata:

1. **Discovery:** Efficient querying and filtering capabilities
2. **Governance:** Status management for protocol governance
3. **Analytics:** Timestamp tracking for usage analytics
4. **Authorization:** Owner-based access control

### Fee Management

Integrated affiliate and protocol fee management:

1. **Automatic Integration:** Fees automatically added to strategy actions
2. **Flexible Configuration:** Per-strategy affiliate customization
3. **Protocol Sustainability:** Guaranteed protocol fee collection

## Security Considerations

- **Access Control:** Owner-only operations for sensitive strategy management
- **Input Validation:** Comprehensive validation of all user inputs
- **Fee Limits:** Hard caps on affiliate fees to prevent abuse
- **Registry Integrity:** Immutable strategy ownership and creation timestamps

## Migration Support

The contract includes migration functionality:

- **Configuration Updates:** Ability to update fee collector and code ID
- **Backwards Compatibility:** Migration preserves existing strategy registry
- **Version Management:** Supports protocol upgrades while maintaining state
