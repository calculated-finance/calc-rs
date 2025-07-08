use calc_rs::{
    manager::StrategyStatus, scheduler::ConditionFilter, statistics::Statistics,
    strategy::StrategyConfig,
};
use cosmwasm_std::{Addr, Coin, Decimal, Uint128};
use rujira_rs::fin::{OrderResponse, OrdersResponse, Price, Side};

use crate::harness::CalcTestApp;

pub struct StrategyHandler<'a> {
    pub strategy_addr: Addr,
    pub owner: Addr,
    pub keeper: Addr,
    pub harness: &'a mut CalcTestApp,
}

impl<'a> StrategyHandler<'a> {
    // Chain helpers

    pub fn advance_blocks(&mut self, blocks: u64) -> &mut Self {
        println!(
            "[StrategyHandler] Advancing {} blocks (strategy: {})",
            blocks, self.strategy_addr
        );
        self.harness.advance_blocks(blocks);

        self.harness
            .execute_filtered_triggers(
                &self.keeper,
                ConditionFilter::Timestamp {
                    start: None,
                    end: Some(self.harness.app.block_info().time),
                },
            )
            .unwrap();

        self.harness
            .execute_filtered_triggers(
                &self.keeper,
                ConditionFilter::BlockHeight {
                    start: None,
                    end: Some(self.harness.app.block_info().height),
                },
            )
            .unwrap();

        self
    }

    pub fn advance_time(&mut self, seconds: u64) -> &mut Self {
        println!(
            "[StrategyHandler] Advancing time by {} seconds (strategy: {})",
            seconds, self.strategy_addr
        );
        self.harness.advance_time(seconds);

        self.harness
            .execute_filtered_triggers(
                &self.keeper,
                ConditionFilter::Timestamp {
                    start: None,
                    end: Some(self.harness.app.block_info().time),
                },
            )
            .unwrap();

        self.harness
            .execute_filtered_triggers(
                &self.keeper,
                ConditionFilter::BlockHeight {
                    start: None,
                    end: Some(self.harness.app.block_info().height),
                },
            )
            .unwrap();

        self
    }

    // Strategy helpers

    pub fn execute(&mut self) -> &mut Self {
        self.harness
            .execute_strategy(&self.keeper, &self.strategy_addr)
            .unwrap();
        self
    }

    pub fn execute_triggers(&mut self) -> &mut Self {
        self.harness
            .execute_owned_triggers(&self.keeper, &self.strategy_addr)
            .unwrap();
        self
    }

    pub fn pause(&mut self) -> &mut Self {
        println!("[StrategyHandler] Pausing strategy");
        self.harness
            .update_strategy_status(&self.owner, &self.strategy_addr, StrategyStatus::Paused)
            .unwrap();
        self
    }

    pub fn resume(&mut self) -> &mut Self {
        println!("[StrategyHandler] Resuming strategy");
        self.harness
            .update_strategy_status(&self.owner, &self.strategy_addr, StrategyStatus::Active)
            .unwrap();
        self
    }

    // Assertion helpers

    pub fn assert_balance(&mut self, expected: Coin) -> &mut Self {
        println!(
            "[StrategyHandler] Asserting strategy balance is {:#?}",
            expected
        );
        let balances = self.harness.query_balances(&self.strategy_addr);
        assert!(
            balances
                .iter()
                .any(|c| c.denom == expected.denom && c.amount == expected.amount),
            "Expected balance not found: {expected:?}"
        );
        self
    }

    pub fn assert_balances(&mut self, expected_balances: Vec<Coin>) -> &mut Self {
        println!(
            "[StrategyHandler] Asserting all strategy balances are {:#?}",
            expected_balances
        );
        let balances = self.harness.query_balances(&self.strategy_addr);
        assert_eq!(
            balances, expected_balances,
            "Expected balances do not match current balances: expected {:#?}, got {:#?}",
            expected_balances, balances
        );
        self
    }

    pub fn assert_stats(&mut self, expected_stats: Statistics) -> &mut Self {
        println!(
            "[StrategyHandler] Asserting strategy stats are {:#?}",
            expected_stats
        );
        let stats = self.harness.query_strategy_stats(&self.strategy_addr);
        assert_eq!(
            stats, expected_stats,
            "Expected stats do not match current stats: expected {:#?}, got {:#?}",
            expected_stats, stats
        );
        self
    }

    pub fn assert_swapped(&mut self, expected_swapped: Vec<Coin>) -> &mut Self {
        println!(
            "[StrategyHandler] Asserting swapped coins are {:#?}",
            expected_swapped
        );
        let stats = self.harness.query_strategy_stats(&self.strategy_addr);
        assert_eq!(
            stats.swapped, expected_swapped,
            "Expected swapped coins do not match current swapped coins: expected {:#?}, got {:#?}",
            expected_swapped, stats.swapped
        );
        self
    }

    pub fn assert_config(&mut self, expected_config: StrategyConfig) -> &mut Self {
        println!(
            "[StrategyHandler] Asserting strategy config is {:#?}",
            expected_config
        );
        let config = self.harness.query_strategy_config(&self.strategy_addr);
        assert_eq!(
            config, expected_config,
            "Expected config does not match current config: expected {:#?}, got {:#?}",
            expected_config, config
        );
        self
    }

    pub fn assert_status(&mut self, expected_status: StrategyStatus) -> &mut Self {
        println!(
            "[StrategyHandler] Asserting strategy status is {:#?}",
            self.strategy_addr
        );
        let strategy = self.harness.query_strategy(&self.strategy_addr);
        assert_eq!(
            strategy.status, expected_status,
            "Expected status does not match current status: expected {:#?}, got {:#?}",
            expected_status, strategy.status
        );
        self
    }

    pub fn assert_fin_orders(
        &mut self,
        pair_address: &Addr,
        expected_fin_orders: Vec<(Side, Decimal, Uint128, Uint128, Uint128)>,
    ) -> &mut Self {
        println!(
            "[StrategyHandler] Asserting strategy fin orders on pair {} are {:#?}",
            pair_address, expected_fin_orders
        );
        let fin_orders =
            self.harness
                .get_fin_orders(pair_address, &self.strategy_addr, None, None, None);
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
                        updated_at: self.harness.app.block_info().time,
                        offer: *offer,
                        remaining: *remaining,
                        filled: *filled,
                    })
                    .collect()
            },
            "Expected fin orders do not match current orders: expected {:#?}, got {:#?}",
            expected_fin_orders,
            fin_orders.orders
        );
        self
    }
}
