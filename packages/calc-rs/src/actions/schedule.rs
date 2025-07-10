use std::{collections::HashSet, str::FromStr};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Coin, Deps, Env, Event, StdResult};
use cron::Schedule as CronSchedule;

use crate::{
    actions::{action::Action, operation::StatelessOperation},
    cadence::Cadence,
    conditions::Condition,
    core::Contract,
    scheduler::SchedulerExecuteMsg,
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
                Event::new("schedule_skipped").add_attribute("reason", reason)
            }
            ScheduleEvent::CreateTrigger { condition } => {
                Event::new("trigger_created").add_attribute("condition", format!("{condition:?}"))
            }
        }
    }
}

#[cw_serde]
pub struct Schedule {
    pub scheduler: Addr,
    pub cadence: Cadence,
    pub execution_rebate: Vec<Coin>,
    pub action: Box<Action>,
}

impl Schedule {
    pub fn execute_unsafe(
        self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if self.cadence.is_due(env)? {
            let (mut messages, mut events, action) = self.action.execute(deps, env);

            let condition = self.cadence.into_condition(env)?;

            let create_trigger_msg = Contract(self.scheduler.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::Create(condition.clone()))?,
                self.execution_rebate.clone(),
            );

            messages.push(StrategyMsg::with_payload(
                create_trigger_msg,
                StrategyMsgPayload {
                    events: vec![ScheduleEvent::CreateTrigger {
                        condition: condition.clone(),
                    }
                    .into()],
                    ..StrategyMsgPayload::default()
                },
            ));

            events.push(ScheduleEvent::CreateTrigger { condition }.into());

            Ok((
                messages,
                events,
                Action::Schedule(Schedule {
                    cadence: self.cadence.next(env)?,
                    action: Box::new(action),
                    ..self
                }),
            ))
        } else {
            let condition = self.cadence.into_condition(env)?;

            let create_trigger_msg = Contract(self.scheduler.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::Create(condition.clone()))?,
                self.execution_rebate.clone(),
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
                Action::Schedule(self),
            ))
        }
    }
}

impl StatelessOperation for Schedule {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if let Cadence::Cron { expr, .. } = self.cadence.clone() {
            CronSchedule::from_str(&expr).map_err(|e| {
                cosmwasm_std::StdError::generic_err(format!("Invalid cron string: {e}"))
            })?;
        }

        Ok((vec![], vec![], Action::Schedule(self)))
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        match self.clone().execute_unsafe(deps, env) {
            Ok((messages, events, action)) => (messages, events, action),
            Err(err) => (
                vec![],
                vec![ScheduleEvent::ExecutionSkipped {
                    reason: err.to_string(),
                }
                .into()],
                Action::Schedule(self),
            ),
        }
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::new())
    }
}
