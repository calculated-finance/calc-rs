use std::{
    hash::{DefaultHasher, Hasher},
    time::Duration,
};

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Coin, Decimal, StdResult, Timestamp, Uint64};

use crate::conditions::condition::Condition;

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
        let mut hash = DefaultHasher::new();

        hash.write(&self.condition.id()?.to_le_bytes());
        hash.write(&self.msg);
        hash.write(self.contract_address.as_bytes());

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
