use std::{collections::HashSet, vec};

use cosmwasm_std::{Coins, Deps, Env, Event, StdResult, SubMsg};

use crate::actions::{action::Action, operation::Operation};

impl Operation for Vec<Action> {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let mut actions = vec![];
        let mut messages = vec![];
        let mut events = vec![];

        for action in self.into_iter() {
            let (action, action_messages, action_events) = action.init(deps, env)?;

            actions.push(action);
            messages.extend(action_messages);
            events.extend(action_events);
        }

        Ok((Action::Many(actions), messages, events))
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

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        let mut escrowed = HashSet::new();

        for action in self.iter() {
            let action_escrowed = action.escrowed(deps, env)?;
            escrowed.extend(action_escrowed);
        }

        Ok(escrowed)
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        let mut balances = Coins::default();

        for action in self.iter() {
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
        desired: &HashSet<String>,
    ) -> StdResult<(Action, Vec<SubMsg>, Vec<Event>)> {
        let mut actions = vec![];
        let mut messages = vec![];
        let mut events = vec![];

        for action in self.clone().into_iter() {
            let (action, action_messages, action_events) = action.withdraw(deps, env, desired)?;

            actions.push(action);
            messages.extend(action_messages);
            events.extend(action_events);
        }

        Ok((Action::Many(actions), messages, events))
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
