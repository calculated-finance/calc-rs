use calc_rs::{
    actions::action::Action,
    manager::Affiliate,
    strategy::{Json, Strategy},
};
use cosmwasm_std::{Addr, Coin};
use cw_multi_test::error::AnyResult;

use crate::{harness::CalcTestApp, strategy_handler::StrategyHandler};

pub struct StrategyBuilder<'a> {
    app: &'a mut CalcTestApp,
    owner: Addr,
    label: String,
    affiliates: Vec<Affiliate>,
    action: Option<Action>,
    keeper: Addr,
}

impl<'a> StrategyBuilder<'a> {
    pub fn new(app: &'a mut CalcTestApp) -> Self {
        let owner = app.app.api().addr_make("owner");
        let keeper = app.app.api().addr_make("keeper");

        Self {
            app,
            owner,
            label: "Test Strategy".to_string(),
            affiliates: vec![],
            action: None,
            keeper,
        }
    }

    pub fn with_action(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }

    pub fn with_affiliates(mut self, affiliates: Vec<Affiliate>) -> Self {
        self.affiliates = affiliates;
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
            .create_strategy(&self.label, strategy, self.affiliates, funds)
            .unwrap();

        StrategyHandler {
            strategy_addr,
            owner: self.owner,
            keeper: self.keeper,
            harness: self.app,
        }
    }

    pub fn try_instantiate(self, funds: &[Coin]) -> AnyResult<StrategyHandler<'a>> {
        let strategy = Strategy {
            owner: self.owner.clone(),
            action: self.action.unwrap(),
            state: Json,
        };

        let strategy_addr =
            self.app
                .create_strategy(&self.label, strategy, self.affiliates, funds)?;

        Ok(StrategyHandler {
            strategy_addr,
            owner: self.owner,
            keeper: self.keeper,
            harness: self.app,
        })
    }
}
