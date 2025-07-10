use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Coin, Decimal, Timestamp};

use crate::conditions::Condition;

#[cw_serde]
pub struct Trigger {
    pub id: u64,
    pub owner: Addr,
    pub condition: Condition,
    pub execution_rebate: Vec<Coin>,
}

#[cw_serde]
pub struct SchedulerInstantiateMsg {
    pub manager: Addr,
}

#[cw_serde]
pub enum SchedulerExecuteMsg {
    Create(Condition),
    Execute(Vec<u64>),
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
    Owned {
        owner: Addr,
        limit: Option<usize>,
        start_after: Option<u64>,
    },
    #[returns(Vec<Trigger>)]
    Filtered {
        filter: ConditionFilter,
        limit: Option<usize>,
    },
    #[returns(bool)]
    CanExecute(u64),
}
