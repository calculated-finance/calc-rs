#[cfg(test)]
mod integration_tests {
    use calc_rs::{
        actions::{
            distribution::{Destination, Distribution, Recipient},
            limit_order::{Direction, Offset, StaleOrder},
            swaps::fin::FinRoute,
            swaps::thor::ThorchainRoute,
        },
        conditions::CompositeCondition,
        constants::BASE_FEE_BPS,
        core::Threshold,
        scheduler::SchedulerExecuteMsg,
        strategy::Committed,
    };

    use std::{collections::HashSet, str::FromStr, time::Duration, vec};

    use calc_rs::{
        actions::{
            action::Action,
            conditional::Conditional,
            schedule::Schedule,
            swaps::swap::{Swap, SwapAmountAdjustment, SwapRoute},
        },
        cadence::Cadence,
        conditions::Condition,
        statistics::Statistics,
        strategy::{Strategy, StrategyConfig},
    };
    use cosmwasm_std::{to_json_binary, Addr, Binary, Coin, Decimal, Uint128};
    use rujira_rs::fin::{Price, Side};

    use calc_rs::actions::limit_order::{LimitOrder, OrderPriceStrategy};
    use calc_rs::manager::StrategyStatus;

    use crate::harness::CalcTestApp;
    use crate::strategy_builder::StrategyBuilder;

    // Test helpers

    fn default_swap_action(harness: &CalcTestApp) -> Swap {
        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        Swap {
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        }
    }

    fn default_swap_action_thor(harness: &CalcTestApp) -> Swap {
        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        Swap {
            routes: vec![SwapRoute::Thorchain(ThorchainRoute {
                streaming_interval: Some(2),
                max_streaming_quantity: Some(1000),
                affiliate_code: Some("rj".to_string()),
                affiliate_bps: Some(10),
                latest_swap: None,
            })],
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        }
    }

    fn default_swap_action_fin(harness: &CalcTestApp) -> Swap {
        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        Swap {
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        }
    }

    fn default_limit_order_action(harness: &CalcTestApp) -> LimitOrder {
        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        LimitOrder {
            pair_address: harness.fin_addr.clone(),
            bid_denom: fin_pair.denoms.base().to_string(),
            max_bid_amount: None,
            side: Side::Base,
            strategy: OrderPriceStrategy::Fixed(Decimal::percent(100)),
            current_order: None,
            scheduler: harness.scheduler_addr.clone(),
            execution_rebate: vec![],
        }
    }

    fn default_schedule_action(harness: &CalcTestApp) -> Schedule {
        Schedule {
            scheduler: harness.scheduler_addr.clone(),
            execution_rebate: vec![],
            cadence: Cadence::Blocks {
                interval: 5,
                previous: None,
            },
            action: Box::new(Action::LimitOrder(default_limit_order_action(harness))),
        }
    }

    fn default_conditional_action(harness: &CalcTestApp) -> Conditional {
        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        Conditional {
            action: Box::new(Action::Schedule(default_schedule_action(harness))),
            condition: Condition::StrategyBalanceAvailable {
                amount: Coin::new(1000u128, fin_pair.denoms.base()),
            },
        }
    }

    fn default_distribution_action(harness: &CalcTestApp) -> Distribution {
        Distribution {
            destinations: vec![Destination {
                recipient: Recipient::Bank {
                    address: harness.owner.clone(),
                },
                shares: Uint128::new(10_000),
                label: None,
            }],
            denoms: vec![default_swap_action(harness).swap_amount.denom.clone()],
        }
    }

    // Instantiate Strategy tests

    #[test]
    fn test_instantiate_strategy_with_single_action_succeeds() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let manager_addr = harness.manager_addr.clone();
        let owner = harness.owner.clone();

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[]);

        strategy.assert_config(StrategyConfig {
            manager: manager_addr,
            escrowed: HashSet::from([swap_action.minimum_receive_amount.denom.clone()]),
            strategy: Strategy {
                owner: owner.clone(),
                action: Action::Swap(swap_action),
                state: Committed {
                    contract_address: strategy.strategy_addr.clone(),
                },
            },
        });
    }

    #[test]
    fn test_instantiate_strategy_with_all_action_types_succeeds() {
        let mut harness = CalcTestApp::setup();

        let swap_action = default_swap_action(&harness);
        let limit_order_action = default_limit_order_action(&harness);
        let schedule_action = default_schedule_action(&harness);
        let conditional_action = default_conditional_action(&harness);
        let distribution_action = default_distribution_action(&harness);

        let many_action = Action::Many(vec![
            Action::Swap(swap_action),
            Action::LimitOrder(limit_order_action),
            Action::Schedule(schedule_action),
            Action::Conditional(conditional_action),
            Action::Distribute(distribution_action),
        ]);

        assert!(StrategyBuilder::new(&mut harness)
            .with_action(many_action)
            .try_instantiate(&[])
            .is_ok());
    }

    #[test]
    fn test_instantiate_strategy_with_nested_conditional_actions_succeeds() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let conditional_action = Conditional {
            condition: Condition::StrategyBalanceAvailable {
                amount: Coin::new(1000u128, fin_pair.denoms.base()),
            },
            action: Box::new(Action::Conditional(Conditional {
                condition: Condition::StrategyBalanceAvailable {
                    amount: Coin::new(1000u128, fin_pair.denoms.base()),
                },
                action: Box::new(Action::Swap(default_swap_action(&harness))),
            })),
        };

        assert!(StrategyBuilder::new(&mut harness)
            .with_action(Action::Conditional(conditional_action))
            .try_instantiate(&[])
            .is_ok());
    }

    #[test]
    fn test_instantiate_strategy_with_nested_schedule_actions_succeeds() {
        let mut harness = CalcTestApp::setup();

        let scheduler_addr = harness.scheduler_addr.clone();
        let nested_schedule_action = default_schedule_action(&harness);

        assert!(StrategyBuilder::new(&mut harness)
            .with_action(Action::Schedule(Schedule {
                scheduler: scheduler_addr,
                cadence: Cadence::Blocks {
                    interval: 10,
                    previous: None
                },
                execution_rebate: vec![],
                action: Box::new(Action::Schedule(nested_schedule_action)),
            }))
            .try_instantiate(&[])
            .is_ok());
    }

    #[test]
    fn test_instantiate_strategy_with_empty_many_action_fails() {
        let mut harness = CalcTestApp::setup();

        assert!(StrategyBuilder::new(&mut harness)
            .with_action(Action::Many(vec![]))
            .try_instantiate(&[])
            .is_err());
    }

    // Swap Action tests

    #[test]
    fn test_instantiate_thor_swap_action_with_zero_swap_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_thor(&harness);

        let swap_action = Swap {
            swap_amount: Coin::new(0u128, default_swap.swap_amount.denom.clone()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_thor_swap_action_with_invalid_maximum_slippage_bps_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_thor(&harness);

        let swap_action = Swap {
            maximum_slippage_bps: 10_001,
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_thor_swap_action_with_non_secured_swap_denom_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_thor(&harness);

        let swap_action = Swap {
            swap_amount: Coin::new(1000u128, "x/ruji".to_string()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_thor_swap_action_with_non_secured_receive_denom_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_thor(&harness);

        let swap_action = Swap {
            minimum_receive_amount: Coin::new(1000u128, "x/ruji".to_string()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_thor_swap_action_with_zero_streaming_interval_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_thor(&harness);

        let swap_action = Swap {
            routes: vec![SwapRoute::Thorchain(ThorchainRoute {
                streaming_interval: Some(0),
                max_streaming_quantity: Some(1000),
                affiliate_code: Some("rj".to_string()),
                affiliate_bps: Some(10),
                latest_swap: None,
            })],
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_thor_swap_action_with_too_high_streaming_interval_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_thor(&harness);

        let swap_action = Swap {
            routes: vec![SwapRoute::Thorchain(ThorchainRoute {
                streaming_interval: Some(51),
                max_streaming_quantity: Some(1000),
                affiliate_code: Some("rj".to_string()),
                affiliate_bps: Some(10),
                latest_swap: None,
            })],
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_thor_swap_action_with_invalid_max_streaming_quantity_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_thor(&harness);

        let swap_action = Swap {
            routes: vec![SwapRoute::Thorchain(ThorchainRoute {
                streaming_interval: Some(5),
                max_streaming_quantity: Some(15_000),
                affiliate_code: Some("rj".to_string()),
                affiliate_bps: Some(10),
                latest_swap: None,
            })],
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_thor_swap_action_executes_immediately() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action_thor(&harness);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount * Uint128::new(2),
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy.assert_swapped(vec![swap_action.swap_amount]);
    }

    #[test]
    fn test_execute_thor_swap_action_with_swap_amount_scaled_to_zero_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action_thor(&harness);

        let swap_action = Swap {
            adjustment: SwapAmountAdjustment::LinearScalar {
                base_receive_amount: Coin::new(
                    10u128,
                    default_swap_action.minimum_receive_amount.denom.clone(),
                ),
                minimum_swap_amount: None,
                scalar: Decimal::percent(10_000),
            },
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(vec![swap_action.swap_amount.clone()])
            .assert_stats(Statistics {
                outgoing: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_thor_swap_action_with_slippage_higher_than_maximum_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action_thor(&harness);

        let swap_action = Swap {
            maximum_slippage_bps: 99,
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy.assert_stats(Statistics {
            outgoing: vec![],
            ..Statistics::default()
        });
    }

    #[test]
    fn test_execute_thor_swap_action_with_receive_amount_lower_than_minimum_threshold_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action_thor(&harness);

        let swap_action = Swap {
            minimum_receive_amount: Coin::new(
                10000000u128,
                default_swap_action.minimum_receive_amount.denom.clone(),
            ),
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(vec![swap_action.swap_amount.clone()])
            .assert_stats(Statistics {
                outgoing: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_thor_swap_action_with_zero_balance_skips() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action_thor(&harness);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[]);

        strategy
            .execute()
            .assert_bank_balances(vec![])
            .assert_stats(Statistics {
                outgoing: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_thor_swap_action_with_less_balance_than_swap_amount_executes() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action_thor(&harness);

        let balance = Coin::new(
            swap_action.swap_amount.amount / Uint128::new(2),
            swap_action.swap_amount.denom.clone(),
        );

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[balance.clone()]);

        strategy.assert_stats(Statistics {
            outgoing: vec![balance],
            ..Statistics::default()
        });
    }

    #[test]
    fn test_execute_thor_swap_action_with_swap_amount_scaled_to_minimum_swap_amount_executes() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action_thor(&harness);
        let minimum_swap_amount = Coin::new(100u128, default_swap_action.swap_amount.denom.clone());

        let swap_action = Swap {
            adjustment: SwapAmountAdjustment::LinearScalar {
                base_receive_amount: Coin::new(
                    10u128,
                    default_swap_action.minimum_receive_amount.denom.clone(),
                ),
                minimum_swap_amount: Some(minimum_swap_amount.clone()),
                scalar: Decimal::percent(10_000),
            },
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy.assert_stats(Statistics {
            outgoing: vec![minimum_swap_amount],
            ..Statistics::default()
        });
    }

    // Swap Action tests

    #[test]
    fn test_instantiate_fin_swap_action_with_zero_swap_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_fin(&harness);

        let swap_action = Swap {
            swap_amount: Coin::new(0u128, default_swap.swap_amount.denom.clone()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_fin_swap_action_with_invalid_maximum_slippage_bps_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_fin(&harness);

        let swap_action = Swap {
            maximum_slippage_bps: 10_001,
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_fin_swap_action_with_invalid_pair_address_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_fin(&harness);

        let swap_action = Swap {
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: Addr::unchecked("not-a-fin-pair"),
            })],
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_fin_swap_action_with_mismatched_pair_and_swap_denom_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_fin(&harness);

        let swap_action = Swap {
            swap_amount: Coin::new(1000u128, "invalid-denom".to_string()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_fin_swap_action_with_mismatched_pair_and_receive_denom_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_fin(&harness);

        let swap_action = Swap {
            minimum_receive_amount: Coin::new(1000u128, "invalid-denom".to_string()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_fin_swap_action_executes_immediately() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action_fin(&harness);

        let manager_addr = harness.manager_addr.clone();
        let owner = harness.owner.clone();

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .assert_config(StrategyConfig {
                manager: manager_addr.clone(),
                escrowed: HashSet::from([swap_action.minimum_receive_amount.denom.clone()]),
                strategy: Strategy {
                    owner: owner.clone(),
                    action: Action::Swap(swap_action.clone()),
                    state: Committed {
                        contract_address: strategy.strategy_addr.clone(),
                    },
                },
            })
            .assert_bank_balances(vec![Coin::new(
                swap_action
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            )])
            .assert_stats(Statistics {
                outgoing: vec![Coin::new(
                    swap_action.swap_amount.amount,
                    swap_action.swap_amount.denom.clone(),
                )],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_fin_swap_action_with_swap_amount_scaled_to_zero_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action_fin(&harness);

        let swap_action = Swap {
            adjustment: SwapAmountAdjustment::LinearScalar {
                base_receive_amount: Coin::new(
                    10u128,
                    default_swap_action.minimum_receive_amount.denom.clone(),
                ),
                minimum_swap_amount: None,
                scalar: Decimal::percent(10_000),
            },
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(vec![swap_action.swap_amount.clone()])
            .assert_stats(Statistics {
                outgoing: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_fin_swap_action_with_slippage_higher_than_maximum_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action_fin(&harness);

        let swap_action = Swap {
            maximum_slippage_bps: 99,
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(vec![swap_action.swap_amount.clone()])
            .assert_stats(Statistics {
                outgoing: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_fin_swap_action_with_receive_amount_lower_than_minimum_threshold_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action_fin(&harness);

        let swap_action = Swap {
            minimum_receive_amount: Coin::new(
                10000000u128,
                default_swap_action.minimum_receive_amount.denom.clone(),
            ),
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(vec![swap_action.swap_amount.clone()])
            .assert_stats(Statistics {
                outgoing: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_fin_swap_action_with_zero_balance_skips() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action_fin(&harness);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[]);

        strategy
            .execute()
            .assert_bank_balances(vec![])
            .assert_stats(Statistics {
                outgoing: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_fin_swap_action_with_less_balance_than_swap_amount_executes() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action_fin(&harness);

        let balance = Coin::new(
            swap_action.swap_amount.amount / Uint128::new(2),
            swap_action.swap_amount.denom.clone(),
        );

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[balance.clone()]);

        strategy
            .execute()
            .assert_bank_balances(vec![Coin::new(
                balance.amount.mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            )])
            .assert_stats(Statistics {
                outgoing: vec![balance],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_fin_swap_action_with_swap_amount_scaled_to_minimum_swap_amount_executes() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action_fin(&harness);
        let minimum_swap_amount = Coin::new(100u128, default_swap_action.swap_amount.denom.clone());

        let swap_action = Swap {
            adjustment: SwapAmountAdjustment::LinearScalar {
                base_receive_amount: Coin::new(
                    10u128,
                    default_swap_action.minimum_receive_amount.denom.clone(),
                ),
                minimum_swap_amount: Some(minimum_swap_amount.clone()),
                scalar: Decimal::percent(10_000),
            },
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .assert_bank_balances(vec![
                Coin::new(
                    swap_action.swap_amount.amount - minimum_swap_amount.amount,
                    swap_action.swap_amount.denom.clone(),
                ),
                Coin::new(
                    minimum_swap_amount.amount.mul_floor(Decimal::percent(99)),
                    swap_action.minimum_receive_amount.denom.clone(),
                ),
            ])
            .assert_stats(Statistics {
                outgoing: vec![minimum_swap_amount],
                ..Statistics::default()
            });
    }

    // Swap Action tests

    #[test]
    fn test_instantiate_optimal_swap_action_with_zero_swap_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action(&harness);

        let swap_action = Swap {
            swap_amount: Coin::new(0u128, default_swap.swap_amount.denom.clone()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_optimal_swap_action_with_invalid_maximum_slippage_bps_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action(&harness);

        let swap_action = Swap {
            maximum_slippage_bps: 10_001,
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_optimal_swap_action_with_no_routes_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action(&harness);

        let swap_action = Swap {
            routes: vec![],
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_optimal_swap_action_immediately_executes() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount * Uint128::new(10),
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy
            .assert_bank_balance(&Coin::new(
                swap_action
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            ))
            .assert_stats(Statistics {
                outgoing: vec![swap_action.swap_amount],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_single_route_succeeds() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_route = Swap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_route.clone()))
            .instantiate(&[Coin::new(
                swap_route.swap_amount.amount * Uint128::new(10),
                swap_route.swap_amount.denom.clone(),
            )]);

        strategy
            .execute()
            .assert_bank_balance(&Coin::new(
                swap_route
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99))
                    * Uint128::new(2),
                swap_route.minimum_receive_amount.denom.clone(),
            ))
            .assert_stats(Statistics {
                outgoing: vec![Coin::new(
                    swap_route.swap_amount.amount * Uint128::new(2),
                    swap_route.swap_amount.denom.clone(),
                )],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_multiple_routes_succeeds() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_route = Swap {
            swap_amount: Coin::new(10000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![
                SwapRoute::Fin(FinRoute {
                    pair_address: harness.fin_addr.clone(),
                }),
                SwapRoute::Thorchain(ThorchainRoute {
                    streaming_interval: Some(3),
                    max_streaming_quantity: Some(100),
                    affiliate_code: None,
                    affiliate_bps: None,
                    latest_swap: None,
                }),
            ],
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_route.clone()))
            .instantiate(&[Coin::new(
                swap_route.swap_amount.amount * Uint128::new(10),
                swap_route.swap_amount.denom.clone(),
            )]);

        strategy
            .execute()
            .assert_bank_balance(&Coin::new(
                swap_route
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99))
                    * Uint128::new(2),
                swap_route.minimum_receive_amount.denom.clone(),
            ))
            .assert_stats(Statistics {
                outgoing: vec![Coin::new(
                    swap_route.swap_amount.amount * Uint128::new(2),
                    swap_route.swap_amount.denom.clone(),
                )],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_swap_amount_scaled_to_zero_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action(&harness);

        let swap_action = Swap {
            adjustment: SwapAmountAdjustment::LinearScalar {
                base_receive_amount: Coin::new(
                    10u128,
                    default_swap_action.minimum_receive_amount.denom.clone(),
                ),
                minimum_swap_amount: None,
                scalar: Decimal::percent(10_000),
            },
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(vec![swap_action.swap_amount.clone()])
            .assert_stats(Statistics {
                outgoing: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_slippage_higher_than_maximum_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action(&harness);

        let swap_action = Swap {
            maximum_slippage_bps: 99,
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(vec![swap_action.swap_amount.clone()])
            .assert_stats(Statistics {
                outgoing: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_receive_amount_lower_than_minimum_threshold_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action(&harness);

        let swap_action = Swap {
            minimum_receive_amount: Coin::new(
                10000000u128,
                default_swap_action.minimum_receive_amount.denom.clone(),
            ),
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(vec![swap_action.swap_amount.clone()])
            .assert_stats(Statistics {
                outgoing: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_zero_balance_skips() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[]);

        strategy
            .execute()
            .assert_bank_balances(vec![])
            .assert_stats(Statistics {
                outgoing: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_less_balance_than_swap_amount_executes() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let balance = Coin::new(
            swap_action.swap_amount.amount / Uint128::new(2),
            swap_action.swap_amount.denom.clone(),
        );

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[balance.clone()]);

        strategy
            .execute()
            .assert_bank_balances(vec![Coin::new(
                balance.amount.mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            )])
            .assert_stats(Statistics {
                outgoing: vec![balance],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_swap_amount_scaled_to_minimum_swap_amount_executes() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action(&harness);
        let minimum_swap_amount = Coin::new(100u128, default_swap_action.swap_amount.denom.clone());

        let swap_action = Swap {
            adjustment: SwapAmountAdjustment::LinearScalar {
                base_receive_amount: Coin::new(
                    10u128,
                    default_swap_action.minimum_receive_amount.denom.clone(),
                ),
                minimum_swap_amount: Some(minimum_swap_amount.clone()),
                scalar: Decimal::percent(10_000),
            },
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(swap_action.clone()))
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .assert_bank_balances(vec![
                Coin::new(
                    swap_action.swap_amount.amount - minimum_swap_amount.amount,
                    swap_action.swap_amount.denom.clone(),
                ),
                Coin::new(
                    minimum_swap_amount.amount.mul_floor(Decimal::percent(99)),
                    swap_action.minimum_receive_amount.denom.clone(),
                ),
            ])
            .assert_stats(Statistics {
                outgoing: vec![minimum_swap_amount],
                ..Statistics::default()
            });
    }

    // LimitOrder Action tests

    #[test]
    fn test_instantiate_limit_order_action_with_bid_amount_too_small_fails() {
        let mut harness = CalcTestApp::setup();

        let order_action = LimitOrder {
            max_bid_amount: Some(Uint128::new(999)),
            ..default_limit_order_action(&harness)
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .try_instantiate(&[Coin::new(1000000u128, order_action.bid_denom.clone())]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_limit_order_action_with_preset_current_price_fails() {
        let mut harness = CalcTestApp::setup();

        let order_action = LimitOrder {
            current_order: Some(StaleOrder {
                price: Decimal::one(),
            }),
            ..default_limit_order_action(&harness)
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .try_instantiate(&[Coin::new(1000000u128, order_action.bid_denom.clone())]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_limit_order_action_executes_immediately() {
        let mut harness = CalcTestApp::setup();
        let order_action = default_limit_order_action(&harness);
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        strategy.assert_bank_balances(vec![]).assert_fin_orders(
            &order_action.pair_address,
            vec![(
                order_action.side,
                Decimal::one(),          // price
                starting_balance.amount, // offer
                starting_balance.amount, // remaining
                Uint128::zero(),         // filled
            )],
        );
    }

    #[test]
    fn test_instantiate_limit_order_action_includes_remaining_amount_in_balances() {
        let mut harness = CalcTestApp::setup();

        let order_action = LimitOrder {
            strategy: OrderPriceStrategy::Fixed(Decimal::percent(50)),
            ..default_limit_order_action(&harness)
        };

        let starting_balance = Coin::new(1_000_000u128, order_action.bid_denom.clone());
        let pair = harness.query_fin_config(&order_action.pair_address);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));
        let remaining_amount = Uint128::new(800_000);

        strategy
            .assert_bank_balances(vec![])
            .assert_strategy_balance(&Coin::new(remaining_amount, order_action.bid_denom.clone()))
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    Decimal::percent(50),    // price
                    starting_balance.amount, // offer
                    remaining_amount,        // remaining
                    filled_amount.amount,    // filled
                )],
            );
    }

    #[test]
    fn test_execute_limit_order_action_with_fixed_price_strategy_is_idempotent() {
        let mut harness = CalcTestApp::setup();
        let order_action = default_limit_order_action(&harness);
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        strategy
            .assert_bank_balances(vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    Decimal::one(),          // price
                    starting_balance.amount, // offer
                    starting_balance.amount, // remaining
                    Uint128::zero(),         // filled
                )],
            )
            .assert_stats(Statistics::default())
            .execute()
            .assert_bank_balances(vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side,
                    Decimal::one(),          // price
                    starting_balance.amount, // offer
                    starting_balance.amount, // remaining
                    Uint128::zero(),         // filled
                )],
            )
            .assert_stats(Statistics::default());
    }

    #[test]
    fn test_execute_limit_order_action_with_fixed_price_strategy_claims_filled_amount() {
        let mut harness = CalcTestApp::setup();

        let order_action = LimitOrder {
            strategy: OrderPriceStrategy::Fixed(Decimal::percent(50)),
            ..default_limit_order_action(&harness)
        };

        let starting_balance = Coin::new(1_000_000u128, order_action.bid_denom.clone());
        let pair = harness.query_fin_config(&order_action.pair_address);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));
        let remaining_amount = Uint128::new(800_000);

        strategy
            .assert_bank_balances(vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    Decimal::percent(50),    // price
                    starting_balance.amount, // offer
                    remaining_amount,        // remaining
                    filled_amount.amount,    // filled
                )],
            )
            .assert_stats(Statistics::default())
            .execute()
            .assert_bank_balance(&filled_amount.clone())
            .assert_stats(Statistics {
                outgoing: vec![Coin::new(
                    starting_balance.amount - remaining_amount,
                    order_action.bid_denom.clone(),
                )],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_limit_order_action_sets_limit_order_filled_condition_trigger() {
        let mut harness = CalcTestApp::setup();

        let order_action = LimitOrder {
            strategy: OrderPriceStrategy::Fixed(Decimal::percent(50)),
            ..default_limit_order_action(&harness)
        };

        let starting_balance = Coin::new(1_000_000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        strategy.assert_triggers(vec![Condition::LimitOrderFilled {
            pair_address: order_action.pair_address.clone(),
            owner: strategy.strategy_addr.clone(),
            side: order_action.side.clone(),
            price: Price::Fixed(Decimal::percent(50)),
            minimum_filled_amount: None,
        }]);
    }

    #[test]
    fn test_execute_limit_order_action_with_additional_balance_deploys_it() {
        let mut harness = CalcTestApp::setup();
        let order_action = default_limit_order_action(&harness);
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        strategy
            .assert_bank_balances(vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    Decimal::one(),          // price
                    starting_balance.amount, // offer
                    starting_balance.amount, // remaining
                    Uint128::zero(),         // filled
                )],
            )
            .deposit(&[starting_balance.clone()])
            .execute()
            .assert_bank_balances(vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side,
                    Decimal::one(),                            // price
                    starting_balance.amount * Uint128::new(2), // offer
                    starting_balance.amount * Uint128::new(2), // remaining
                    Uint128::zero(),                           // filled
                )],
            );
    }

    #[test]
    fn test_execute_limit_order_action_with_new_desired_price_outside_tolerance_updates_order() {
        let mut harness = CalcTestApp::setup();
        let pair = harness.query_fin_config(&harness.fin_addr);

        let order_action = LimitOrder {
            strategy: OrderPriceStrategy::Offset {
                direction: Direction::Down,
                offset: Offset::Percent(10),
                tolerance: Offset::Exact(Decimal::percent(1)),
            },
            pair_address: harness.fin_addr.clone(),
            side: Side::Quote,
            bid_denom: pair.denoms.quote().to_string(),
            ..default_limit_order_action(&harness)
        };

        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());
        let unknown = harness.unknown.clone();

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));

        strategy.assert_bank_balances(vec![]).assert_fin_orders(
            &order_action.pair_address,
            vec![(
                order_action.side.clone(),
                Decimal::from_str("0.891").unwrap(), // price
                starting_balance.amount,             // offer
                starting_balance.amount,             // remaining
                Uint128::zero(),                     // filled
            )],
        );

        let new_order_amount = Coin::new(1_000_000u128, filled_amount.denom);

        strategy
            .harness
            .set_fin_orders(
                &unknown,
                &order_action.pair_address,
                vec![(
                    Side::Base,
                    Price::Fixed(Decimal::from_str("0.40").unwrap()),
                    Some(new_order_amount.amount),
                )],
                &[new_order_amount],
            )
            .unwrap();

        let new_offer_amount = Uint128::new(1331750);

        strategy
            .deposit(&[starting_balance.clone()])
            .execute()
            .assert_bank_balances(vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side,
                    Decimal::from_str("0.8019").unwrap(), // price
                    new_offer_amount,                     // offer
                    new_offer_amount,                     // remaining
                    Uint128::zero(),                      // filled
                )],
            );
    }

    #[test]
    fn test_execute_limit_order_action_with_new_desired_price_inside_tolerance_skips() {
        let mut harness = CalcTestApp::setup();
        let pair = harness.query_fin_config(&harness.fin_addr);

        let order_action = LimitOrder {
            strategy: OrderPriceStrategy::Offset {
                direction: Direction::Down,
                offset: Offset::Percent(10),
                tolerance: Offset::Exact(Decimal::percent(90)),
            },
            pair_address: harness.fin_addr.clone(),
            side: Side::Quote,
            bid_denom: pair.denoms.quote().to_string(),
            ..default_limit_order_action(&harness)
        };

        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());
        let unknown = harness.unknown.clone();

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        strategy.assert_bank_balances(vec![]).assert_fin_orders(
            &order_action.pair_address,
            vec![(
                order_action.side.clone(),
                Decimal::from_str("0.891").unwrap(), // price
                starting_balance.amount,             // offer
                starting_balance.amount,             // remaining
                Uint128::zero(),                     // filled
            )],
        );

        let new_order_amount = Coin::new(1_000_000u128, order_action.bid_denom);

        strategy
            .harness
            .set_fin_orders(
                &unknown,
                &order_action.pair_address,
                vec![(
                    Side::Quote,
                    Price::Fixed(Decimal::from_str("1.40").unwrap()),
                    Some(new_order_amount.amount),
                )],
                &[new_order_amount],
            )
            .unwrap();

        strategy
            .assert_bank_balances(vec![])
            .execute()
            .assert_bank_balances(vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    Decimal::from_str("0.891").unwrap(), // price
                    starting_balance.amount,             // offer
                    starting_balance.amount,             // remaining
                    Uint128::zero(),                     // filled
                )],
            );
    }

    #[test]
    fn test_withdraw_limit_order_action_with_escrowed_denoms_fails() {
        let mut harness = CalcTestApp::setup();
        let pair = harness.query_fin_config(&harness.fin_addr);

        let order_action = default_limit_order_action(&harness);
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        assert!(strategy
            .try_withdraw(HashSet::from([pair
                .denoms
                .ask(&order_action.side)
                .to_string()]))
            .is_err());
    }

    #[test]
    fn test_withdraw_limit_order_action_with_unrelated_denoms_does_nothing() {
        let mut harness = CalcTestApp::setup();
        let pair = harness.query_fin_config(&harness.fin_addr);

        let price = Decimal::percent(50);
        let order_action = LimitOrder {
            strategy: OrderPriceStrategy::Fixed(price),
            ..default_limit_order_action(&harness)
        };
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        let remaining_amount = Uint128::new(800_000);
        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));

        strategy
            .assert_bank_balances(vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    price,                   // price
                    starting_balance.amount, // offer
                    remaining_amount,        // remaining
                    filled_amount.amount,    // filled
                )],
            )
            .withdraw(HashSet::from(["not-a-denom".to_string()]))
            .assert_bank_balances(vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    price,                   // price
                    starting_balance.amount, // offer
                    remaining_amount,        // remaining
                    filled_amount.amount,    // filled
                )],
            )
            .assert_stats(Statistics::default());
    }

    #[test]
    fn test_withdraw_limit_order_action_with_filled_amount_withdraws_and_claims() {
        let mut harness = CalcTestApp::setup();
        let manager = harness.manager_addr.clone();
        let owner = harness.owner.clone();
        let pair = harness.query_fin_config(&harness.fin_addr);

        let price = Decimal::percent(50);
        let order_action = LimitOrder {
            strategy: OrderPriceStrategy::Fixed(price),
            ..default_limit_order_action(&harness)
        };
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        let contract_address = strategy.strategy_addr.clone();
        let remaining_amount = Uint128::new(800_000);
        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));

        strategy
            .assert_bank_balances(vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    price,                   // price
                    starting_balance.amount, // offer
                    remaining_amount,        // remaining
                    filled_amount.amount,    // filled
                )],
            )
            .withdraw(HashSet::from([order_action.bid_denom.clone()]))
            .assert_fin_orders(&order_action.pair_address, vec![])
            .assert_bank_balance(&filled_amount)
            .assert_bank_balance(&Coin::new(0u128, order_action.bid_denom.clone()))
            .assert_stats(Statistics {
                outgoing: vec![Coin::new(
                    starting_balance.amount - remaining_amount,
                    order_action.bid_denom.clone(),
                )],
                ..Statistics::default()
            })
            .assert_config(StrategyConfig {
                manager,
                strategy: Strategy {
                    owner,
                    // asserts that we remove the current order
                    action: Action::LimitOrder(order_action),
                    state: Committed { contract_address },
                },
                escrowed: HashSet::from([filled_amount.denom]),
            });
    }

    #[test]
    fn test_pause_limit_order_action_with_filled_amount_withdraws_and_claims() {
        let mut harness = CalcTestApp::setup();
        let pair = harness.query_fin_config(&harness.fin_addr);

        let price = Decimal::percent(50);
        let order_action = LimitOrder {
            strategy: OrderPriceStrategy::Fixed(price),
            ..default_limit_order_action(&harness)
        };
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        let remaining_amount = Uint128::new(800_000);
        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));

        strategy
            .assert_bank_balances(vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    price,                   // price
                    starting_balance.amount, // offer
                    remaining_amount,        // remaining
                    filled_amount.amount,    // filled
                )],
            )
            .pause()
            .assert_fin_orders(&order_action.pair_address, vec![])
            .assert_bank_balance(&filled_amount)
            .assert_bank_balance(&Coin::new(remaining_amount, order_action.bid_denom.clone()))
            .assert_stats(Statistics {
                outgoing: vec![Coin::new(
                    starting_balance.amount - remaining_amount,
                    order_action.bid_denom.clone(),
                )],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_resume_limit_order_action_with_bid_denom_balance_deploys_it() {
        let mut harness = CalcTestApp::setup();
        let pair = harness.query_fin_config(&harness.fin_addr);

        let price = Decimal::percent(50);
        let order_action = LimitOrder {
            strategy: OrderPriceStrategy::Fixed(price),
            ..default_limit_order_action(&harness)
        };
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::LimitOrder(order_action.clone()))
            .instantiate(&[starting_balance.clone()]);

        let remaining_amount = Uint128::new(800_000);
        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));

        strategy
            .assert_bank_balances(vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    price,                   // price
                    starting_balance.amount, // offer
                    remaining_amount,        // remaining
                    filled_amount.amount,    // filled
                )],
            )
            .pause()
            .assert_fin_orders(&order_action.pair_address, vec![])
            .assert_bank_balance(&filled_amount)
            .assert_bank_balance(&Coin::new(remaining_amount, order_action.bid_denom.clone()))
            .resume()
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side,
                    price,            // price
                    remaining_amount, // offer
                    remaining_amount, // remaining
                    Uint128::zero(),  // filled
                )],
            )
            .assert_bank_balance(&Coin::new(0u128, order_action.bid_denom.clone()));
    }

    // Many Action tests

    #[test]
    fn test_instantiate_empty_many_action_fails() {
        let mut harness = CalcTestApp::setup();

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Many(vec![]))
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_many_action_with_too_many_actions_fails() {
        let mut harness = CalcTestApp::setup();

        let actions = (1..=11)
            .into_iter()
            .map(|_| Action::LimitOrder(default_limit_order_action(&harness)))
            .collect::<Vec<_>>();

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Many(actions))
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_many_action_succeeds() {
        let mut harness = CalcTestApp::setup();
        let pair = harness.query_fin_config(&harness.fin_addr);

        let actions = vec![
            Action::Swap(default_swap_action(&harness)),
            Action::Swap(default_swap_action(&harness)),
        ];

        let manager = harness.manager_addr.clone();
        let owner = harness.owner.clone();

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Many(actions.clone()))
            .instantiate(&[]);

        strategy.assert_config(StrategyConfig {
            manager: manager,
            strategy: Strategy {
                action: Action::Many(actions),
                state: Committed {
                    contract_address: strategy.strategy_addr.clone(),
                },
                owner,
            },
            escrowed: HashSet::from([pair.denoms.quote().to_string()]),
        });
    }

    #[test]
    fn test_execute_many_action_executes_actions_in_order() {
        let mut harness = CalcTestApp::setup();
        let pair_address = harness.fin_addr.clone();

        let fin_swap_action = default_swap_action(&harness);
        let limit_order_action = default_limit_order_action(&harness);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(
                Action::Many(vec![
                    Action::Swap(fin_swap_action.clone()),
                    Action::LimitOrder(limit_order_action.clone()),
                ])
                .clone(),
            )
            .instantiate(&[fin_swap_action.swap_amount.clone()]);

        strategy
            .assert_bank_balance(&Coin::new(0u128, fin_swap_action.swap_amount.denom.clone()))
            .assert_fin_orders(&pair_address, vec![])
            .assert_stats(Statistics {
                outgoing: vec![fin_swap_action.swap_amount.clone()],
                ..Statistics::default()
            });

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(
                Action::Many(vec![
                    Action::LimitOrder(limit_order_action.clone()),
                    Action::Swap(fin_swap_action.clone()),
                ])
                .clone(),
            )
            .instantiate(&[fin_swap_action.swap_amount.clone()]);

        strategy
            .assert_bank_balance(&Coin::new(0u128, fin_swap_action.swap_amount.denom.clone()))
            .assert_fin_orders(
                &pair_address,
                vec![(
                    limit_order_action.side,
                    Decimal::one(),                     // price
                    fin_swap_action.swap_amount.amount, // offer
                    fin_swap_action.swap_amount.amount, // remaining
                    Uint128::zero(),                    // filled
                )],
            )
            .assert_stats(Statistics::default());
    }

    #[test]
    fn test_instantiate_many_action_with_nested_many_action_succeeds() {
        let mut harness = CalcTestApp::setup();
        let pair = harness.query_fin_config(&harness.fin_addr);

        let actions = vec![
            Action::Swap(default_swap_action(&harness)),
            Action::Many(vec![Action::LimitOrder(default_limit_order_action(
                &harness,
            ))]),
        ];

        let manager = harness.manager_addr.clone();
        let owner = harness.owner.clone();

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Many(actions.clone()))
            .instantiate(&[]);

        strategy.assert_config(StrategyConfig {
            manager,
            strategy: Strategy {
                action: Action::Many(actions),
                state: Committed {
                    contract_address: strategy.strategy_addr.clone(),
                },
                owner,
            },
            escrowed: HashSet::from([pair.denoms.quote().to_string()]),
        });
    }

    // Distribution Action tests

    #[test]
    fn test_instantiate_distribution_with_empty_denoms_fails() {
        let mut harness = CalcTestApp::setup();
        let distribution_action = Distribution {
            denoms: vec![],
            ..default_distribution_action(&harness)
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Distribute(distribution_action))
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_distribution_with_empty_destinations_fails() {
        let mut harness = CalcTestApp::setup();
        let distribution_action = Distribution {
            destinations: vec![],
            ..default_distribution_action(&harness)
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Distribute(distribution_action))
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_distribution_with_zero_shares_destination_fails() {
        let mut harness = CalcTestApp::setup();
        let distribution_action = Distribution {
            destinations: vec![
                Destination {
                    recipient: Recipient::Bank {
                        address: harness.owner.clone(),
                    },
                    shares: Uint128::new(10_000),
                    label: None,
                },
                Destination {
                    recipient: Recipient::Bank {
                        address: harness.owner.clone(),
                    },
                    shares: Uint128::zero(),
                    label: None,
                },
            ],
            ..default_distribution_action(&harness)
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Distribute(distribution_action))
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_distribution_with_invalid_bank_recipient_fails() {
        let mut harness = CalcTestApp::setup();
        let distribution_action = Distribution {
            destinations: vec![Destination {
                recipient: Recipient::Bank {
                    address: Addr::unchecked("test_invalid_recipient"),
                },
                shares: Uint128::new(10_000),
                label: None,
            }],
            ..default_distribution_action(&harness)
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Distribute(distribution_action))
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_distribution_with_invalid_contract_recipient_fails() {
        let mut harness = CalcTestApp::setup();
        let distribution_action = Distribution {
            destinations: vec![Destination {
                recipient: Recipient::Contract {
                    address: Addr::unchecked("test_invalid_recipient"),
                    msg: Binary::default(),
                },
                shares: Uint128::new(10_000),
                label: None,
            }],
            ..default_distribution_action(&harness)
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Distribute(distribution_action))
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_distribution_with_native_denom_and_deposit_destination_fails() {
        let mut harness = CalcTestApp::setup();
        let distribution_action = Distribution {
            destinations: vec![Destination {
                recipient: Recipient::Deposit {
                    memo: "-secure:eth-usdc".to_string(),
                },
                shares: Uint128::new(10_000),
                label: None,
            }],
            denoms: vec!["x/ruji".to_string()],
            ..default_distribution_action(&harness)
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Distribute(distribution_action))
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_distribution_with_false_strategy_recipient_fails() {
        let mut harness = CalcTestApp::setup();
        let distribution_action = Distribution {
            destinations: vec![Destination {
                recipient: Recipient::Strategy {
                    contract_address: Addr::unchecked("test_strategy"),
                },
                shares: Uint128::new(10_000),
                label: None,
            }],
            ..default_distribution_action(&harness)
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::Distribute(distribution_action))
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_distribution_with_native_denom_and_non_deposit_recipients_succeeds() {
        let mut harness = CalcTestApp::setup();
        let owner = harness.owner.clone();
        let scheduler = harness.scheduler_addr.clone();
        let fee_collector = harness.fee_collector_addr.clone();

        let default_swap_action = default_swap_action(&harness);

        let other_strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(default_swap_action.clone()))
            .instantiate(&[Coin::new(100_000u128, "x/ruji")]);

        let destinations = vec![
            Destination {
                recipient: Recipient::Strategy {
                    contract_address: other_strategy.strategy_addr.clone(),
                },
                shares: Uint128::new(5_000),
                label: None,
            },
            Destination {
                recipient: Recipient::Bank {
                    address: owner.clone(),
                },
                shares: Uint128::new(10_000),
                label: None,
            },
            Destination {
                recipient: Recipient::Contract {
                    address: scheduler.clone(),
                    msg: to_json_binary(&SchedulerExecuteMsg::Create(Condition::BlocksCompleted(
                        12,
                    )))
                    .unwrap(),
                },
                shares: Uint128::new(5_000),
                label: None,
            },
        ];

        let total_fee_applied_shares = destinations
            .iter()
            .filter(|d| match d.recipient {
                Recipient::Bank { .. } | Recipient::Contract { .. } | Recipient::Deposit { .. } => {
                    true
                }
                Recipient::Strategy { .. } => false,
            })
            .fold(Uint128::zero(), |acc, d| acc + d.shares)
            .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

        let fee_collector_destination = Destination {
            recipient: Recipient::Bank {
                address: fee_collector.clone(),
            },
            shares: total_fee_applied_shares.mul_floor(Decimal::bps(BASE_FEE_BPS)),
            label: None,
        };

        let distribution_action = Distribution {
            denoms: vec!["x/ruji".to_string()],
            destinations: destinations.clone(),
            ..default_distribution_action(&harness)
        };

        let starting_balances = vec![Coin::new(120_000u128, "x/ruji")];

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Distribute(distribution_action))
            .instantiate(&starting_balances);

        let total_shares = destinations.iter().map(|d| d.shares).sum::<Uint128>()
            + fee_collector_destination.shares;

        strategy
            .assert_bank_balance(&Coin::new(0u128, "x/ruji"))
            .assert_stats(Statistics {
                distributed: [destinations, vec![fee_collector_destination]]
                    .concat()
                    .iter()
                    .map(|d| {
                        (
                            d.recipient.clone(),
                            starting_balances
                                .iter()
                                .map(|b| {
                                    Coin::new(
                                        b.amount
                                            .mul_floor(Decimal::from_ratio(d.shares, total_shares)),
                                        b.denom.clone(),
                                    )
                                })
                                .collect(),
                        )
                    })
                    .collect::<Vec<_>>(),
                ..Statistics::default()
            });
    }

    #[test]
    fn test_instantiate_distribution_with_secured_denom_and_all_recipient_types_succeeds() {
        let mut harness = CalcTestApp::setup();
        let owner = harness.owner.clone();
        let scheduler = harness.scheduler_addr.clone();
        let fee_collector = harness.fee_collector_addr.clone();

        let default_swap_action = default_swap_action(&harness);

        let other_strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Swap(default_swap_action.clone()))
            .instantiate(&[Coin::new(100_000u128, "eth-usdc")]);

        let destinations = vec![
            Destination {
                recipient: Recipient::Strategy {
                    contract_address: other_strategy.strategy_addr.clone(),
                },
                shares: Uint128::new(5_000),
                label: None,
            },
            Destination {
                recipient: Recipient::Deposit {
                    memo: "-secure:eth-usdc".to_string(),
                },
                shares: Uint128::new(10_000),
                label: None,
            },
            Destination {
                recipient: Recipient::Bank {
                    address: owner.clone(),
                },
                shares: Uint128::new(10_000),
                label: None,
            },
            Destination {
                recipient: Recipient::Contract {
                    address: scheduler.clone(),
                    msg: to_json_binary(&SchedulerExecuteMsg::Create(Condition::BlocksCompleted(
                        12,
                    )))
                    .unwrap(),
                },
                shares: Uint128::new(5_000),
                label: None,
            },
        ];

        let mut total_fee_applied_shares = Uint128::zero();

        for destination in destinations.iter() {
            match &destination.recipient {
                Recipient::Bank { .. } | Recipient::Contract { .. } | Recipient::Deposit { .. } => {
                    total_fee_applied_shares += destination.shares;
                }
                Recipient::Strategy { .. } => {}
            }
        }

        let fee_collector_destination = Destination {
            recipient: Recipient::Bank {
                address: fee_collector.clone(),
            },
            shares: total_fee_applied_shares.mul_ceil(Decimal::bps(BASE_FEE_BPS)),
            label: None,
        };

        let distribution_action = Distribution {
            denoms: vec!["eth-usdc".to_string()],
            destinations: destinations.clone(),
            ..default_distribution_action(&harness)
        };

        let starting_balances = vec![Coin::new(100_000u128, "eth-usdc")];

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Distribute(distribution_action.clone()))
            .instantiate(&starting_balances);

        let total_shares = destinations.iter().map(|d| d.shares).sum::<Uint128>()
            + fee_collector_destination.shares;

        strategy.assert_stats(Statistics {
            distributed: [destinations, vec![fee_collector_destination]]
                .concat()
                .iter()
                .map(|d| {
                    (
                        d.recipient.clone(),
                        starting_balances
                            .iter()
                            .map(|b| {
                                Coin::new(
                                    b.amount
                                        .mul_floor(Decimal::from_ratio(d.shares, total_shares)),
                                    b.denom.clone(),
                                )
                            })
                            .collect(),
                    )
                })
                .collect::<Vec<_>>(),
            ..Statistics::default()
        });
        // TODO: Enable when MsgDeposit mock handler moves bank funds
        // .assert_balance(&Coin::new(0u128, "eth-usdc"));
    }

    #[test]
    fn test_execute_distribution_multiple_times_accumulates_statistics() {
        let mut harness = CalcTestApp::setup();
        let owner = harness.owner.clone();
        let fee_collector = harness.fee_collector_addr.clone();

        let destinations = vec![Destination {
            recipient: Recipient::Bank {
                address: owner.clone(),
            },
            shares: Uint128::new(10_000),
            label: None,
        }];

        let total_fee_applied_shares = destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares)
            .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

        let fee_collector_destination = Destination {
            recipient: Recipient::Bank {
                address: fee_collector.clone(),
            },
            shares: total_fee_applied_shares.mul_floor(Decimal::bps(BASE_FEE_BPS)),
            label: None,
        };

        let distribution_action = Distribution {
            denoms: vec!["x/ruji".to_string()],
            destinations: destinations.clone(),
            ..default_distribution_action(&harness)
        };

        let starting_balances = vec![Coin::new(100_000u128, "x/ruji")];

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Distribute(distribution_action))
            .instantiate(&starting_balances);

        let total_shares = destinations.iter().map(|d| d.shares).sum::<Uint128>()
            + fee_collector_destination.shares;

        strategy
            .deposit(&starting_balances.clone())
            .execute()
            .assert_bank_balance(&Coin::new(1u128, "x/ruji")) // just dust
            .assert_stats(Statistics {
                distributed: [destinations, vec![fee_collector_destination]]
                    .concat()
                    .iter()
                    .map(|d| {
                        (
                            d.recipient.clone(),
                            starting_balances
                                .iter()
                                .map(|b| {
                                    Coin::new(
                                        (b.amount * Uint128::new(2))
                                            .mul_floor(Decimal::from_ratio(d.shares, total_shares)),
                                        b.denom.clone(),
                                    )
                                })
                                .collect(),
                        )
                    })
                    .collect::<Vec<_>>(),
                ..Statistics::default()
            });
    }

    // Conditional Action tests

    #[test]
    fn test_instantiate_conditional_action_with_too_many_nested_actions_fails() {
        let mut harness = CalcTestApp::setup();

        let nested_actions = (1..=11)
            .into_iter()
            .map(|_| Action::LimitOrder(default_limit_order_action(&harness)))
            .collect::<Vec<_>>();

        let action = Action::Conditional(Conditional {
            condition: Condition::StrategyBalanceAvailable {
                amount: Coin::new(1000u128, "x/ruji"),
            },
            action: Box::new(Action::Many(nested_actions)),
        });

        let result = StrategyBuilder::new(&mut harness)
            .with_action(action)
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_conditional_action_with_too_many_conditions_fails() {
        let mut harness = CalcTestApp::setup();

        let action = Action::Conditional(Conditional {
            condition: Condition::Composite(CompositeCondition {
                conditions: vec![
                    Condition::StrategyBalanceAvailable {
                        amount: Coin::new(1000u128, "x/ruji"),
                    };
                    20
                ],
                threshold: Threshold::All,
            }),
            action: Box::new(Action::Swap(default_swap_action(&harness))),
        });

        let result = StrategyBuilder::new(&mut harness)
            .with_action(action)
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_conditional_action_succeeds() {
        let mut harness = CalcTestApp::setup();
        let manager = harness.manager_addr.clone();
        let owner = harness.owner.clone();

        let action = Action::Conditional(Conditional {
            condition: Condition::StrategyBalanceAvailable {
                amount: Coin::new(1000u128, "x/ruji"),
            },
            action: Box::new(Action::Swap(default_swap_action(&harness))),
        });

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(action.clone())
            .instantiate(&[]);

        strategy.assert_config(StrategyConfig {
            manager: manager,
            strategy: Strategy {
                owner: owner,
                action: action,
                state: Committed {
                    contract_address: strategy.strategy_addr.clone(),
                },
            },
            escrowed: HashSet::from(["eth-usdc".to_string()]),
        });
    }

    #[test]
    fn test_execute_conditional_action_with_satisfied_conditions_executes_action() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = Swap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
        };

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount + Uint128::one(),
            fin_pair.denoms.base(),
        )];

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::StrategyBalanceAvailable {
                    amount: swap_action.swap_amount.clone(),
                },
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds);

        strategy.assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_conditional_action_with_unsatisfied_conditions_skips_action() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = Swap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
        };

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount + Uint128::one(),
            fin_pair.denoms.base(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::StrategyBalanceAvailable {
                    amount: Coin::new(
                        swap_action.swap_amount.amount * Uint128::new(2),
                        fin_pair.denoms.base(),
                    ),
                },
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![]);
    }

    #[test]
    fn test_execute_condition_action_respects_timestamp_elapsed_condition() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = Swap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
        };

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount + Uint128::one(),
            fin_pair.denoms.base(),
        )];

        let block_time = harness.app.block_info().time;

        StrategyBuilder::new(&mut harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::TimestampElapsed(block_time.plus_seconds(60)),
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![])
            .advance_time(61)
            .execute()
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_condition_action_respects_block_elapsed_condition() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = Swap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
        };

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount + Uint128::one(),
            fin_pair.denoms.base(),
        )];

        let block_height = harness.app.block_info().height;

        StrategyBuilder::new(&mut harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::BlocksCompleted(block_height + 60),
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![])
            .advance_blocks(61)
            .execute()
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_condition_action_respects_can_swap_condition() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = default_swap_action(&harness);

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount + Uint128::one(),
            fin_pair.denoms.base(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::CanSwap(Swap {
                    minimum_receive_amount: Coin::new(20_000_000u128, fin_pair.denoms.quote()),
                    ..swap_action.clone()
                }),
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![]);

        StrategyBuilder::new(&mut harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::CanSwap(Swap {
                    minimum_receive_amount: Coin::new(20u128, fin_pair.denoms.quote()),
                    ..swap_action.clone()
                }),
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_condition_action_respects_balance_available_condition() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = Swap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
        };

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount + Uint128::one(),
            fin_pair.denoms.base(),
        )];

        let random = harness.app.api().addr_make("random");
        let owner = harness.owner.clone();

        StrategyBuilder::new(&mut harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::BalanceAvailable {
                    address: random,
                    amount: Coin::new(1u128, fin_pair.denoms.base()),
                },
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![]);

        StrategyBuilder::new(&mut harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::BalanceAvailable {
                    address: owner,
                    amount: Coin::new(1u128, fin_pair.denoms.base()),
                },
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_condition_action_respects_strategy_balance_available_condition() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = Swap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
        };

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount + Uint128::one(),
            fin_pair.denoms.base(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::StrategyBalanceAvailable {
                    amount: Coin::new(1u128, fin_pair.denoms.base()),
                },
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&[])
            .assert_swapped(vec![]);

        StrategyBuilder::new(&mut harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::StrategyBalanceAvailable {
                    amount: funds[0].clone(),
                },
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_condition_action_respects_strategy_status_condition() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = Swap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
        };

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount + Uint128::one(),
            fin_pair.denoms.base(),
        )];

        let manager = harness.manager_addr.clone();
        let strategy_action = Action::Swap(default_swap_action(&harness));

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(strategy_action)
            .instantiate(&[Coin::new(100_000u128, "x/ruji")]);

        StrategyBuilder::new(&mut strategy.harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::StrategyStatus {
                    manager_contract: manager.clone(),
                    contract_address: strategy.strategy_addr.clone(),
                    status: StrategyStatus::Archived,
                },
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![]);

        StrategyBuilder::new(&mut strategy.harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::StrategyStatus {
                    manager_contract: manager,
                    contract_address: strategy.strategy_addr.clone(),
                    status: StrategyStatus::Active,
                },
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_condition_action_respects_not_condition() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = Swap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
        };

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount + Uint128::one(),
            fin_pair.denoms.base(),
        )];

        let manager = harness.manager_addr.clone();
        let strategy_action = Action::Swap(default_swap_action(&harness));

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(strategy_action)
            .instantiate(&[Coin::new(100_000u128, "x/ruji")]);

        StrategyBuilder::new(&mut strategy.harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::StrategyStatus {
                    manager_contract: manager.clone(),
                    contract_address: strategy.strategy_addr.clone(),
                    status: StrategyStatus::Archived,
                },
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![]);

        StrategyBuilder::new(&mut strategy.harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::Not(Box::new(Condition::StrategyStatus {
                    manager_contract: manager,
                    contract_address: strategy.strategy_addr.clone(),
                    status: StrategyStatus::Archived,
                })),
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_condition_action_respects_composite_condition_threshold() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = Swap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
        };

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount + Uint128::one(),
            fin_pair.denoms.base(),
        )];

        let manager = harness.manager_addr.clone();
        let strategy_action = Action::Swap(default_swap_action(&harness));

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(strategy_action)
            .instantiate(&[Coin::new(100_000u128, "x/ruji")]);

        StrategyBuilder::new(&mut strategy.harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::Composite(CompositeCondition {
                    conditions: vec![
                        Condition::StrategyStatus {
                            manager_contract: manager.clone(),
                            contract_address: strategy.strategy_addr.clone(),
                            status: StrategyStatus::Archived,
                        },
                        Condition::StrategyBalanceAvailable {
                            amount: funds[0].clone(),
                        },
                    ],
                    threshold: Threshold::All,
                }),
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![]);

        StrategyBuilder::new(&mut strategy.harness)
            .with_action(Action::Conditional(Conditional {
                condition: Condition::Composite(CompositeCondition {
                    conditions: vec![
                        Condition::StrategyStatus {
                            manager_contract: manager.clone(),
                            contract_address: strategy.strategy_addr.clone(),
                            status: StrategyStatus::Archived,
                        },
                        Condition::StrategyBalanceAvailable {
                            amount: funds[0].clone(),
                        },
                    ],
                    threshold: Threshold::Any,
                }),
                action: Box::new(Action::Swap(swap_action.clone())),
            }))
            .instantiate(&funds)
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    // Schedule Action tests

    #[test]
    fn test_instantiate_schedule_action_with_invalid_cron_expression_fails() {
        let mut harness = CalcTestApp::setup();

        let action = Action::Schedule(Schedule {
            action: Box::new(Action::Swap(default_swap_action(&harness))),
            scheduler: Addr::unchecked("scheduler"),
            cadence: Cadence::Cron {
                expr: "invalid cron".to_string(),
                previous: None,
            },
            execution_rebate: vec![],
        });

        let result = StrategyBuilder::new(&mut harness)
            .with_action(action)
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_schedule_action_with_too_many_nested_actions_fails() {
        let mut harness = CalcTestApp::setup();

        let nested_actions = (1..=11)
            .into_iter()
            .map(|_| Action::LimitOrder(default_limit_order_action(&harness)))
            .collect::<Vec<_>>();

        let action = Action::Schedule(Schedule {
            action: Box::new(Action::Many(nested_actions)),
            scheduler: Addr::unchecked("scheduler"),
            cadence: Cadence::Cron {
                expr: "invalid cron".to_string(),
                previous: None,
            },
            execution_rebate: vec![],
        });

        let result = StrategyBuilder::new(&mut harness)
            .with_action(action)
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_schedule_action_with_time_cadence_due_executes_immediately() {
        let mut harness = CalcTestApp::setup();

        let swap_action = default_swap_action(&harness);

        let action = Action::Schedule(Schedule {
            action: Box::new(Action::Swap(swap_action.clone())),
            scheduler: harness.scheduler_addr.clone(),
            cadence: Cadence::Time {
                duration: Duration::from_secs(60),
                previous: None,
            },
            execution_rebate: vec![],
        });

        let funds = vec![Coin::new(
            100_000u128,
            swap_action.swap_amount.denom.clone(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_action(action.clone())
            .instantiate(&funds)
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_instantiate_schedule_action_with_time_cadence_not_due_executes_immediately() {
        let mut harness = CalcTestApp::setup();

        let swap_action = default_swap_action(&harness);

        let action = Action::Schedule(Schedule {
            action: Box::new(Action::Swap(swap_action.clone())),
            scheduler: harness.scheduler_addr.clone(),
            cadence: Cadence::Time {
                duration: Duration::from_secs(60),
                previous: Some(harness.app.block_info().time),
            },
            execution_rebate: vec![],
        });

        let funds = vec![Coin::new(
            100_000u128,
            swap_action.swap_amount.denom.clone(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_action(action.clone())
            .instantiate(&funds)
            .assert_swapped(vec![])
            .advance_time(61)
            .execute_triggers()
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_instantiate_schedule_action_with_block_cadence_due_executes_immediately() {
        let mut harness = CalcTestApp::setup();

        let swap_action = default_swap_action(&harness);

        let action = Action::Schedule(Schedule {
            action: Box::new(Action::Swap(swap_action.clone())),
            scheduler: harness.scheduler_addr.clone(),
            cadence: Cadence::Blocks {
                interval: 60,
                previous: None,
            },
            execution_rebate: vec![],
        });

        let funds = vec![Coin::new(
            100_000u128,
            swap_action.swap_amount.denom.clone(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_action(action.clone())
            .instantiate(&funds)
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_instantiate_schedule_action_with_block_cadence_not_due_executes_immediately() {
        let mut harness = CalcTestApp::setup();

        let swap_action = default_swap_action(&harness);

        let action = Action::Schedule(Schedule {
            action: Box::new(Action::Swap(swap_action.clone())),
            scheduler: harness.scheduler_addr.clone(),
            cadence: Cadence::Blocks {
                interval: 60,
                previous: Some(harness.app.block_info().height),
            },
            execution_rebate: vec![],
        });

        let funds = vec![Coin::new(
            100_000u128,
            swap_action.swap_amount.denom.clone(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_action(action.clone())
            .instantiate(&funds)
            .assert_swapped(vec![])
            .advance_blocks(61)
            .execute_triggers()
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_instantiate_schedule_action_with_cron_cadence_due_executes_immediately() {
        let mut harness = CalcTestApp::setup();

        let swap_action = default_swap_action(&harness);

        let action = Action::Schedule(Schedule {
            action: Box::new(Action::Swap(swap_action.clone())),
            scheduler: harness.scheduler_addr.clone(),
            cadence: Cadence::Cron {
                expr: "0 0 * * * *".to_string(),
                previous: None,
            },
            execution_rebate: vec![],
        });

        let funds = vec![Coin::new(
            100_000u128,
            swap_action.swap_amount.denom.clone(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_action(action.clone())
            .instantiate(&funds)
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_instantiate_schedule_action_with_cron_cadence_not_due_executes_immediately() {
        let mut harness = CalcTestApp::setup();

        let swap_action = default_swap_action(&harness);

        let action = Action::Schedule(Schedule {
            action: Box::new(Action::Swap(swap_action.clone())),
            scheduler: harness.scheduler_addr.clone(),
            cadence: Cadence::Cron {
                expr: "0 0 * * * *".to_string(),
                previous: Some(harness.app.block_info().time),
            },
            execution_rebate: vec![],
        });

        let funds = vec![Coin::new(
            100_000u128,
            swap_action.swap_amount.denom.clone(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_action(action.clone())
            .instantiate(&funds)
            .assert_swapped(vec![])
            .advance_time(3601)
            .assert_swapped(vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_crank_time_schedule_sets_and_resets_triggers() {
        let mut harness = CalcTestApp::setup();

        let swap_action = default_swap_action(&harness);

        let action = Action::Schedule(Schedule {
            action: Box::new(Action::Swap(swap_action.clone())),
            scheduler: harness.scheduler_addr.clone(),
            cadence: Cadence::Time {
                duration: Duration::from_secs(60),
                previous: Some(harness.app.block_info().time),
            },
            execution_rebate: vec![],
        });

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount * Uint128::new(20),
            swap_action.swap_amount.denom.clone(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_action(action.clone())
            .instantiate(&funds)
            .assert_swapped(vec![])
            .advance_time(62)
            .advance_time(62)
            .advance_time(62)
            .advance_time(62)
            .advance_time(62)
            .assert_swapped(vec![Coin::new(
                swap_action.swap_amount.amount * Uint128::new(5),
                swap_action.swap_amount.denom.clone(),
            )]);
    }

    #[test]
    fn test_schedule_action_without_execution_rebate_balance_skips() {
        let mut harness = CalcTestApp::setup();

        let swap_action = default_swap_action(&harness);

        let action = Action::Schedule(Schedule {
            action: Box::new(Action::Swap(swap_action.clone())),
            scheduler: harness.scheduler_addr.clone(),
            cadence: Cadence::Time {
                duration: Duration::from_secs(60),
                previous: Some(harness.app.block_info().time),
            },
            execution_rebate: vec![Coin::new(1u128, "x/ruji")],
        });

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount * Uint128::new(20),
            swap_action.swap_amount.denom.clone(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_action(action.clone())
            .instantiate(&funds)
            .advance_time(62)
            .advance_time(62)
            .advance_time(62)
            .advance_time(62)
            .advance_time(62)
            .assert_swapped(vec![]);
    }

    #[test]
    fn test_schedule_action_deposits_execution_rebate() {
        let mut harness = CalcTestApp::setup();

        let swap_action = default_swap_action(&harness);

        let action = Action::Schedule(Schedule {
            action: Box::new(Action::Swap(swap_action.clone())),
            scheduler: harness.scheduler_addr.clone(),
            cadence: Cadence::Time {
                duration: Duration::from_secs(60),
                previous: Some(harness.app.block_info().time),
            },
            execution_rebate: vec![Coin::new(1u128, "x/ruji")],
        });

        let funds = vec![
            Coin::new(
                swap_action.swap_amount.amount * Uint128::new(20),
                swap_action.swap_amount.denom.clone(),
            ),
            Coin::new(10u128, "x/ruji"),
        ];

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_action(action.clone())
            .instantiate(&funds);

        let keeper = strategy.keeper.clone();

        strategy
            .assert_bank_balance(&Coin::new(10u128, "x/ruji"))
            .advance_time(62)
            .advance_time(62)
            .advance_time(62)
            .advance_time(62)
            .advance_time(62)
            .assert_swapped(vec![Coin::new(
                swap_action.swap_amount.amount * Uint128::new(5),
                swap_action.swap_amount.denom.clone(),
            )])
            .assert_bank_balance(&Coin::new(5u128, "x/ruji"));

        let keeper_balance = strategy
            .harness
            .app
            .wrap()
            .query_balance(keeper, "x/ruji")
            .unwrap();

        assert_eq!(keeper_balance, Coin::new(5u128, "x/ruji"));
    }
}
