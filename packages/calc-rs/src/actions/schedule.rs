use std::collections::HashSet;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Coin, Coins, Deps, Env, Event, StdResult};

use crate::{
    actions::{action::Action, operation::Operation},
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

impl Into<Event> for ScheduleEvent {
    fn into(self) -> Event {
        match self {
            ScheduleEvent::ExecutionSkipped { reason } => {
                Event::new("schedule_skipped").add_attribute("reason", reason)
            }
            ScheduleEvent::CreateTrigger { condition } => {
                Event::new("trigger_created").add_attribute("condition", format!("{:?}", condition))
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

impl Operation for Schedule {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        Ok((Action::Schedule(self), vec![], vec![]))
    }

    fn execute(self, _deps: Deps, env: &Env) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        if self.cadence.is_due(env)? {
            let (action, mut messages, events) = self.action.execute(_deps, env)?;
            let next = self.cadence.next(env)?;
            let condition = next.into_condition(env)?;

            let create_trigger_msg = Contract(self.scheduler.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::Create(condition.clone()))?,
                self.execution_rebate.clone(),
            );

            messages.push(StrategyMsg::with_payload(
                create_trigger_msg,
                StrategyMsgPayload {
                    events: vec![ScheduleEvent::CreateTrigger { condition }.into()],
                    ..StrategyMsgPayload::default()
                },
            ));

            Ok((
                Action::Schedule(Schedule {
                    cadence: next,
                    action: Box::new(action),
                    ..self
                }),
                messages,
                events,
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

            Ok((
                Action::Schedule(self),
                vec![StrategyMsg::with_payload(
                    create_trigger_msg,
                    StrategyMsgPayload {
                        events: vec![ScheduleEvent::CreateTrigger { condition }.into()],
                        ..StrategyMsgPayload::default()
                    },
                )],
                vec![skipped_event.into()],
            ))
        }
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::new())
    }

    fn balances(&self, _deps: Deps, _env: &Env, _denoms: &HashSet<String>) -> StdResult<Coins> {
        Ok(Coins::default())
    }

    fn withdraw(
        self,
        _deps: Deps,
        _env: &Env,
        _desired: &HashSet<String>,
    ) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        Ok((Action::Schedule(self), vec![], vec![]))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        Ok((Action::Schedule(self), vec![], vec![]))
    }
}
