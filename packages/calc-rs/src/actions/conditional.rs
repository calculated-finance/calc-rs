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
    pub action: Box<Action>,
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

        let (messages, events, action) = self.action.init(deps, env)?;

        Ok((
            messages,
            events,
            Action::Conditional(Conditional {
                action: Box::new(action),
                ..self
            }),
        ))
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        if self.condition.is_satisfied(deps, env).unwrap_or(false) {
            let (msgs, events, action) = self.action.execute(deps, env);
            (
                msgs,
                events,
                Action::Conditional(Conditional {
                    action: Box::new(action),
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
        self.action.denoms(deps, env)
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        self.action.escrowed(deps, env)
    }
}

impl StatefulOperation for Conditional {
    fn commit(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        let (messages, events, action) = self.action.commit(deps, env)?;
        Ok((
            messages,
            events,
            Action::Conditional(Conditional {
                action: Box::new(action),
                ..self
            }),
        ))
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        self.action.balances(deps, env, denoms)
    }

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        let (messages, events, action) = self.action.withdraw(deps, env, desired)?;
        Ok((
            messages,
            events,
            Action::Conditional(Conditional {
                action: Box::new(action),
                ..self
            }),
        ))
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        let (messages, events, action) = self.action.cancel(deps, env)?;
        Ok((
            messages,
            events,
            Action::Conditional(Conditional {
                action: Box::new(action),
                ..self
            }),
        ))
    }
}
