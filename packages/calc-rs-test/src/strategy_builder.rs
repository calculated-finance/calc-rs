use calc_rs::{
    actions::action::Action,
    manager::StrategyStatus,
    statistics::Statistics,
    strategy::{Json, Strategy, StrategyConfig},
};
use cosmwasm_std::{Addr, Coin, Decimal, Uint128};
use rujira_rs::fin::{OrderResponse, OrdersResponse, Price, Side};

use crate::harness::CalcTestApp;

pub struct StrategyBuilder<'a> {
    app: &'a mut CalcTestApp,
    owner: Addr,
    label: String,
    action: Option<Action>,
    funds: Vec<Coin>,
    keeper: Option<Addr>,
}

impl<'a> StrategyBuilder<'a> {
    pub fn new(app: &'a mut CalcTestApp, owner: Addr, label: &str) -> Self {
        Self {
            app,
            owner,
            label: label.to_string(),
            action: None,
            funds: vec![],
            keeper: None,
        }
    }

    pub fn with_action(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }

    pub fn with_funds(mut self, funds: Vec<Coin>) -> Self {
        self.funds = funds;
        self
    }

    pub fn with_keeper(mut self, keeper: Addr) -> Self {
        self.keeper = Some(keeper);
        self
    }

    pub fn instantiate(self) -> StrategyHandler {
        let strategy = Strategy {
            owner: self.owner.clone(),
            action: self.action.unwrap(),
            state: Json,
        };

        let strategy_addr = self
            .app
            .create_strategy(&self.owner, &self.owner, &self.label, strategy)
            .unwrap();

        if !self.funds.is_empty() {
            self.app
                .fund_contract(&strategy_addr, &self.owner, &self.funds);
        }

        StrategyHandler {
            strategy_addr,
            keeper: self.keeper,
        }
    }
}

pub struct StrategyHandler {
    pub strategy_addr: Addr,
    keeper: Option<Addr>,
}

impl StrategyHandler {
    pub fn execute(&mut self, app: &mut CalcTestApp, sender: &Addr, funds: &[Coin]) -> &mut Self {
        let keeper = self.keeper.as_ref().unwrap_or(sender);
        app.execute_strategy(keeper, &self.strategy_addr, funds)
            .unwrap();
        self
    }

    pub fn assert_balance(&mut self, app: &mut CalcTestApp, expected_balances: Coin) -> &mut Self {
        let balances = app.query_balances(&self.strategy_addr);
        assert!(balances.iter().any(|c| c.denom == expected_balances.denom && c.amount == expected_balances.amount), "Expected balance not found: {expected_balances:?}");
        self
    }

    pub fn assert_balances(
        &mut self,
        app: &mut CalcTestApp,
        expected_balances: Vec<Coin>,
    ) -> &mut Self {
        let balances = app.query_balances(&self.strategy_addr);
        assert_eq!(balances, expected_balances);
        self
    }

    pub fn assert_stats(&mut self, app: &mut CalcTestApp, expected_stats: Statistics) -> &mut Self {
        let stats = app.query_strategy_stats(&self.strategy_addr);
        assert_eq!(stats, expected_stats);
        self
    }

    pub fn pause(&mut self, app: &mut CalcTestApp, sender: &Addr) -> &mut Self {
        let keeper = self.keeper.as_ref().unwrap_or(sender);
        app.update_strategy_status(keeper, &self.strategy_addr, StrategyStatus::Paused)
            .unwrap();
        self
    }

    pub fn resume(&mut self, app: &mut CalcTestApp, sender: &Addr) -> &mut Self {
        let keeper = self.keeper.as_ref().unwrap_or(sender);
        app.update_strategy_status(keeper, &self.strategy_addr, StrategyStatus::Active)
            .unwrap();
        self
    }

    pub fn advance_blocks(&mut self, app: &mut CalcTestApp, blocks: u64) -> &mut Self {
        app.advance_blocks(blocks);
        self
    }

    pub fn advance_time(&mut self, app: &mut CalcTestApp, seconds: u64) -> &mut Self {
        app.advance_time(seconds);
        self
    }

    pub fn assert_swapped(
        &mut self,
        app: &mut CalcTestApp,
        expected_swapped: Vec<Coin>,
    ) -> &mut Self {
        let stats = app.query_strategy_stats(&self.strategy_addr);
        assert_eq!(stats.swapped, expected_swapped);
        self
    }

    pub fn assert_config(
        &mut self,
        app: &mut CalcTestApp,
        expected_config: StrategyConfig,
    ) -> &mut Self {
        let config = app.query_strategy_config(&self.strategy_addr);
        assert_eq!(config, expected_config);
        self
    }

    pub fn assert_status(
        &mut self,
        app: &mut CalcTestApp,
        expected_status: StrategyStatus,
    ) -> &mut Self {
        let strategy = app.query_strategy(&self.strategy_addr);
        assert_eq!(strategy.status, expected_status);
        self
    }

    pub fn assert_fin_orders(
        &mut self,
        app: &mut CalcTestApp,
        pair_address: &Addr,
        expected_fin_orders: Vec<(Side, Decimal, Uint128, Uint128, Uint128)>,
    ) -> &mut Self {
        let fin_orders = app.get_fin_orders(pair_address, &self.strategy_addr, None, None, None);
        assert_eq!(
            fin_orders,
            OrdersResponse {
                orders: expected_fin_orders
                    .iter()
                    .map(|(side, price, offer, remaining, filled)| OrderResponse {
                        owner: self.strategy_addr.to_string(),
                        side: side.clone(),
                        price: Price::Fixed(price.clone()),
                        rate: *price,
                        updated_at: app.app.block_info().time,
                        offer: *offer,
                        remaining: *remaining,
                        filled: *filled,
                    })
                    .collect()
            }
        );
        self
    }
}
