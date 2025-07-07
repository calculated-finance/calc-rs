use std::collections::HashSet;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Coin, Coins, Deps, Env, Event, StdResult, SubMsg};

use crate::{
    actions::{action::Action, operation::Operation},
    cadence::Cadence,
    conditions::Threshold,
    core::Contract,
    manager::ManagerExecuteMsg,
    scheduler::{CreateTrigger, SchedulerExecuteMsg},
};

#[cw_serde]
pub struct Schedule {
    pub scheduler: Addr,
    pub cadence: Cadence,
    pub execution_rebate: Vec<Coin>,
    pub action: Box<Action>,
}

impl Operation for Schedule {
    fn init(self, _deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let set_trigger_msg = Contract(self.scheduler.clone()).call(
            to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
                condition: self.cadence.into_condition(env)?,
                threshold: Threshold::All,
                to: env.contract.address.clone(),
                msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                    contract_address: env.contract.address.clone(),
                })?,
            }]))?,
            self.execution_rebate.clone(),
        );

        let cadence = match &self.cadence {
            Cadence::Cron {
                expr,
                previous: None,
            } => Cadence::Cron {
                expr: expr.clone(),
                previous: Some(env.block.time),
            },
            _ => self.cadence.clone(),
        };

        Ok((
            Action::Schedule(Schedule { cadence, ..self }),
            vec![SubMsg::reply_never(set_trigger_msg)],
            vec![],
        ))
    }

    fn execute(self, _deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let mut messages = vec![];
        let mut events = vec![];

        if self.cadence.is_due(env)? {
            let next = self.cadence.next(env)?;

            let (action, messages_from_action, events_from_action) =
                self.action.execute(_deps, env)?;

            messages.extend(messages_from_action);
            events.extend(events_from_action);

            let set_trigger_msg = Contract(self.scheduler.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
                    condition: next.into_condition(env)?,
                    threshold: Threshold::All,
                    to: env.contract.address.clone(),
                    msg: to_json_binary(&ManagerExecuteMsg::ExecuteStrategy {
                        contract_address: env.contract.address.clone(),
                    })?,
                }]))?,
                self.execution_rebate.clone(),
            );

            messages.push(SubMsg::reply_never(set_trigger_msg));

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
            Ok((Action::Schedule(self), messages, events))
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
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::Schedule(self), vec![], vec![]))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::Schedule(self), vec![], vec![]))
    }
}
