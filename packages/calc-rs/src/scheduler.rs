use std::{
    hash::{DefaultHasher, Hasher},
    time::Duration,
};

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{to_json_binary, Addr, Binary, Coin, Decimal, StdResult, Timestamp, Uint64};

use crate::condition::Condition;

#[cw_serde]
pub struct Trigger {
    pub id: Uint64,
    pub condition: Condition,
    pub msg: Binary,
    pub contract_address: Addr,
    pub executors: Vec<Addr>,
    pub execution_rebate: Vec<Coin>,
    pub jitter: Option<Duration>,
}

#[cw_serde]
pub struct SchedulerInstantiateMsg {}

#[cw_serde]
pub struct CreateTriggerMsg {
    pub condition: Condition,
    pub msg: Binary,
    pub contract_address: Addr,
    pub executors: Vec<Addr>,
    pub jitter: Option<Duration>,
}

impl CreateTriggerMsg {
    pub fn id(&self) -> StdResult<Uint64> {
        let salt_data = to_json_binary(&self)?;
        let mut hash = DefaultHasher::new();
        hash.write(salt_data.as_slice());
        Ok(hash.finish().into())
    }
}

#[cw_serde]
pub enum SchedulerExecuteMsg {
    Create(CreateTriggerMsg),
    Execute(Vec<Uint64>),
}

#[cw_serde]
pub enum ConditionFilter {
    Timestamp {
        start: Option<Timestamp>,
        end: Option<Timestamp>,
    },
    BlockHeight {
        start: Option<u64>,
        end: Option<u64>,
    },
    LimitOrder {
        pair_address: Addr,
        price_range: Option<(Decimal, Decimal)>,
        start_after: Option<u64>,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum SchedulerQueryMsg {
    #[returns(Vec<Trigger>)]
    Filtered {
        filter: ConditionFilter,
        limit: Option<usize>,
    },
    #[returns(bool)]
    CanExecute(Uint64),
}
