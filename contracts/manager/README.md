# Manager Contract

## High-Level Behaviour

The `manager` contract serves as the central factory, registry, and orchestrator for all strategy contracts within the CALC protocol. It acts as the primary gateway for users to create, manage, execute, and fetch trading strategies.

The contract manages the strategy lifecycle, from initial instantiation through execution and eventual archival. It automatically handles affiliate fee integration, validates strategy parameters, and provides efficient querying capabilities through multi-indexed storage. The manager serves as both a smart contract factory and a centralized registry.

## Key Features

- **Strategy Factory:** Deploys new strategy contracts using deterministic instantiate2_address addresses
- **Registry & Discovery:** Maintains searchable registry with multi-indexed storage for efficient queries
- **Affiliate Management:** Integration of affiliate fees with CALC protocol base fees
- **Lifecycle Management:** Strategy status management (Active/Paused/Archived)
- **Access Control:** Owner-based authorization for sensitive operations
- **Metadata Tracking:** Tracking of creation & update timestamps
- **Validation:** Input validation for strategy parameters, labels, and affiliate configurations

## Affiliate Fee System

The manager implements the following affiliate fee system:

- **Base Protocol Fee:** 25 basis points (0.25%) on all distributions
- **Affiliate Allocation:** First 10 bps can reduce protocol fee, additional bps add to total
- **Maximum Affiliate Fees:** 200 basis points (2%) total affiliate fees allowed
- **Automatic Integration:** Affiliate fees are only taken on distribute and withdrawal actions

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
pub struct Strategy {
    pub id: u64,                // Unique sequential identifier
    pub owner: Addr,            // Strategy owner (can update/manage)
    pub contract_address: Addr, // Deployed strategy contract address
    pub created_at: u64,        // Block timestamp of creation
    pub updated_at: u64,        // Block timestamp of last update
    pub label: String,          // Human-readable strategy name (1-100 chars)
    pub status: StrategyStatus, // Current operational status
}
```

### Strategy Status Types

```rust
pub enum StrategyStatus {
    Active,    // Strategy executes normally
    Paused,    // Strategy execution suspended, can be reactivated
}
```

## Execute Messages

### `Instantiate`

Creates and deploys a new strategy contract.

```rust
Instantiate {
    owner: Addr,                     // Strategy owner address
    label: String,                   // Strategy display name (1-100 characters)
    affiliates: Vec<Affiliate>,      // Affiliate fee configuration
    nodes: Vec<Node>,                // DAG node structure (actions and conditions)
}
```

- **Authorization:** Can be called by any address (owner is specified in message)
- **Validation:**
  - Owner address must be valid
  - Label must be 1-100 characters
  - Total affiliate fees cannot exceed 200 bps
- **Logic:**
  1. **Validation:** Validates owner address, label, and affiliate fee limits
  2. **Fee Integration:** Combines provided affiliates with protocol base fee affiliate
  3. **Salt Generation:** Creates deterministic salt from owner, ID, and block height
  4. **Address Generation:** Uses CREATE2 for deterministic contract address
  5. **Registry Update:** Saves strategy metadata to indexed storage
  6. **Contract Deployment:** Dispatches WasmMsg::Instantiate2 to deploy strategy

### `Execute`

Triggers execution of an existing strategy contract.

```rust
Execute {
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

### `Update`

Updates an existing strategy with new node configuration.

```rust
Update {
    contract_address: Addr,      // Strategy contract to update
    nodes: Vec<Node>,            // New DAG node structure
    label: Option<String>,       // Optional new label
}
```

- **Authorization:** Strategy owner only
- **Logic:**
  1. **Owner Verification:** Validates caller is strategy owner
  2. **Label Validation:** If provided, validates label is 1-100 characters
  3. **Registry Update:** Updates strategy label (if provided) and timestamp
  4. **Contract Update:** Dispatches StrategyExecuteMsg::Update(nodes) to strategy contract

### `UpdateStatus`

Changes the operational status of a strategy.

```rust
UpdateStatus {
    contract_address: Addr,       // Strategy contract to update
    status: StrategyStatus,       // New status (Active/Paused)
}
```

- **Authorization:** Strategy owner only
- **Logic:**
  1. **Owner Verification:** Validates caller is strategy owner
  2. **Registry Update:** Updates status and timestamp in registry
  3. **Contract Notification:** Dispatches appropriate message based on status:
     - Active: StrategyExecuteMsg::Execute
     - Paused: StrategyExecuteMsg::Cancel

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
Strategy { address: Addr } -> Strategy
```

**Parameters:**

- `address`: Contract address of the strategy

**Returns:** Complete strategy metadata including ownership, status, and timestamps

### `Strategies`

Lists strategies with optional filtering and pagination.

```rust
Strategies {
    owner: Option<Addr>,           // Filter by strategy owner
    status: Option<StrategyStatus>, // Filter by operational status
    start_after: Option<u64>,      // Pagination cursor (timestamp)
    limit: Option<u16>,            // Result limit (max 30, default 30)
} -> Vec<Strategy>
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
// Primary storage: contract_address -> Strategy
strategies: IndexedMap<Addr, Strategy>

// Indexes for efficient querying:
updated_at: UniqueIndex<String, Strategy>                    // All strategies by update time
owner_updated_at: UniqueIndex<(Addr, String), Strategy>      // By owner + update time
status_updated_at: UniqueIndex<(u8, String), Strategy>       // By status + update time
owner_status_updated_at: UniqueIndex<(Addr, u8, String), Strategy> // Combined filtering
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

1. **Automatic Integration:** Fees automatically added to relevant strategy actions
2. **Flexible Configuration:** Per-strategy affiliate customization

## Security Considerations

- **Access Control:** Owner-only operations for strategy management
- **Input Validation:** Comprehensive validation of all user inputs
- **Fee Limits:** Hard caps on affiliate fees to prevent abuse
- **Registry Integrity:** Immutable strategy ownership and creation timestamps
