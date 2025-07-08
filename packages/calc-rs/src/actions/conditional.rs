use std::collections::HashSet;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdResult};

use crate::{
    actions::{action::Action, operation::Operation},
    conditions::{Condition, Threshold},
    strategy::StrategyMsg,
};

enum ConditionalEvent {
    Skipped { reason: String },
}

impl Into<Event> for ConditionalEvent {
    fn into(self) -> Event {
        match self {
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
    fn init(self, _deps: Deps, _env: &Env) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        let (action, messages, events) = self.action.init(_deps, _env)?;

        Ok((
            Action::Conditional(Conditional {
                action: Box::new(action),
                ..self
            }),
            messages,
            events,
        ))
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        if self.is_satisfied(deps, env) {
            let (action, msgs, events) = self.action.execute(deps, env)?;

            Ok((
                Action::Conditional(Conditional {
                    action: Box::new(action),
                    ..self
                }),
                msgs,
                events,
            ))
        } else {
            Ok((
                Action::Conditional(self),
                vec![],
                vec![ConditionalEvent::Skipped {
                    reason: "Conditions not met".into(),
                }
                .into()],
            ))
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
    ) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        let (action, messages, events) = self.action.withdraw(deps, env, desired)?;

        Ok((
            Action::Conditional(Conditional {
                action: Box::new(action),
                ..self
            }),
            messages,
            events,
        ))
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Action, Vec<StrategyMsg>, Vec<Event>)> {
        let (action, messages, events) = self.action.cancel(deps, env)?;

        Ok((
            Action::Conditional(Conditional {
                action: Box::new(action),
                ..self
            }),
            messages,
            events,
        ))
    }
}
