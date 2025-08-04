use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coins, Deps, Env, Event, StdResult};

use crate::{
    actions::{action::Action, operation::Operation},
    condition::Condition,
    core::Threshold,
    manager::Affiliate,
    strategy::StrategyMsg,
};

#[cw_serde]
pub struct Conditional {
    pub threshold: Threshold,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
}

impl Operation<Action> for Conditional {
    fn init(self, _deps: Deps, _env: &Env, _affiliates: &[Affiliate]) -> StdResult<Action> {
        Ok(Action::Conditional(self))
    }

    fn execute(self, _deps: Deps, _env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Action) {
        (vec![], vec![], Action::Conditional(self))
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

    fn commit(self, _deps: Deps, _env: &Env) -> StdResult<Action> {
        Ok(Action::Conditional(self))
    }

    fn balances(&self, _deps: Deps, _env: &Env, _denoms: &HashSet<String>) -> StdResult<Coins> {
        Ok(Coins::default())
    }

    fn withdraw(
        self,
        _deps: Deps,
        _env: &Env,
        _desired: &HashSet<String>,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        Ok((vec![], vec![], Action::Conditional(self)))
    }

    fn cancel(self, _deps: Deps, _env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Action)> {
        Ok((vec![], vec![], Action::Conditional(self)))
    }
}
