use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Coin, MessageInfo, Timestamp};

use crate::conditions::{Condition, Threshold};

#[cw_serde]
pub struct CreateTrigger {
    pub condition: Condition,
    pub threshold: Threshold,
    pub to: Addr,
    pub msg: Binary,
}

#[cw_serde]
pub struct Trigger {
    pub id: u64,
    pub owner: Addr,
    pub condition: Condition,
    pub threshold: Threshold,
    pub msg: Binary,
    pub to: Addr,
    pub execution_rebate: Vec<Coin>,
}

impl Trigger {
    pub fn from_command(info: &MessageInfo, command: CreateTrigger, rebate: Vec<Coin>) -> Self {
        Self {
            id: 0, // This will be set later when the trigger is stored
            owner: info.sender.clone(),
            condition: command.condition,
            threshold: command.threshold,
            msg: command.msg,
            to: command.to,
            execution_rebate: rebate,
        }
    }
}

#[cw_serde]
pub struct SchedulerInstantiateMsg {}

#[cw_serde]
pub enum SchedulerExecuteMsg {
    CreateTrigger(CreateTrigger),
    SetTriggers(Vec<CreateTrigger>),
    ExecuteTrigger(u64),
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
    CanExecute { id: u64 },
}
