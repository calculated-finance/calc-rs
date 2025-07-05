export type Uint128 = string;

export interface ExpectedReceiveAmount {
  receive_amount: Coin;
  slippage_bps: number;
}
export interface Coin {
  amount: Uint128;
  denom: string;
}
/**
 * A human readable address.
 *
 * In Cosmos, this is typically bech32 encoded. But for multi-chain smart contracts no assumptions should be made other than being UTF-8 encoded and of reasonable length.
 *
 * This type represents a validated address. It can be created in the following ways 1. Use `Addr::unchecked(input)` 2. Use `let checked: Addr = deps.api.addr_validate(input)?` 3. Use `let checked: Addr = deps.api.addr_humanize(canonical_addr)?` 4. Deserialize from JSON. This must only be done from JSON that was validated before such as a contract's state. `Addr` must not be used in messages sent by the user because this would result in unvalidated instances.
 *
 * This type is immutable. If you really need to mutate it (Really? Are you sure?), create a mutable copy using `let mut mutable = Addr::to_string()` and operate on that `String` instance.
 */
export type Addr = string;

export interface ExchangerInstantiateMsg {
  affiliate_bps?: number | null;
  affiliate_code?: string | null;
  scheduler_address: Addr;
}
export type ExchangerQueryMsg = {
  expected_receive_amount: {
    route?: Route | null;
    swap_amount: Coin;
    target_denom: string;
  };
};
export type Route =
  | {
      fin_market: {
        address: Addr;
      };
    }
  | {
      thorchain: {};
    };
export type ExchangerExecuteMsg = {
  swap: {
    maximum_slippage_bps: number;
    minimum_receive_amount: Coin;
    on_complete?: Callback | null;
    recipient?: Addr | null;
    route?: Route | null;
  };
};
/**
 * Binary is a wrapper around Vec<u8> to add base64 de/serialization with serde. It also adds some helper methods to help encode inline.
 *
 * This is only needed as serde-json-{core,wasm} has a horrible encoding for Vec<u8>. See also <https://github.com/CosmWasm/cosmwasm/blob/main/docs/MESSAGE_TYPES.md>.
 */
export type Binary = string;

export interface Callback {
  contract: Addr;
  execution_rebate: Coin[];
  msg: Binary;
}
export type Boolean = boolean;
export type Condition =
  | {
      timestamp: {
        timestamp: Timestamp;
      };
    }
  | {
      block_height: {
        height: number;
      };
    }
  | {
      limit_order: {
        minimum_receive_amount: Coin;
        swap_amount: Coin;
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

export interface Destination {
  address: Addr;
  label?: string | null;
  shares: Uint128;
}
export interface Duration {
  nanos: number;
  secs: number;
}
export interface DcaStatistics {
  amount_deposited: Coin;
  amount_received: Coin;
  amount_swapped: Coin;
}
export interface NewStrategy {
  owner: Addr;
}

export type StrategyStatus = "active" | "paused" | "archived";
export type ArrayOf_Strategy = Strategy[];

export interface Strategy {
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
        action: Action;
        affiliates: Affiliate[];
        label: string;
        owner: Addr;
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
        update: StrategyConfig;
      };
    };
export type Action =
  | {
      check: Condition;
    }
  | {
      crank: Schedule;
    }
  | {
      perform: Swap;
    }
  | {
      set: Order;
    }
  | {
      distribute_to: Recipients;
    }
  | {
      exhibit: Behaviour;
    };
export type Price =
  | {
      fixed: Decimal;
    }
  | {
      oracle: number;
    };
/**
 * A fixed-point decimal value with 18 fractional digits, i.e. Decimal(1_000_000_000_000_000_000) == 1.0
 *
 * The greatest possible value that can be represented is 340282366920938463463.374607431768211455 (which is (2^128 - 1) / 10^18)
 */
export type Decimal = string;
export type Side = "base" | "quote";
export type Threshold = "all" | "any";
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
      cron: string;
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
export type OrderPriceStrategy =
  | {
      fixed: {
        price: Decimal;
      };
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
      bps: number;
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
    };

export interface Schedule {
  cadence: Cadence;
  execution_rebate: Coin[];
  scheduler: Addr;
}

export interface Swap {
  adjustment: SwapAmountAdjustment;
  exchange_contract: Addr;
  maximum_slippage_bps: number;
  minimum_receive_amount: Coin;
  route?: Route | null;
  swap_amount: Coin;
}
export interface Order {
  bid_amount?: Uint128 | null;
  bid_denom: string;
  current_price?: Price | null;
  pair_address: Addr;
  side: Side;
  strategy: OrderPriceStrategy;
}
export interface Recipients {
  denoms: string[];
  immutable_destinations: Destination[];
  mutable_destinations: Destination[];
}

export interface Behaviour {
  actions: Action[];
  threshold: Threshold;
}

export interface StrategyConfig {
  action: Action;
  escrowed: string[];
  manager: Addr;
  owner: Addr;
}

export type ArrayOf_Affiliate = Affiliate[];

export interface ManagerConfig {
  fee_collector: Addr;
  strategy_code_id: number;
}

export type ArrayOf_Trigger = Trigger[];

export interface Trigger {
  conditions: Condition[];
  execution_rebate: Coin[];
  id: number;
  msg: Binary;
  owner: Addr;
  threshold: Threshold;
  to: Addr;
}

export interface SchedulerInstantiateMsg {}
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
        start_after?: number | null;
      };
    };
export type SchedulerExecuteMsg =
  | {
      create_trigger: CreateTrigger;
    }
  | {
      set_triggers: CreateTrigger[];
    }
  | {
      execute_trigger: number;
    };

export interface CreateTrigger {
  conditions: Condition[];
  msg: Binary;
  threshold: Threshold;
  to: Addr;
}

export type TriggerConditionsThreshold = "any" | "all";

export interface Statistics {
  distributed: [Recipient, Coin[]][];
  filled: Coin[];
  swapped: Coin[];
  withdrawn: Coin[];
}

export interface StrategyInstantiateMsg {
  action: Action;
  affiliates: Affiliate[];
  owner: Addr;
}

export type StrategyQueryMsg =
  | {
      config: {};
    }
  | {
      statistics: {};
    }
  | {
      balances: {
        include: string[];
      };
    };
export type StrategyExecuteMsg =
  | {
      execute: {};
    }
  | {
      withdraw: Coin[];
    }
  | {
      update: StrategyConfig;
    }
  | {
      update_status: StrategyStatus;
    }
  | {
      clear: {};
    };
export type ArrayOf_Coin = Coin[];
