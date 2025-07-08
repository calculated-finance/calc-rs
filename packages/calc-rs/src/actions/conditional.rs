use std::collections::HashSet;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdError, StdResult};

use crate::{
    actions::{action::Action, operation::Operation},
    conditions::{Condition, Threshold},
    strategy::StrategyMsg,
};

enum ConditionalEvent {
    Skipped { reason: String },
}

impl From<ConditionalEvent> for Event {
    fn from(val: ConditionalEvent) -> Self {
        match val {
            ConditionalEvent::Skipped { reason } => {
                Event::new("conditional_action_skipped").add_attribute("reason", reason)
            }
        }
    }
}

#[cw_serde]
pub struct Conditional {
    pub conditions: Vec<Condition>,
    pub threshold: Threshold,
    pub action: Box<Action>,
}

impl Conditional {
    fn is_satisfied(&self, deps: Deps, env: &Env) -> bool {
        match self.threshold {
            Threshold::All => {
                for condition in &self.conditions {
                    if !condition.is_satisfied(deps, env) {
                        return false;
                    }
                }
                true
            }
            Threshold::Any => {
                for condition in &self.conditions {
                    if condition.is_satisfied(deps, env) {
                        return true;
                    }
                }
                false
            }
        }
    }
}

impl Operation for Conditional {
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        if self.conditions.is_empty() {
            return Err(StdError::generic_err(
                "Conditional conditions cannot be empty",
            ));
        }

        let (messages, events, action) = self.action.init(_deps, _env)?;

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
        if self.is_satisfied(deps, env) {
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
                vec![ConditionalEvent::Skipped {
                    reason: "Conditions not met".into(),
                }
                .into()],
                Action::Conditional(self),
            )
        }
    }

    fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        self.action.escrowed(deps, env)
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
