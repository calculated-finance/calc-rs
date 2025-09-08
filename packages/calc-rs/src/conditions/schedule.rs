use std::{cmp::min, str::FromStr, time::Duration};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Coin, Coins, CosmosMsg, Deps, Env, StdResult};
use cron::Schedule as CronSchedule;

use crate::{
    cadence::Cadence,
    conditions::condition::Condition,
    core::Contract,
    manager::{Affiliate, ManagerExecuteMsg},
    operation::{Operation, StatefulOperation},
    scheduler::{CreateTriggerMsg, SchedulerExecuteMsg},
};

#[cw_serde]
pub struct Schedule {
    pub scheduler_address: Addr,
    pub manager_address: Addr,
    pub cadence: Cadence,
    pub next: Option<Cadence>,
    pub execution_rebate: Vec<Coin>,
    pub executors: Vec<Addr>,
    pub jitter: Option<Duration>,
    pub executions: Option<u32>,
}

impl Schedule {
    pub fn execute_unsafe(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Condition)> {
        let mut rebate = Coins::default();

        for amount in self.execution_rebate.iter() {
            let balance = deps
                .querier
                .query_balance(&env.contract.address, &amount.denom)?;

            rebate.add(Coin {
                denom: amount.denom.clone(),
                amount: min(amount.amount, balance.amount),
            })?;
        }

        let (condition, schedule) = if self.cadence.is_due(deps, env)? {
            let current = self.cadence.clone().crank(env)?;
            let condition = current.into_condition(env)?;
            (
                condition,
                Schedule {
                    next: Some(current),
                    ..self
                },
            )
        } else {
            let condition = self.cadence.into_condition(env)?;
            (condition, self)
        };

        let create_trigger_msg = Contract(schedule.scheduler_address.clone()).call(
            to_json_binary(&SchedulerExecuteMsg::Create(Box::new(CreateTriggerMsg {
                condition: condition.clone(),
                msg: to_json_binary(&ManagerExecuteMsg::Execute {
                    contract_address: env.contract.address.clone(),
                })?,
                contract_address: schedule.manager_address.clone(),
                executors: schedule.executors.clone(),
                jitter: schedule.jitter,
            })))?,
            rebate.to_vec(),
        );

        Ok((
            vec![create_trigger_msg],
            Condition::Schedule(Schedule {
                executions: Some(schedule.executions.unwrap_or(0) + 1),
                ..schedule
            }),
        ))
    }
}

impl Operation<Condition> for Schedule {
    fn init(self, deps: Deps, _env: &Env, _affiliates: &[Affiliate]) -> StdResult<Condition> {
        deps.api
            .addr_validate(self.manager_address.as_str())
            .map_err(|_| {
                cosmwasm_std::StdError::generic_err(format!(
                    "Invalid manager address for schedule: {}",
                    self.manager_address,
                ))
            })?;

        deps.api
            .addr_validate(self.scheduler_address.as_str())
            .map_err(|_| {
                cosmwasm_std::StdError::generic_err(format!(
                    "Invalid scheduler address for schedule: {}",
                    self.scheduler_address,
                ))
            })?;

        if self.jitter.is_some() && self.executors.is_empty() {
            return Err(cosmwasm_std::StdError::generic_err(
                "Schedule jitter is set but executors are not restricted, rendering the jitter ineffective",
            ));
        }

        if let Cadence::Cron { expr, .. } = &self.cadence {
            CronSchedule::from_str(expr).map_err(|e| {
                cosmwasm_std::StdError::generic_err(format!("Invalid cron string: {e}"))
            })?;
        }

        Ok(Condition::Schedule(self))
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Condition)> {
        self.execute_unsafe(deps, env)
    }
}

impl StatefulOperation<Condition> for Schedule {
    fn commit(self, _deps: Deps, _env: &Env) -> StdResult<Condition> {
        if let Some(next) = self.next {
            Ok(Condition::Schedule(Schedule {
                cadence: next,
                next: None,
                ..self
            }))
        } else {
            Ok(Condition::Schedule(self))
        }
    }

    fn balances(&self, _deps: Deps, _env: &Env) -> StdResult<Coins> {
        Ok(Coins::default())
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Vec<CosmosMsg>, Condition)> {
        Ok((vec![], Condition::Schedule(self)))
    }
}
