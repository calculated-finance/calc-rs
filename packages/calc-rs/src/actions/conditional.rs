use std::collections::HashSet;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdError, StdResult};

use crate::{
    actions::{
        action::Action,
        operation::{StatefulOperation, StatelessOperation},
    },
    conditions::Condition,
    strategy::StrategyMsg,
};

enum ConditionalEvent {
    SkipConditionalExecution { reason: String },
}

impl From<ConditionalEvent> for Event {
    fn from(val: ConditionalEvent) -> Self {
        match val {
            ConditionalEvent::SkipConditionalExecution { reason } => {
                Event::new("skip_conditional_execution").add_attribute("reason", reason)
            }
        }
    }
}

#[cw_serde]
pub struct Conditional {
    pub condition: Condition,
    pub actions: Vec<Action>,
}

impl StatelessOperation for Conditional {
    fn init(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        // We don't care if it's satisfied at init time,
        // only that the condition itself is valid.
        self.condition.is_satisfied(deps, env)?;

        if self.condition.size() > 10 {
            return Err(StdError::generic_err(
                "Condition size exceeds maximum limit of 20",
            ));
        }

        let mut actions = Vec::with_capacity(self.actions.len());
        let mut messages = vec![];
        let mut events = vec![];

        for action in self.actions.into_iter() {
            let (action_messages, action_events, action) = action.init(deps, env)?;

            actions.push(action);
            messages.extend(action_messages);
            events.extend(action_events);
        }

        Ok((
            messages,
            events,
            Action::Conditional(Conditional { actions, ..self }),
        ))
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        if self.condition.is_satisfied(deps, env).unwrap_or(false) {
            let mut all_messages = vec![];
            let mut all_events = vec![];
            let mut new_actions = Vec::with_capacity(self.actions.len());

            for action in self.actions.into_iter() {
                let (messages, events, action) = action.execute(deps, env);

                new_actions.push(action);
                all_messages.extend(messages);
                all_events.extend(events);
            }

            (
                all_messages,
                all_events,
                Action::Conditional(Conditional {
                    actions: new_actions,
                    ..self
                }),
            )
        } else {
            (
                vec![],
                vec![ConditionalEvent::SkipConditionalExecution {
                    reason: "Conditions not met".into(),
                }
                .into()],
                Action::Conditional(self),
            )
        }
    }

    fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        let mut denoms = HashSet::new();

        for action in self.actions.iter() {
            let action_denoms = action.denoms(deps, env)?;
            denoms.extend(action_denoms);
        }

        Ok(denoms)
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        let mut escrowed = HashSet::new();

        for action in self.actions.iter() {
            let action_escrowed = action.escrowed(deps, env)?;
            escrowed.extend(action_escrowed);
        }

        Ok(escrowed)
    }
}

impl StatefulOperation for Conditional {
    fn commit(self, deps: Deps, env: &Env) -> StdResult<Action> {
        let mut actions = Vec::with_capacity(self.actions.len());

        for action in self.actions.into_iter() {
            actions.push(action.commit(deps, env)?);
        }

        Ok(Action::Conditional(Conditional { actions, ..self }))
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
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
        desired: &HashSet<String>,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        let mut actions = vec![];
        let mut messages = vec![];
        let mut events = Vec::with_capacity(self.actions.len());

        for action in self.actions.clone().into_iter() {
            let (action_messages, action_events, action) = action.withdraw(deps, env, desired)?;

            actions.push(action);
            messages.extend(action_messages);
            events.extend(action_events);
        }

        Ok((
            messages,
            events,
            Action::Conditional(Conditional { actions, ..self }),
        ))
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        let mut messages = vec![];
        let mut events = vec![];
        let mut actions = Vec::with_capacity(self.actions.len());

        for action in self.actions.into_iter() {
            let (action_messages, action_events, action) = action.cancel(deps, env)?;

            actions.push(action);
            messages.extend(action_messages);
            events.extend(action_events);
        }

        Ok((
            messages,
            events,
            Action::Conditional(Conditional { actions, ..self }),
        ))
    }
}
