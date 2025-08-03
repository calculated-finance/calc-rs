// use std::{collections::HashSet, vec};

// use cosmwasm_std::{Coins, Deps, Env, Event, StdError, StdResult};

// use crate::{
//     actions::{
//         action::Action,
//         operation::{StatefulOperation, StatelessOperation},
//     },
//     strategy::StrategyMsg,
// };

// impl StatelessOperation for Vec<Action> {
//     fn init(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Vec<Action>)> {
//         if self.is_empty() {
//             return Err(StdError::generic_err(
//                 "Cannot initialize an empty action list",
//             ));
//         }

//         let mut actions = Vec::with_capacity(self.len());
//         let mut messages = vec![];
//         let mut events = vec![];

//         for action in self.into_iter() {
//             let (action_messages, action_events, action) = action.init(deps, env)?;

//             actions.push(action);
//             messages.extend(action_messages);
//             events.extend(action_events);
//         }

//         Ok((messages, events, actions))
//     }

//     fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Vec<Action>) {
//         let mut all_messages = vec![];
//         let mut all_events = vec![];
//         let mut new_actions = Vec::with_capacity(self.len());

//         for action in self.into_iter() {
//             let (messages, events, action) = action.execute(deps, env);

//             new_actions.push(action);
//             all_messages.extend(messages);
//             all_events.extend(events);
//         }

//         (all_messages, all_events, new_actions)
//     }

//     fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
//         let mut denoms = HashSet::new();

//         for action in self.iter() {
//             let action_denoms = action.denoms(deps, env)?;
//             denoms.extend(action_denoms);
//         }

//         Ok(denoms)
//     }

//     fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
//         let mut escrowed = HashSet::new();

//         for action in self.iter() {
//             let action_escrowed = action.escrowed(deps, env)?;
//             escrowed.extend(action_escrowed);
//         }

//         Ok(escrowed)
//     }
// }

// impl StatefulOperation for Vec<Action> {
//     fn commit(
//         self,
//         deps: Deps,
//         env: &Env,
//     ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Vec<Action>)> {
//         let mut actions = Vec::with_capacity(self.len());
//         let mut messages = vec![];
//         let mut events = vec![];

//         for action in self.into_iter() {
//             let (action_messages, action_events, action) = action.commit(deps, env)?;

//             actions.push(action);
//             messages.extend(action_messages);
//             events.extend(action_events);
//         }

//         Ok((messages, events, actions))
//     }

//     fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
//         let mut balances = Coins::default();

//         for action in self.iter() {
//             let action_balances = action.balances(deps, env, denoms)?;

//             for balance in action_balances {
//                 balances.add(balance)?;
//             }
//         }

//         Ok(balances)
//     }

//     fn withdraw(
//         self,
//         deps: Deps,
//         env: &Env,
//         desired: &HashSet<String>,
//     ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Vec<Action>)> {
//         let mut actions = vec![];
//         let mut messages = vec![];
//         let mut events = Vec::with_capacity(self.len());

//         for action in self.clone().into_iter() {
//             let (action_messages, action_events, action) = action.withdraw(deps, env, desired)?;

//             actions.push(action);
//             messages.extend(action_messages);
//             events.extend(action_events);
//         }

//         Ok((messages, events, actions))
//     }

//     fn cancel(
//         self,
//         deps: Deps,
//         env: &Env,
//     ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Vec<Action>)> {
//         let mut all_messages = vec![];
//         let mut all_events = vec![];
//         let mut new_actions = Vec::with_capacity(self.len());

//         for action in self.into_iter() {
//             let (messages, events, action) = action.cancel(deps, env)?;

//             new_actions.push(action);
//             all_messages.extend(messages);
//             all_events.extend(events);
//         }

//         Ok((all_messages, all_events, new_actions))
//     }
// }
