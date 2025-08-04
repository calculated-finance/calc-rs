use calc_rs::{
    manager::Affiliate,
    strategy::{Indexable, Node, Strategy},
};
use cosmwasm_std::{Addr, Coin};
use cw_multi_test::error::AnyResult;

use crate::{harness::CalcTestApp, strategy_handler::StrategyHandler};

pub struct StrategyBuilder<'a> {
    app: &'a mut CalcTestApp,
    owner: Addr,
    label: String,
    affiliates: Vec<Affiliate>,
    nodes: Vec<Node>,
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
            nodes: vec![],
            keeper,
        }
    }

    pub fn with_nodes(mut self, nodes: Vec<Node>) -> Self {
        self.nodes = nodes;
        self
    }

    pub fn with_affiliates(mut self, affiliates: Vec<Affiliate>) -> Self {
        self.affiliates = affiliates;
        self
    }

    pub fn instantiate(self, funds: &[Coin]) -> StrategyHandler<'a> {
        let strategy = Strategy {
            owner: self.owner.clone(),
            nodes: self.nodes,
            affiliates: self.affiliates.clone(),
            state: Indexable,
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
            nodes: self.nodes,
            affiliates: self.affiliates.clone(),
            state: Indexable,
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

    pub fn instantiate_with_affiliates(
        self,
        affiliates: Vec<Affiliate>,
        funds: &[Coin],
    ) -> StrategyHandler<'a> {
        let strategy = Strategy {
            owner: self.owner.clone(),
            nodes: self.nodes,
            affiliates: self.affiliates.clone(),
            state: Indexable,
        };

        let strategy_addr = self
            .app
            .create_strategy(&self.label, strategy, affiliates, funds)
            .unwrap();

        StrategyHandler {
            strategy_addr,
            owner: self.owner,
            keeper: self.keeper,
            harness: self.app,
        }
    }

    pub fn try_instantiate_with_affiliates(
        self,
        affiliates: Vec<Affiliate>,
        funds: &[Coin],
    ) -> AnyResult<StrategyHandler<'a>> {
        let strategy = Strategy {
            owner: self.owner.clone(),
            nodes: self.nodes,
            affiliates: self.affiliates.clone(),
            state: Indexable,
        };

        let strategy_addr = self
            .app
            .create_strategy(&self.label, strategy, affiliates, funds)?;

        Ok(StrategyHandler {
            strategy_addr,
            owner: self.owner,
            keeper: self.keeper,
            harness: self.app,
        })
    }
}
