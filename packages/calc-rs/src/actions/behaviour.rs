use std::{collections::HashSet, u8, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdError, StdResult, SubMsg};

use crate::{
    actions::{action::Action, operation::Operation},
    conditions::{Condition, Threshold},
};

#[cw_serde]
pub struct Behaviour {
    pub actions: Vec<Action>,
    pub threshold: Threshold,
}

impl Behaviour {
    pub fn size(&self) -> usize {
        self.actions
            .iter()
            .map(|action| {
                if let Action::Compose(behaviour) = action {
                    behaviour.size() + 1
                } else {
                    1
                }
            })
            .sum()
    }
}

impl Operation for Behaviour {
    fn init(self, deps: Deps, env: &Env) -> StdResult<Action> {
        let mut actions = vec![];

        for action in self.actions.into_iter() {
            let action = action.init(deps, env)?;
            actions.push(action);
        }

        Ok(Action::Compose(Behaviour { actions, ..self }))
    }

    fn condition(&self, env: &Env) -> Option<Condition> {
        Some(Condition::Compound {
            conditions: [vec![Condition::Compound {
                conditions: self
                    .actions
                    .iter()
                    .flat_map(|action| action.condition(env))
                    .collect(),
                operator: Threshold::All,
            }]]
            .concat(),
            operator: Threshold::All,
        })
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let mut all_messages = vec![];
        let mut all_events = vec![];
        let mut new_actions = vec![];

        if self.threshold == Threshold::All {
            for action in self.actions.clone().into_iter() {
                let result = action.execute(deps, env);

                if result.is_err() {
                    all_messages.clear();
                    all_events.clear();
                    new_actions.clear();

                    return Ok((Action::Compose(self), vec![], vec![]));
                }

                let (action, messages, events) = result?;

                new_actions.push(action);
                all_messages.extend(messages);
                all_events.extend(events);
            }
        } else {
            for action in self.actions.into_iter() {
                let (action, messages, events) = action.execute(deps, env)?;

                new_actions.push(action);
                all_messages.extend(messages);
                all_events.extend(events);
            }
        }

        Ok((
            Action::Compose(Behaviour {
                actions: new_actions,
                ..self
            }),
            all_messages,
            all_events,
        ))
    }

    fn update(
        self,
        deps: Deps,
        env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        if let Action::Compose(update) = update {
            let new_behaviour = update.init(deps, env)?;
            let (action, messages, events) = new_behaviour.execute(deps, env)?;
            Ok((action, messages, events))
        } else {
            Err(StdError::generic_err("Invalid action type for update"))
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        let mut escrowed = HashSet::new();

        for action in self.actions.iter() {
            let action_escrowed = action.escrowed(deps, env)?;
            escrowed.extend(action_escrowed);
        }

        Ok(escrowed)
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &[String]) -> StdResult<Coins> {
        let mut balances = Coins::default();

        for action in self.actions.iter() {
            let action_balances = action.balances(deps, env, denoms)?;

            for balance in action_balances {
                balances.add(balance)?;
            }
        }

        Ok(balances)
    }

    fn withdraw(&self, deps: Deps, env: &Env, desired: &Coins) -> StdResult<(Vec<SubMsg>, Coins)> {
        let mut remaining_desired = desired.clone();
        let mut withdrawals = Coins::default();
        let mut actions = vec![];
        let mut messages = vec![];

        for action in self.actions.clone().into_iter() {
            let (action_messages, action_withdrawals) = action.withdraw(deps, env, desired)?;

            for withdrawal in action_withdrawals {
                remaining_desired.sub(withdrawal.clone())?;
                withdrawals.add(withdrawal)?;
            }

            actions.push(action);
            messages.extend(action_messages);
        }

        Ok((messages, withdrawals))
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let mut all_messages = vec![];
        let mut all_events = vec![];
        let mut new_actions = vec![];

        for action in self.actions.into_iter() {
            let (action, messages, events) = action.cancel(deps, env)?;

            new_actions.push(action);
            all_messages.extend(messages);
            all_events.extend(events);
        }

        Ok((
            Action::Compose(Behaviour {
                actions: new_actions,
                ..self
            }),
            all_messages,
            all_events,
        ))
    }
}
