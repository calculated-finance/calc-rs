use std::fmt::Debug;

use calc_rs::{manager::StrategyStatus, scheduler::ConditionFilter, strategy::StrategyConfig};
use cosmwasm_std::{Addr, Coin, Decimal, Uint128};
use cw_multi_test::{error::AnyResult, AppResponse};
use rujira_rs::fin::{OrderResponse, OrdersResponse, Price, Side};

use crate::harness::CalcTestApp;

pub struct StrategyHandler<'a> {
    pub strategy_addr: Addr,
    pub owner: Addr,
    pub keeper: Addr,
    pub harness: &'a mut CalcTestApp,
}

impl Debug for StrategyHandler<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StrategyHandler")
            .field("strategy_addr", &self.strategy_addr)
            .field("owner", &self.owner)
            .field("keeper", &self.keeper)
            .finish()
    }
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
                    end: None,
                },
            )
            .unwrap();

        self.harness
            .execute_filtered_triggers(
                &self.keeper,
                ConditionFilter::BlockHeight {
                    start: None,
                    end: None,
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

        self.harness
            .execute_filtered_triggers(
                &self.keeper,
                ConditionFilter::LimitOrder {
                    pair_address: self.harness.fin_addr.clone(),
                    price_range: None,
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

    pub fn deposit(&mut self, funds: &[Coin]) -> &mut Self {
        self.harness
            .fund_contract(&self.owner, &self.strategy_addr, funds);
        self
    }

    pub fn execute_triggers(&mut self, filter: ConditionFilter) -> &mut Self {
        self.harness
            .execute_filtered_triggers(&self.keeper, filter)
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

    pub fn withdraw(&mut self, amounts: Vec<Coin>) -> &mut Self {
        self.harness
            .withdraw(&self.owner, &self.strategy_addr, amounts)
            .unwrap();
        self
    }

    pub fn try_withdraw(&mut self, amounts: Vec<Coin>) -> AnyResult<AppResponse> {
        self.harness
            .withdraw(&self.owner, &self.strategy_addr, amounts)
    }

    pub fn config(self) -> StrategyConfig {
        self.harness.query_strategy_config(&self.strategy_addr)
    }

    // Assertion helpers

    pub fn assert_strategy_balance(&mut self, expected: &Coin) -> &mut Self {
        println!("[StrategyHandler] Asserting strategy balance is {expected:#?}");
        let balances = self.harness.query_strategy_balances(&self.strategy_addr);

        assert!(
            // Allow for rounding discrepancies
            balances.iter().any(|b| b.amount.abs_diff(expected.amount) < Uint128::new(10)),
            "Expected strategy balance not found: {expected:?}\n\nCurrent strategy balances: {balances:#?}",
        );
        self
    }

    pub fn assert_strategy_balances(&mut self, expected_balances: &[Coin]) -> &mut Self {
        println!("[StrategyHandler] Asserting all strategy balances are {expected_balances:#?}");
        let balances = self.harness.query_balances(&self.strategy_addr);

        for expected in expected_balances {
            let actual = balances.iter().find(|c| {
                // Allow for rounding discrepancies
                c.denom == expected.denom && c.amount.abs_diff(expected.amount) < Uint128::new(10)
            });

            if expected.amount.is_zero() {
                assert!(
                    actual.is_none(),
                    "Expected zero balance for {} but found: {actual:?}",
                    expected.denom
                );
            } else {
                assert!(
                    actual.is_some(),
                    "Expected balance not found: {expected:?}\n\nAll balances: {balances:#?}",
                );
            };
        }

        self
    }

    pub fn assert_address_balances(
        &mut self,
        address: &Addr,
        expected_balances: &[Coin],
    ) -> &mut Self {
        println!(
            "[StrategyHandler] Asserting all balances for {address} are {expected_balances:#?}"
        );
        let balances = self.harness.query_balances(address);

        for expected in expected_balances {
            let actual = balances.iter().find(|c| {
                // Allow for rounding discrepancies
                c.denom == expected.denom && c.amount.abs_diff(expected.amount) < Uint128::new(10)
            });

            if expected.amount.is_zero() {
                assert!(
                    actual.is_none(),
                    "Expected zero balance for {} but found: {actual:?}",
                    expected.denom
                );
            } else {
                assert!(
                    actual.is_some(),
                    "Expected balance not found: {expected:?}\n\nAll balances: {balances:#?}",
                );
            };
        }

        self
    }

    pub fn assert_config(&mut self, expected_config: StrategyConfig) -> &mut Self {
        println!("[StrategyHandler] Asserting strategy config is {expected_config:#?}");
        let config = self.harness.query_strategy_config(&self.strategy_addr);
        assert_eq!(
            config, expected_config,
            "Expected config does not match current config: expected {expected_config:#?}, got {config:#?}"
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

    pub fn assert_strategy_fin_orders(
        &mut self,
        pair_address: &Addr,
        expected_fin_orders: Vec<(Side, Decimal, Uint128, Uint128, Uint128)>,
    ) -> &mut Self {
        println!(
            "[StrategyHandler] Asserting strategy fin orders on pair {pair_address} are (side, price, offer, remaining, filled): {expected_fin_orders:#?}"
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
                        price: Price::Fixed(*price),
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

    pub fn assert_fin_orders(
        &mut self,
        owner: &Addr,
        pair_address: &Addr,
        expected_fin_orders: Vec<(Side, Decimal, Uint128, Uint128, Uint128)>,
    ) -> &mut Self {
        println!(
            "[StrategyHandler] Asserting fin orders owned by ${owner} on pair {pair_address} are (side, price, offer, remaining, filled): {expected_fin_orders:#?}"
        );
        let fin_orders = self
            .harness
            .get_fin_orders(pair_address, owner, None, None, None);
        assert_eq!(
            fin_orders,
            OrdersResponse {
                orders: expected_fin_orders
                    .iter()
                    .map(|(side, price, offer, remaining, filled)| OrderResponse {
                        owner: owner.to_string(),
                        side: side.clone(),
                        price: Price::Fixed(*price),
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
