use std::{cmp::min, collections::HashSet, str::FromStr, time::Duration};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{to_json_binary, Addr, Binary, Coin, Coins, CosmosMsg, Deps, Env, StdResult};
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
    pub scheduler: Addr,
    pub contract_address: Addr,
    pub msg: Option<Binary>,
    pub cadence: Cadence,
    pub next: Option<Cadence>,
    pub execution_rebate: Vec<Coin>,
    pub executors: Vec<Addr>,
    pub jitter: Option<Duration>,
}

impl Schedule {
    pub fn execute_unsafe(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Condition)> {
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
            let current = self.cadence.clone().crank(deps, env)?;
            let condition = current.into_condition(deps, env, &self.scheduler)?;

            let create_trigger_msg = Contract(self.scheduler.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::Create(CreateTriggerMsg {
                    condition,
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
                vec![create_trigger_msg],
                Condition::Schedule(Schedule {
                    next: Some(current),
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

            Ok((vec![create_trigger_msg], Condition::Schedule(self)))
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

    fn execute(self, deps: Deps, env: &Env) -> (Vec<CosmosMsg>, Condition) {
        match self.clone().execute_unsafe(deps, env) {
            Ok((messages, schedule)) => (messages, schedule),
            Err(_) => (vec![], Condition::Schedule(self)),
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

    fn balances(&self, _deps: Deps, _env: &Env, _denoms: &HashSet<String>) -> StdResult<Coins> {
        Ok(Coins::default())
    }

    fn withdraw(
        self,
        _deps: Deps,
        _env: &Env,
        _desired: &HashSet<String>,
    ) -> StdResult<(Vec<CosmosMsg>, Condition)> {
        Ok((vec![], Condition::Schedule(self)))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Vec<CosmosMsg>, Condition)> {
        Ok((vec![], Condition::Schedule(self)))
    }
}
