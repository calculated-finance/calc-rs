export type Addr = string;
export type StrategyStatus = "active" | "paused" | "archived";
export type ArrayOf_StrategyHandle = StrategyHandle[];

export interface StrategyHandle {
  affiliates: Affiliate[];
  contract_address: Addr;
  created_at: number;
  id: number;
  label: string;
  owner: Addr;
  status: StrategyStatus;
  updated_at: number;
}
export interface Affiliate {
  address: Addr;
  bps: number;
  label: string;
}

export interface ManagerInstantiateMsg {
  fee_collector: Addr;
  strategy_code_id: number;
}
export type ManagerQueryMsg =
  | {
      config: {};
    }
  | {
      strategy: {
        address: Addr;
      };
    }
  | {
      strategies: {
        limit?: number | null;
        owner?: Addr | null;
        start_after?: number | null;
        status?: StrategyStatus | null;
      };
    };
export type ManagerExecuteMsg =
  | {
      instantiate_strategy: {
        affiliates: Affiliate[];
        label: string;
        strategy: StrategyFor_Json;
      };
    }
  | {
      execute_strategy: {
        contract_address: Addr;
      };
    }
  | {
      update_strategy_status: {
        contract_address: Addr;
        status: StrategyStatus;
      };
    }
  | {
      update_strategy: {
        contract_address: Addr;
        update: StrategyFor_Json;
      };
    };
export type Action =
  | {
      swap: Swap;
    }
  | {
      limit_order: LimitOrder;
    }
  | {
      distribute: Distribution;
    }
  | {
      schedule: Schedule;
    }
  | {
      conditional: Conditional;
    }
  | {
      many: Action[];
    };
export type SwapAmountAdjustment =
  | "fixed"
  | {
      linear_scalar: {
        base_receive_amount: Coin;
        minimum_swap_amount?: Coin | null;
        scalar: Decimal;
      };
    };
/**
 * A thin wrapper around u128 that is using strings for JSON encoding/decoding, such that the full u128 range can be used for clients that convert JSON numbers to floats, like JavaScript and jq.
 *
 * # Examples
 *
 * Use `from` to create instances of this and `u128` to get the value out:
 *
 * ``` # use cosmwasm_std::Uint128; let a = Uint128::from(123u128); assert_eq!(a.u128(), 123);
 *
 * let b = Uint128::from(42u64); assert_eq!(b.u128(), 42);
 *
 * let c = Uint128::from(70u32); assert_eq!(c.u128(), 70); ```
 */
export type Uint128 = string;
/**
 * A fixed-point decimal value with 18 fractional digits, i.e. Decimal(1_000_000_000_000_000_000) == 1.0
 *
 * The greatest possible value that can be represented is 340282366920938463463.374607431768211455 (which is (2^128 - 1) / 10^18)
 */
export type Decimal = string;
export type SwapRoute =
  | {
      fin: FinRoute;
    }
  | {
      thorchain: ThorchainRoute;
    };
export type Side = "base" | "quote";
export type OrderPriceStrategy =
  | {
      fixed: Decimal;
    }
  | {
      offset: {
        direction: Direction;
        offset: Offset;
        tolerance: Offset;
      };
    };
export type Direction = "above" | "below";
export type Offset =
  | {
      exact: Decimal;
    }
  | {
      percent: number;
    };
export type Recipient =
  | {
      bank: {
        address: Addr;
      };
    }
  | {
      contract: {
        address: Addr;
        msg: Binary;
      };
    }
  | {
      deposit: {
        memo: string;
      };
    }
  | {
      strategy: {
        contract_address: Addr;
      };
    };
/**
 * Binary is a wrapper around Vec<u8> to add base64 de/serialization with serde. It also adds some helper methods to help encode inline.
 *
 * This is only needed as serde-json-{core,wasm} has a horrible encoding for Vec<u8>. See also <https://github.com/CosmWasm/cosmwasm/blob/main/docs/MESSAGE_TYPES.md>.
 */
export type Binary = string;
export type Cadence =
  | {
      blocks: {
        interval: number;
        previous?: number | null;
      };
    }
  | {
      time: {
        duration: Duration;
        previous?: Timestamp | null;
      };
    }
  | {
      cron: {
        expr: string;
        previous?: Timestamp | null;
      };
    }
  | {
      limit_order: {
        pair_address: Addr;
        previous?: Decimal | null;
        side: Side;
        strategy: OrderPriceStrategy;
      };
    };
/**
 * A point in time in nanosecond precision.
 *
 * This type can represent times from 1970-01-01T00:00:00Z to 2554-07-21T23:34:33Z.
 *
 * ## Examples
 *
 * ``` # use cosmwasm_std::Timestamp; let ts = Timestamp::from_nanos(1_000_000_202); assert_eq!(ts.nanos(), 1_000_000_202); assert_eq!(ts.seconds(), 1); assert_eq!(ts.subsec_nanos(), 202);
 *
 * let ts = ts.plus_seconds(2); assert_eq!(ts.nanos(), 3_000_000_202); assert_eq!(ts.seconds(), 3); assert_eq!(ts.subsec_nanos(), 202); ```
 */
export type Timestamp = Uint64;
/**
 * A thin wrapper around u64 that is using strings for JSON encoding/decoding, such that the full u64 range can be used for clients that convert JSON numbers to floats, like JavaScript and jq.
 *
 * # Examples
 *
 * Use `from` to create instances of this and `u64` to get the value out:
 *
 * ``` # use cosmwasm_std::Uint64; let a = Uint64::from(42u64); assert_eq!(a.u64(), 42);
 *
 * let b = Uint64::from(70u32); assert_eq!(b.u64(), 70); ```
 */
export type Uint64 = string;
export type Condition =
  | {
      timestamp_elapsed: Timestamp;
    }
  | {
      blocks_completed: number;
    }
  | {
      can_swap: Swap;
    }
  | {
      limit_order_filled: {
        owner: Addr;
        pair_address: Addr;
        price: Decimal;
        side: Side;
      };
    }
  | {
      balance_available: {
        address: Addr;
        amount: Coin;
      };
    }
  | {
      strategy_balance_available: {
        amount: Coin;
      };
    }
  | {
      strategy_status: {
        contract_address: Addr;
        manager_contract: Addr;
        status: StrategyStatus;
      };
    }
  | {
      oracle_price: {
        asset: string;
        direction: Direction;
        rate: Decimal;
      };
    }
  | {
      not: Condition;
    }
  | {
      composite: CompositeCondition;
    };
export type Threshold = "all" | "any";
export type Json = null;

export interface StrategyFor_Json {
  action: Action;
  owner: Addr;
  state: Json;
}
export interface Swap {
  adjustment: SwapAmountAdjustment;
  maximum_slippage_bps: number;
  minimum_receive_amount: Coin;
  routes: SwapRoute[];
  swap_amount: Coin;
}
export interface Coin {
  amount: Uint128;
  denom: string;
}
export interface FinRoute {
  pair_address: Addr;
}
export interface ThorchainRoute {
  affiliate_bps?: number | null;
  affiliate_code?: string | null;
  latest_swap?: StreamingSwap | null;
  max_streaming_quantity?: number | null;
  streaming_interval?: number | null;
}
export interface StreamingSwap {
  expected_receive_amount: Coin;
  memo: string;
  starting_block: number;
  streaming_swap_blocks: number;
  swap_amount: Coin;
}
export interface LimitOrder {
  bid_denom: string;
  current_order?: StaleOrder | null;
  max_bid_amount?: Uint128 | null;
  pair_address: Addr;
  side: Side;
  strategy: OrderPriceStrategy;
}
export interface StaleOrder {
  price: Decimal;
}
export interface Distribution {
  denoms: string[];
  destinations: Destination[];
}
export interface Destination {
  label?: string | null;
  recipient: Recipient;
  shares: Uint128;
}
export interface Schedule {
  action: Action;
  cadence: Cadence;
  execution_rebate: Coin[];
  scheduler: Addr;
}
export interface Duration {
  nanos: number;
  secs: number;
}
export interface Conditional {
  action: Action;
  condition: Condition;
}
export interface CompositeCondition {
  conditions: Condition[];
  threshold: Threshold;
}

export interface ManagerConfig {
  fee_collector: Addr;
  strategy_code_id: number;
}

export type ArrayOf_Trigger = Trigger[];

export interface Trigger {
  condition: Condition;
  execution_rebate: Coin[];
  id: number;
  owner: Addr;
}

export interface SchedulerInstantiateMsg {
  manager: Addr;
}
export type SchedulerQueryMsg =
  | {
      owned: {
        limit?: number | null;
        owner: Addr;
        start_after?: number | null;
      };
    }
  | {
      filtered: {
        filter: ConditionFilter;
        limit?: number | null;
      };
    }
  | {
      can_execute: number;
    };
export type ConditionFilter =
  | {
      timestamp: {
        end?: Timestamp | null;
        start?: Timestamp | null;
      };
    }
  | {
      block_height: {
        end?: number | null;
        start?: number | null;
      };
    }
  | {
      limit_order: {
        pair_address: Addr;
        /**
         * @minItems 2
         * @maxItems 2
         */
        price_range?: [Decimal, Decimal] | null;
        start_after?: number | null;
      };
    };
export type SchedulerExecuteMsg =
  | {
      create: Condition;
    }
  | {
      execute: number[];
    };
export type Boolean = boolean;

export interface Statistics {
  credited: [Recipient, Coin[]][];
  debited: Coin[];
}

export interface StrategyInstantiateMsg {
  action: Action;
  owner: Addr;
  state: Indexed;
}

export interface Indexed {
  contract_address: Addr;
}
export type StrategyQueryMsg =
  | {
      config: {};
    }
  | {
      statistics: {};
    }
  | {
      balances: string[];
    };
export type StrategyExecuteMsg =
  | ("execute" | "commit" | "clear")
  | {
      withdraw: string[];
    }
  | {
      update: StrategyFor_Indexed;
    }
  | {
      update_status: StrategyStatus;
    };

export interface StrategyFor_Indexed {
  action: Action;
  owner: Addr;
  state: Indexed;
}

export type ArrayOf_Coin = Coin[];

export interface StrategyConfig {
  escrowed: string[];
  manager: Addr;
  strategy: StrategyFor_Committed;
}
export interface StrategyFor_Committed {
  action: Action;
  owner: Addr;
  state: Committed;
}

export interface Committed {
  contract_address: Addr;
}
