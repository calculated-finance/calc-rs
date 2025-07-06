use std::collections::HashSet;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Coin, Coins, Deps, Env, Event, StdResult, SubMsg};

use crate::{
    actions::{action::Action, operation::Operation},
    conditions::{Cadence, Threshold},
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

use cron::Schedule as CronSchedule;
use std::str::FromStr;

impl Operation for Schedule {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<Action> {
        if let Cadence::Cron(cron_str) = &self.cadence {
            if CronSchedule::from_str(cron_str).is_err() {
                return Err(cosmwasm_std::StdError::generic_err(format!(
                    "Invalid cron string: {cron_str}",
                )));
            }
        }

        Ok(Action::Schedule(self))
    }

    fn execute(self, _deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let mut messages = vec![];
        let mut events = vec![];

        if self.cadence.is_due(env) {
            let next = self.cadence.next(env);

            let (action, messages_from_action, events_from_action) =
                self.action.execute(_deps, env)?;

            messages.extend(messages_from_action);
            events.extend(events_from_action);

            let set_trigger_msg = Contract(self.scheduler.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::SetTriggers(vec![CreateTrigger {
                    condition: next.into_condition(env),
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

    fn update(
        self,
        deps: Deps,
        env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        if let Action::Schedule(update) = update {
            update.init(deps, env)?.execute(deps, env)
        } else {
            Err(cosmwasm_std::StdError::generic_err(
                "Cannot update Crank action with a different action type",
            ))
        }
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::new())
    }

    fn balances(&self, _deps: Deps, _env: &Env, _denoms: &[String]) -> StdResult<Coins> {
        Ok(Coins::default())
    }

    fn withdraw(
        &self,
        _deps: Deps,
        _env: &Env,
        _desired: &Coins,
    ) -> StdResult<(Vec<SubMsg>, Coins)> {
        Ok((vec![], Coins::default()))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        Ok((Action::Schedule(self), vec![], vec![]))
    }
}
