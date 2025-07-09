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
        owner: Addr;
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
      fin_swap: FinSwap;
    }
  | {
      thor_swap: ThorSwap;
    }
  | {
      optimal_swap: OptimalSwap;
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
      fin: Addr;
    }
  | {
      thorchain: {
        affiliate_bps?: number | null;
        affiliate_code?: string | null;
        max_streaming_quantity?: number | null;
        previous_swap?: StreamingSwap | null;
        streaming_interval?: number | null;
      };
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
export type Direction = "up" | "down";
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
      wasm: {
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
      can_swap: {
        minimum_receive_amount: Coin;
        route: SwapRoute;
        swap_amount: Coin;
      };
    }
  | {
      limit_order_filled: {
        owner: Addr;
        pair_address: Addr;
        price: Price;
        rate: Decimal;
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
      not: Condition;
    };
export type Price =
  | {
      fixed: Decimal;
    }
  | {
      oracle: number;
    };
export type Threshold = "all" | "any";
export type Json = null;

export interface StrategyFor_Json {
  action: Action;
  owner: Addr;
  state: Json;
}
export interface FinSwap {
  adjustment: SwapAmountAdjustment;
  maximum_slippage_bps: number;
  minimum_receive_amount: Coin;
  pair_address: Addr;
  swap_amount: Coin;
}
export interface Coin {
  amount: Uint128;
  denom: string;
}
export interface ThorSwap {
  adjustment: SwapAmountAdjustment;
  affiliate_bps?: number | null;
  affiliate_code?: string | null;
  max_streaming_quantity?: number | null;
  maximum_slippage_bps: number;
  minimum_receive_amount: Coin;
  previous_swap?: StreamingSwap | null;
  streaming_interval?: number | null;
  swap_amount: Coin;
}
export interface StreamingSwap {
  expected_receive_amount: Coin;
  starting_block: number;
  streaming_swap_blocks: number;
  swap_amount: Coin;
}
export interface OptimalSwap {
  adjustment: SwapAmountAdjustment;
  maximum_slippage_bps: number;
  minimum_receive_amount: Coin;
  routes: SwapRoute[];
  swap_amount: Coin;
}
export interface LimitOrder {
  bid_denom: string;
  current_order?: StaleOrder | null;
  execution_rebate: Coin[];
  max_bid_amount?: Uint128 | null;
  pair_address: Addr;
  scheduler: Addr;
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
  conditions: Condition[];
  threshold: Threshold;
}

export type ArrayOf_Affiliate = Affiliate[];

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
      can_execute: {
        id: number;
      };
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
export type DcaSchedule =
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
    };

export interface DcaStrategy {
  conditions: Condition[];
  exchange_contract: Addr;
  fee_collector: Addr;
  immutable_destinations: Destination[];
  minimum_receive_amount: Coin;
  mutable_destinations: Destination[];
  owner: Addr;
  schedule: DcaSchedule;
  scheduler_contract: Addr;
  statistics: DcaStatistics;
  swap_amount: Coin;
}

export interface DcaStatistics {
  amount_deposited: Coin;
  amount_received: Coin;
  amount_swapped: Coin;
}
export interface NewStrategy {
  owner: Addr;
}

export type Route =
  | {
      fin: {
        address: Addr;
      };
    }
  | {
      thorchain: {};
    };
export type TriggerConditionsThreshold = "any" | "all";

export interface Statistics {
  distributed: [Recipient, Coin[]][];
  filled: Coin[];
  swapped: Coin[];
  withdrawn: Coin[];
}

export interface StrategyInstantiateMsg {
  action: Action;
  owner: Addr;
  state: Instantiable;
}

export interface Instantiable {
  code_id: number;
  contract_address: Addr;
  label: string;
  salt: Binary;
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
      update: StrategyFor_Instantiable;
    }
  | {
      update_status: StrategyStatus;
    };

export interface StrategyFor_Instantiable {
  action: Action;
  owner: Addr;
  state: Instantiable;
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
