export type Addr = string;
export type StrategyStatus = "active" | "paused";
export type ArrayOf_Strategy = Strategy[];

export interface Strategy {
  contract_address: Addr;
  created_at: number;
  id: number;
  label: string;
  owner: Addr;
  source?: string | null;
  status: StrategyStatus;
  updated_at: number;
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
    }
  | {
      count: {};
    };
export type Uint64 = number;
export type ManagerExecuteMsg =
  | {
      instantiate: {
        affiliates: Affiliate[];
        label: string;
        nodes: Node[];
        owner?: Addr | null;
        source?: string | null;
      };
    }
  | {
      execute: {
        contract_address: Addr;
      };
    }
  | {
      update_status: {
        contract_address: Addr;
        status: StrategyStatus;
      };
    }
  | {
      update: {
        contract_address: Addr;
        nodes: Node[];
      };
    }
  | {
      update_label: {
        contract_address: Addr;
        label: string;
      };
    };
export type Node =
  | {
      action: {
        action: Action;
        index: number;
        next?: number | null;
      };
    }
  | {
      condition: {
        condition: Condition;
        index: number;
        on_failure?: number | null;
        on_success?: number | null;
      };
    };
export type Action =
  | {
      swap: Swap;
    }
  | {
      limit_order: FinLimitOrder;
    }
  | {
      distribute: Distribution;
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
export type PriceStrategy =
  | {
      fixed: Decimal;
    }
  | {
      offset: {
        direction: Direction;
        offset: Offset;
        side: Side;
        tolerance?: Offset | null;
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
    };
/**
 * Binary is a wrapper around Vec<u8> to add base64 de/serialization with serde. It also adds some helper methods to help encode inline.
 *
 * This is only needed as serde-json-{core,wasm} has a horrible encoding for Vec<u8>. See also <https://github.com/CosmWasm/cosmwasm/blob/main/docs/MESSAGE_TYPES.md>.
 */
export type Binary = string;
export type Condition =
  | {
      timestamp_elapsed: Timestamp;
    }
  | {
      blocks_completed: number;
    }
  | {
      schedule: Schedule;
    }
  | {
      can_swap: Swap;
    }
  | {
      fin_limit_order_filled: {
        owner?: Addr | null;
        pair_address: Addr;
        price: Decimal;
        side: Side;
      };
    }
  | {
      balance_available: {
        address?: Addr | null;
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
        price: Decimal;
      };
    }
  | {
      asset_value_ratio: AssetValueRatio;
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
        strategy: PriceStrategy;
      };
    };
export type PriceSource =
  | "thorchain"
  | {
      fin: {
        address: Addr;
      };
    };

export interface Affiliate {
  address: Addr;
  bps: number;
  label: string;
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
export interface FinLimitOrder {
  bid_amount?: Uint128 | null;
  bid_denom: string;
  current_order?: StaleOrder | null;
  min_fill_ratio?: Decimal | null;
  pair_address: Addr;
  side: Side;
  strategy: PriceStrategy;
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
  cadence: Cadence;
  execution_rebate: Coin[];
  executors: Addr[];
  jitter?: Duration | null;
  manager_address: Addr;
  next?: Cadence | null;
  scheduler_address: Addr;
}
export interface Duration {
  nanos: number;
  secs: number;
}
export interface AssetValueRatio {
  denominator: string;
  numerator: string;
  oracle: PriceSource;
  ratio: Decimal;
  tolerance: Decimal;
}

export interface ManagerConfig {
  fee_collector: Addr;
  strategy_code_id: number;
}

export type ArrayOf_Trigger = Trigger[];

export interface Trigger {
  condition: Condition;
  contract_address: Addr;
  execution_rebate: Coin[];
  executors: Addr[];
  id: Uint64;
  jitter?: Duration | null;
  msg: Binary;
  owner: Addr;
}

export interface SchedulerInstantiateMsg {}
export type SchedulerQueryMsg =
  | {
      filtered: {
        filter: ConditionFilter;
        limit?: number | null;
      };
    }
  | {
      can_execute: Uint64;
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
      };
    };
export type Threshold = "all" | "any";

export interface CompositeCondition {
  conditions: Condition[];
  threshold: Threshold;
}
export type SchedulerExecuteMsg =
  | {
      create: CreateTriggerMsg;
    }
  | {
      execute: Uint64[];
    };

export interface CreateTriggerMsg {
  condition: Condition;
  contract_address: Addr;
  executors: Addr[];
  jitter?: Duration | null;
  msg: Binary;
}

export type Boolean = boolean;

export interface Statistics {
  credited: [Recipient, Coin[]][];
  debited: Coin[];
}

export interface StrategyInstantiateMsg {
  affiliates: Affiliate[];
  contract_address: Addr;
  nodes: Node[];
  owner: Addr;
}

export type StrategyQueryMsg =
  | {
      config: {};
    }
  | {
      balances: {};
    };
export type StrategyExecuteMsg =
  | {
      init: Node[];
    }
  | {
      execute: {};
    }
  | {
      withdraw: Coin[];
    }
  | {
      update: Node[];
    }
  | {
      cancel: {};
    }
  | {
      process: {
        operation: StrategyOperation;
        previous?: number | null;
      };
    };
export type StrategyOperation = "execute" | "cancel";
export type ArrayOf_Coin = Coin[];

export interface StrategyConfig {
  manager: Addr;
  nodes: Node[];
  owner: Addr;
}
