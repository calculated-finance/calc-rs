use std::{collections::HashSet, u8, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, StdError, StdResult, SubMsg};

use crate::{
    actions::{action::Action, operation::Operation},
    conditions::{Condition, LogicalOperator},
    events::DomainEvent,
};

#[cw_serde]
pub struct CompositeAction {
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
}

impl Operation<CompositeAction> for CompositeAction {
    fn init(self, deps: Deps, env: &Env) -> StdResult<CompositeAction> {
        Ok(CompositeAction {
            actions: self
                .actions
                .into_iter()
                .flat_map(|action| action.init(deps, env))
                .collect(),
            conditions: self.conditions.clone(),
        })
    }

    fn condition(self, env: &Env) -> Option<Condition> {
        // A behaviour can only be executed if it's own conditions are met,
        // and all of it's actions' conditions are satisfied.
        Some(Condition::Compound {
            conditions: [
                vec![Condition::Compound {
                    conditions: self
                        .actions
                        .into_iter()
                        .flat_map(|action| action.condition(env))
                        .collect(),
                    // If actions are in the same list,
                    // they either all try to fire or none do.
                    operator: LogicalOperator::And,
                }],
                self.conditions.clone(),
            ]
            .concat(),
            operator: LogicalOperator::And,
        })
    }

    fn execute(
        self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(CompositeAction, Vec<SubMsg>, Vec<DomainEvent>)> {
        if self.conditions.iter().any(|c| c.check(deps, env).is_err()) {
            return Ok((self, vec![], vec![]));
        }

        let mut all_messages = vec![];
        let mut all_events = vec![];
        let mut new_actions = vec![];

        for action in self.actions.into_iter() {
            let (action, messages, events) = action.execute(deps, env)?;

            new_actions.push(action);
            all_messages.extend(messages);
            all_events.extend(events);
        }

        let new_conditions = self
            .conditions
            .iter()
            .map(|c| c.next(env))
            .collect::<Vec<_>>();

        Ok((
            CompositeAction {
                actions: new_actions,
                conditions: new_conditions,
            },
            all_messages,
            all_events,
        ))
    }

    fn update(
        self,
        deps: Deps,
        env: &Env,
        update: CompositeAction,
    ) -> StdResult<(CompositeAction, Vec<SubMsg>, Vec<DomainEvent>)> {
        if self.actions.len() > update.actions.len() {
            return Err(StdError::generic_err("Cannot remove actions"));
        }

        let mut all_messages = vec![];
        let mut all_events = vec![];
        let mut new_actions = vec![];

        for (i, action) in update.actions.into_iter().enumerate() {
            let (action, messages, events) = if i >= self.actions.len() {
                let action = action.init(deps, env)?;
                (action, vec![], vec![])
            } else {
                self.actions[i].clone().update(deps, env, action.clone())?
            };

            new_actions.push(action);
            all_messages.extend(messages);
            all_events.extend(events);
        }

        Ok((
            CompositeAction {
                actions: new_actions,
                conditions: update.conditions,
            },
            all_messages,
            all_events,
        ))
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

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &Coins,
    ) -> StdResult<(CompositeAction, Vec<SubMsg>, Coins)> {
        let mut remaining_desired = desired.clone();
        let mut withdrawals = Coins::default();
        let mut actions = vec![];
        let mut messages = vec![];

        for action in self.actions.clone().into_iter() {
            let (action, action_messages, action_withdrawals) =
                action.withdraw(deps, env, desired)?;

            for withdrawal in action_withdrawals {
                remaining_desired.sub(withdrawal.clone())?;
                withdrawals.add(withdrawal)?;
            }

            actions.push(action);
            messages.extend(action_messages);
        }

        Ok((CompositeAction { actions, ..self }, messages, withdrawals))
    }

    fn cancel(
        self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(CompositeAction, Vec<SubMsg>, Vec<DomainEvent>)> {
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
            CompositeAction {
                actions: new_actions,
                conditions: self.conditions,
            },
            all_messages,
            all_events,
        ))
    }
}
