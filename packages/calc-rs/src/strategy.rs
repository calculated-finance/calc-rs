use std::{collections::HashSet, u8, vec};

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Coin, Coins, Deps, Env, StdError, StdResult, SubMsg, Uint128};

use crate::{
    actions::{action::Action, operation::Operation},
    conditions::{Condition, LogicalOperator},
    events::DomainEvent,
    manager::{Affiliate, StrategyStatus},
    statistics::Statistics,
};

#[cw_serde]
pub struct Strategy2 {
    pub manager: Addr,
    pub owner: Addr,
    pub actions: Vec<Action>,
}
impl Strategy2 {
    pub fn init(self, deps: Deps, env: &Env) -> StdResult<Strategy2> {
        Ok(Strategy2 {
            actions: self
                .actions
                .into_iter()
                .flat_map(|action| action.init(deps, env))
                .collect(),
            ..self
        })
    }

    pub fn condition(self, env: &Env) -> Option<Condition> {
        Some(Condition::Compound {
            conditions: self
                .actions
                .into_iter()
                .flat_map(|action| action.condition(env))
                .collect(),
            operator: LogicalOperator::Or,
        })
    }

    pub fn execute(
        self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(Strategy2, Vec<SubMsg>, Vec<DomainEvent>)> {
        let mut all_messages = vec![];
        let mut all_events = vec![];
        let mut new_actions = vec![];

        for action in self.actions.into_iter() {
            let (action, messages, events) = action.execute(deps, env)?;

            new_actions.push(action);
            all_messages.extend(messages);
            all_events.extend(events);
        }

        Ok((
            Strategy2 {
                actions: new_actions,
                ..self
            },
            all_messages,
            all_events,
        ))
    }

    pub fn update(
        self,
        deps: Deps,
        env: &Env,
        update: Strategy2,
    ) -> StdResult<(Strategy2, Vec<SubMsg>, Vec<DomainEvent>)> {
        if self.actions.len() > update.actions.len() {
            return Err(StdError::generic_err("Cannot remove actions"));
        }

        let mut all_messages = vec![];
        let mut all_events = vec![];
        let mut new_actions = vec![];

        for (i, action) in update.actions.into_iter().enumerate() {
            let (new_action, messages, events) = if i >= self.actions.len() {
                let action = action.init(deps, env)?;
                (action, vec![], vec![])
            } else {
                self.actions[i].clone().update(deps, env, action.clone())?
            };

            new_actions.push(new_action);
            all_messages.extend(messages);
            all_events.extend(events);
        }

        Ok((
            Strategy2 {
                actions: new_actions,
                ..self
            },
            all_messages,
            all_events,
        ))
    }

    pub fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        let mut escrowed = HashSet::new();

        for action in self.actions.iter() {
            let action_escrowed = action.escrowed(deps, env)?;
            escrowed.extend(action_escrowed);
        }

        Ok(escrowed)
    }

    pub fn balances(&self, deps: Deps, env: &Env, denoms: &[String]) -> StdResult<Coins> {
        let mut balances = Coins::default();

        for denom in denoms {
            let balance = deps
                .querier
                .query_balance(env.contract.address.clone(), denom)?;

            balances.add(balance)?;
        }

        for action in self.actions.iter() {
            let action_balances = action.balances(deps, env, denoms)?;

            for balance in action_balances {
                balances.add(balance)?;
            }
        }

        Ok(balances)
    }

    pub fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &Coins,
    ) -> StdResult<(Strategy2, Vec<SubMsg>, Coins)> {
        let escrowed = self.escrowed(deps, env)?;

        for denom in escrowed {
            if desired.amount_of(&denom).gt(&Uint128::zero()) {
                return Err(StdError::generic_err(format!(
                    "Cannot withdraw escrowed denom: {denom}"
                )));
            }
        }

        let mut outstanding = desired.clone();
        let mut withdrawals = Coins::default();
        let mut messages = vec![];
        let mut actions = vec![];

        for action in self.actions.clone().into_iter() {
            let (action, action_messages, action_withdrawals) =
                action.withdraw(deps, env, desired)?;

            for withdrawal in action_withdrawals {
                outstanding.sub(withdrawal.clone())?;
                withdrawals.add(withdrawal)?;
            }

            messages.extend(action_messages);
            actions.push(action);
        }

        Ok((Strategy2 { actions, ..self }, messages, withdrawals))
    }

    pub fn cancel(
        self,
        deps: Deps,
        env: &Env,
    ) -> StdResult<(Strategy2, Vec<SubMsg>, Vec<DomainEvent>)> {
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
            Strategy2 {
                actions: new_actions,
                ..self
            },
            all_messages,
            all_events,
        ))
    }
}

#[cw_serde]
pub struct StrategyInstantiateMsg {
    pub owner: Addr,
    pub affiliates: Vec<Affiliate>,
    pub actions: Vec<Action>,
}

#[cw_serde]
pub enum StrategyExecuteMsg {
    Execute {},
    Withdraw(Vec<Coin>),
    Update(Strategy2),
    UpdateStatus(StrategyStatus),
    Clear {},
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum StrategyQueryMsg {
    #[returns(Strategy2)]
    Config {},
    #[returns(Statistics)]
    Statistics {},
    #[returns(Vec<Coin>)]
    Balances { include: Vec<String> },
}
