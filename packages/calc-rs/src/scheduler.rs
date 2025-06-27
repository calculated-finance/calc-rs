use crate::types::Condition;
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Coin, Deps, Env, MessageInfo, StdResult, Timestamp};

#[cw_serde]
pub enum TriggerConditionsThreshold {
    Any,
    All,
}

#[cw_serde]
pub struct CreateTrigger {
    pub conditions: Vec<Condition>,
    pub threshold: TriggerConditionsThreshold,
    pub to: Addr,
    pub msg: Binary,
}

#[cw_serde]
pub struct Trigger {
    pub id: u64,
    pub owner: Addr,
    pub conditions: Vec<Condition>,
    pub threshold: TriggerConditionsThreshold,
    pub msg: Binary,
    pub to: Addr,
    pub execution_rebate: Vec<Coin>,
}

impl Trigger {
    pub fn from_command(info: &MessageInfo, command: CreateTrigger, rebate: Vec<Coin>) -> Self {
        Self {
            id: 0,
            owner: info.sender.clone(),
            conditions: command.conditions,
            threshold: command.threshold,
            msg: command.msg,
            to: command.to,
            execution_rebate: rebate,
        }
    }

    pub fn can_execute(&self, deps: Deps, env: &Env) -> StdResult<bool> {
        Ok(match self.threshold {
            TriggerConditionsThreshold::All => {
                self.conditions.iter().all(|c| c.check(deps, env).is_ok())
            }
            TriggerConditionsThreshold::Any => {
                self.conditions.iter().any(|c| c.check(deps, env).is_ok())
            }
        })
    }
}

#[cw_serde]
pub enum SchedulerExecuteMsg {
    CreateTrigger(CreateTrigger),
    SetTriggers(Vec<CreateTrigger>),
    ExecuteTrigger(u64),
}

#[cw_serde]
pub enum ConditionFilter {
    Owner {
        address: Addr,
    },
    Timestamp {
        start: Option<Timestamp>,
        end: Option<Timestamp>,
    },
    BlockHeight {
        start: Option<u64>,
        end: Option<u64>,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum SchedulerQueryMsg {
    #[returns(Vec<Trigger>)]
    Triggers {
        filter: ConditionFilter,
        limit: Option<usize>,
        can_execute: Option<bool>,
    },
    #[returns(bool)]
    CanExecute { id: u64 },
}
