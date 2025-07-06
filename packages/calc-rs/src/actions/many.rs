use std::{collections::HashSet, vec};

use cosmwasm_std::{Coins, Deps, Env, Event, StdError, StdResult, SubMsg};

use crate::actions::{action::Action, operation::Operation};

impl Operation for Vec<Action> {
    fn init(self, deps: Deps, env: &Env) -> StdResult<Action> {
        let mut actions = vec![];

        for action in self.into_iter() {
            let action = action.init(deps, env)?;
            actions.push(action);
        }

        Ok(Action::Many(actions))
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let mut all_messages = vec![];
        let mut all_events = vec![];
        let mut new_actions = vec![];

        for action in self.into_iter() {
            let (action, messages, events) = action.execute(deps, env)?;

            new_actions.push(action);
            all_messages.extend(messages);
            all_events.extend(events);
        }

        Ok((Action::Many(new_actions), all_messages, all_events))
    }

    fn update(
        self,
        deps: Deps,
        env: &Env,
        update: Action,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        if let Action::Many(update) = update {
            let new_behaviour = update.init(deps, env)?;
            let (action, messages, events) = new_behaviour.execute(deps, env)?;
            Ok((action, messages, events))
        } else {
            Err(StdError::generic_err("Invalid action type for update"))
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        let mut escrowed = HashSet::new();

        for action in self.iter() {
            let action_escrowed = action.escrowed(deps, env)?;
            escrowed.extend(action_escrowed);
        }

        Ok(escrowed)
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &[String]) -> StdResult<Coins> {
        let mut balances = Coins::default();

        for action in self.iter() {
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

        for action in self.clone().into_iter() {
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

        for action in self.into_iter() {
            let (action, messages, events) = action.cancel(deps, env)?;

            new_actions.push(action);
            all_messages.extend(messages);
            all_events.extend(events);
        }

        Ok((Action::Many(new_actions), all_messages, all_events))
    }
}
