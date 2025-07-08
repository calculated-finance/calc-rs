use calc_rs::{
    actions::action::Action,
    strategy::{Json, Strategy},
};
use cosmwasm_std::{Addr, Coin};

use crate::{harness::CalcTestApp, strategy_handler::StrategyHandler};

pub struct StrategyBuilder<'a> {
    app: &'a mut CalcTestApp,
    owner: Addr,
    label: String,
    action: Option<Action>,
    keeper: Addr,
}

impl<'a> StrategyBuilder<'a> {
    pub fn new(app: &'a mut CalcTestApp, owner: Addr, label: &str, keeper: Addr) -> Self {
        Self {
            app,
            owner,
            label: label.to_string(),
            action: None,
            keeper,
        }
    }

    pub fn with_action(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }

    pub fn instantiate(self, funds: &[Coin]) -> StrategyHandler<'a> {
        let strategy = Strategy {
            owner: self.owner.clone(),
            action: self.action.unwrap(),
            state: Json,
        };

        let strategy_addr = self
            .app
            .create_strategy(&self.owner, &self.owner, &self.label, strategy, funds)
            .unwrap();

        StrategyHandler {
            strategy_addr,
            owner: self.owner,
            keeper: self.keeper,
            harness: self.app,
        }
    }
}
