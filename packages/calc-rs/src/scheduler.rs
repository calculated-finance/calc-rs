use std::{
    hash::{DefaultHasher, Hash, Hasher},
    time::Duration,
};

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Coin, Decimal, StdError, StdResult, Timestamp, Uint64};

use crate::conditions::condition::Condition;

#[cw_serde]
pub struct Trigger {
    pub id: Uint64,
    pub owner: Addr,
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
    pub fn id(&self, owner: &Addr) -> StdResult<Uint64> {
        let mut hasher = DefaultHasher::new();

        hasher.write(owner.as_bytes());

        match &self.condition {
            Condition::TimestampElapsed(timestamp) => {
                0u8.hash(&mut hasher);
                timestamp.seconds().hash(&mut hasher);
            }
            Condition::BlocksCompleted(height) => {
                1u8.hash(&mut hasher);
                height.hash(&mut hasher);
            }
            Condition::FinLimitOrderFilled {
                owner,
                pair_address,
                side,
                price,
            } => {
                2u8.hash(&mut hasher);
                owner
                    .as_ref()
                    .unwrap_or(&Addr::unchecked(""))
                    .hash(&mut hasher);
                pair_address.hash(&mut hasher);
                side.to_string().hash(&mut hasher);
                price.to_string().hash(&mut hasher);
            }
            _ => Err(StdError::generic_err(format!(
                "ID generation for condition {:?} not supported",
                self.condition
            )))?,
        };

        hasher.write(&self.msg);
        hasher.write(self.contract_address.as_bytes());

        Ok(hasher.finish().into())
    }
}

#[cw_serde]
pub enum SchedulerExecuteMsg {
    Create(Box<CreateTriggerMsg>),
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
