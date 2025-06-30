export type Uint128 = string;

export interface DistributorStatistics {
  distributed: {};
  withdrawn: Coin[];
}
export interface Coin {
  amount: Uint128;
  denom: string;
}
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
/**
 * Binary is a wrapper around Vec<u8> to add base64 de/serialization with serde. It also adds some helper methods to help encode inline.
 *
 * This is only needed as serde-json-{core,wasm} has a horrible encoding for Vec<u8>. See also <https://github.com/CosmWasm/cosmwasm/blob/main/docs/MESSAGE_TYPES.md>.
 */
export type Binary = string;

export interface DistributorInstantiateMsg {
  denoms: string[];
  immutable_destinations: Destination[];
  mutable_destinations: Destination[];
  owner: Addr;
}
export interface Destination {
  label?: string | null;
  recipient: Recipient;
  shares: Uint128;
}
export type DistributorQueryMsg =
  | {
      config: {};
    }
  | {
      statistics: {};
    };
export type DistributorExecuteMsg =
  | {
      distribute: {};
    }
  | {
      withdraw: {
        amounts: Coin[];
      };
    }
  | {
      update: DistributorConfig;
    };
export type Condition =
  | {
      timestamp_elapsed: Timestamp;
    }
  | {
      blocks_completed: number;
    }
  | {
      exchange_liquidity_provided: {
        exchanger_contract: Addr;
        maximum_slippage_bps: number;
        minimum_receive_amount: Coin;
        route?: Route | null;
        swap_amount: Coin;
      };
    }
  | {
      balance_available: {
        address: Addr;
        amount: Coin;
      };
    }
  | {
      strategy_status: {
        contract_address: Addr;
        manager_contract: Addr;
        status: StrategyStatus;
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
export type Route =
  | {
      fin: {
        address: Addr;
      };
    }
  | {
      thorchain: {};
    };
export type StrategyStatus = "active" | "paused" | "archived";

export interface DistributorConfig {
  conditions: Condition[];
  denoms: string[];
  immutable_destinations: Destination[];
  mutable_destinations: Destination[];
  owner: Addr;
}

export interface ExpectedReceiveAmount {
  receive_amount: Coin;
  slippage_bps: number;
}

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
export type ExchangerExecuteMsg = {
  swap: {
    maximum_slippage_bps: number;
    minimum_receive_amount: Coin;
    on_complete?: Callback | null;
    recipient?: Addr | null;
    route?: Route | null;
  };
};

export interface Callback {
  contract: Addr;
  execution_rebate: Coin[];
  msg: Binary;
}
export type Boolean = boolean;
export type StrategyConfig =
  | {
      dca: DcaStrategy;
    }
  | {
      new: NewStrategy;
    };
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

export type ArrayOf_Strategy = Strategy[];

export interface Strategy {
  affiliates: Affiliate[];
  contract_address: Addr;
  created_at: number;
  label: string;
  owner: Addr;
  status: StrategyStatus;
  updated_at: number;
}
export interface Affiliate {
  address: Addr;
  bps: number;
  code: string;
}

export interface ManagerInstantiateMsg {
  admin: Addr;
  affiliate_creation_fee: Coin;
  code_ids: {};
  default_affiliate_bps: number;
  fee_collector: Addr;
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
        start_after?: Addr | null;
        status?: StrategyStatus | null;
      };
    }
  | {
      affiliate: {
        code: string;
      };
    }
  | {
      affiliates: {
        limit?: number | null;
        start_after?: Addr | null;
      };
    };
export type ManagerExecuteMsg =
  | {
      instantiate_strategy: {
        label: string;
        owner: Addr;
        strategy: CreateStrategyConfig;
      };
    }
  | {
      execute_strategy: {
        contract_address: Addr;
        msg?: Binary | null;
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
    }
  | {
      add_affiliate: {
        address: Addr;
        code: string;
      };
    };
export type CreateStrategyConfig = {
  twap: InstantiateTwapCommand;
};
export type Schedule =
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

export interface InstantiateTwapCommand {
  affiliate_code?: string | null;
  distributor_code_id: number;
  exchanger_contract: Addr;
  execution_rebate?: Coin | null;
  immutable_destinations: Destination[];
  maximum_slippage_bps: number;
  minimum_distribute_amount?: Coin | null;
  minimum_receive_amount: Coin;
  mutable_destinations: Destination[];
  owner: Addr;
  route?: Route | null;
  scheduler_contract: Addr;
  swap_amount: Coin;
  swap_cadence: Schedule;
}

export interface TwapConfig {
  distributor_contract: Addr;
  exchanger_contract: Addr;
  execution_rebate?: Coin | null;
  manager_contract: Addr;
  maximum_slippage_bps: number;
  minimum_receive_amount: Coin;
  owner: Addr;
  route?: Route | null;
  schedule_conditions: Condition[];
  scheduler_contract: Addr;
  swap_amount: Coin;
  swap_cadence: Schedule;
  swap_conditions: Condition[];
}

export type ArrayOf_Affiliate = Affiliate[];

export interface ManagerConfig {
  admin: Addr;
  affiliate_creation_fee: Coin;
  code_ids: {};
  default_affiliate_bps: number;
  fee_collector: Addr;
}

export interface SchedulerInstantiateMsg {}
export type SchedulerQueryMsg =
  | {
      triggers: {
        can_execute?: boolean | null;
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
      owner: {
        address: Addr;
      };
    }
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
export type TriggerConditionsThreshold = "any" | "all";

export interface CreateTrigger {
  conditions: Condition[];
  msg: Binary;
  threshold: TriggerConditionsThreshold;
  to: Addr;
}

export type ArrayOf_Trigger = Trigger[];

export interface Trigger {
  conditions: Condition[];
  execution_rebate: Coin[];
  id: number;
  msg: Binary;
  owner: Addr;
  threshold: TriggerConditionsThreshold;
  to: Addr;
}

export type StrategyStatistics = {
  twap: {
    distributed: {};
    received: Coin;
    remaining: Coin;
    swapped: Coin;
    withdrawn: Coin[];
  };
};

export interface TwapInstantiateMsg {
  config: CreateStrategyConfig;
  fee_collector: Addr;
}

export type TwapQueryMsg =
  | {
      config: {};
    }
  | {
      statistics: {};
    };
export type TwapExecuteMsg =
  | {
      execute: {};
    }
  | {
      withdraw: {
        amounts: Coin[];
      };
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
