use std::{cmp::min, collections::HashSet, str::FromStr, time::Duration};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Binary, Coin, Coins, Deps, Env, Event, StdResult};
use cron::Schedule as CronSchedule;

use crate::{
    cadence::Cadence,
    conditions::condition::Condition,
    core::Contract,
    manager::{Affiliate, ManagerExecuteMsg},
    operation::Operation,
    scheduler::{CreateTriggerMsg, SchedulerExecuteMsg},
    strategy::{StrategyMsg, StrategyMsgPayload},
};

enum ScheduleEvent {
    ExecutionSkipped { reason: String },
    CreateTrigger { condition: Condition },
}

impl From<ScheduleEvent> for Event {
    fn from(val: ScheduleEvent) -> Self {
        match val {
            ScheduleEvent::ExecutionSkipped { reason } => {
                Event::new("skip_schedule").add_attribute("reason", reason)
            }
            ScheduleEvent::CreateTrigger { condition } => Event::new("create_trigger")
                .add_attribute(
                    "condition",
                    to_json_binary(&condition)
                        .unwrap_or(Binary::default())
                        .to_string(),
                ),
        }
    }
}

#[cw_serde]
pub struct Schedule {
    pub scheduler: Addr,
    pub contract_address: Addr,
    pub msg: Option<Binary>,
    pub cadence: Cadence,
    pub execution_rebate: Vec<Coin>,
    pub executors: Vec<Addr>,
    pub jitter: Option<Duration>,
}

impl Schedule {
    pub fn execute_unsafe(
        self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Condition)> {
        let mut rebate = Coins::default();

        for amount in self.execution_rebate.iter() {
            let balance = deps
                .querier
                .query_balance(env.contract.address.clone(), amount.denom.clone())?;

            rebate.add(Coin {
                denom: amount.denom.clone(),
                amount: min(amount.amount, balance.amount),
            })?;
        }

        if self.cadence.is_due(deps, env, &self.scheduler)? {
            let condition = self.cadence.into_condition(deps, env, &self.scheduler)?;

            let create_trigger_msg = Contract(self.scheduler.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::Create(CreateTriggerMsg {
                    condition: condition.clone(),
                    msg: self
                        .msg
                        .clone()
                        .unwrap_or(to_json_binary(&ManagerExecuteMsg::Execute {
                            contract_address: env.contract.address.clone(),
                        })?),
                    contract_address: self.contract_address.clone(),
                    executors: self.executors.clone(),
                    jitter: self.jitter,
                }))?,
                rebate.to_vec(),
            );

            Ok((
                vec![StrategyMsg::with_payload(
                    create_trigger_msg,
                    StrategyMsgPayload {
                        events: vec![ScheduleEvent::CreateTrigger {
                            condition: condition.clone(),
                        }
                        .into()],
                        ..StrategyMsgPayload::default()
                    },
                )],
                vec![ScheduleEvent::CreateTrigger {
                    condition: condition.clone(),
                }
                .into()],
                Condition::Schedule(Schedule {
                    cadence: self.cadence.next(deps, env)?,
                    ..self
                }),
            ))
        } else {
            let condition = self.cadence.into_condition(deps, env, &self.scheduler)?;

            let create_trigger_msg = Contract(self.scheduler.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::Create(CreateTriggerMsg {
                    condition: condition.clone(),
                    msg: self
                        .msg
                        .clone()
                        .unwrap_or(to_json_binary(&ManagerExecuteMsg::Execute {
                            contract_address: env.contract.address.clone(),
                        })?),
                    contract_address: self.contract_address.clone(),
                    executors: self.executors.clone(),
                    jitter: self.jitter,
                }))?,
                rebate.to_vec(),
            );

            let skipped_event = ScheduleEvent::ExecutionSkipped {
                reason: format!("Schedule not due: {:?}", self.cadence.clone()),
            };

            let trigger_created_event = ScheduleEvent::CreateTrigger {
                condition: condition.clone(),
            };

            Ok((
                vec![StrategyMsg::with_payload(
                    create_trigger_msg,
                    StrategyMsgPayload {
                        events: vec![ScheduleEvent::CreateTrigger { condition }.into()],
                        ..StrategyMsgPayload::default()
                    },
                )],
                vec![skipped_event.into(), trigger_created_event.into()],
                Condition::Schedule(self),
            ))
        }
    }
}

impl Operation<Condition> for Schedule {
    fn init(self, _deps: Deps, _env: &Env, _affiliates: &[Affiliate]) -> StdResult<Condition> {
        if let Cadence::Cron { expr, .. } = self.cadence.clone() {
            CronSchedule::from_str(&expr).map_err(|e| {
                cosmwasm_std::StdError::generic_err(format!("Invalid cron string: {e}"))
            })?;
        }

        Ok(Condition::Schedule(self))
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Condition) {
        match self.clone().execute_unsafe(deps, env) {
            Ok((messages, events, schedule)) => (messages, events, schedule),
            Err(err) => (
                vec![],
                vec![ScheduleEvent::ExecutionSkipped {
                    reason: err.to_string(),
                }
                .into()],
                Condition::Schedule(self),
            ),
        }
    }

    fn denoms(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(self
            .execution_rebate
            .iter()
            .map(|coin| coin.denom.clone())
            .collect())
    }
}
