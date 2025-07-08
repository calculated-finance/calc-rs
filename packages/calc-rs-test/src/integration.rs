#[cfg(test)]
mod integration_tests {
    use calc_rs::{actions::thor_swap::ThorSwap, core::ContractError};
    use std::{collections::HashSet, time::Duration, vec};

    use calc_rs::{
        actions::{
            action::Action,
            conditional::Conditional,
            fin_swap::FinSwap,
            optimal_swap::{OptimalSwap, SwapAmountAdjustment, SwapRoute},
            schedule::Schedule,
        },
        cadence::Cadence,
        conditions::{Condition, Threshold},
        statistics::Statistics,
        strategy::{Idle, Strategy, StrategyConfig},
    };
    use cosmwasm_std::{Addr, Coin, Decimal, Uint128};
    use rujira_rs::fin::Side;

    use calc_rs::actions::limit_order::{LimitOrder, OrderPriceStrategy};
    use calc_rs::manager::StrategyStatus;

    use crate::harness::CalcTestApp;
    use crate::strategy_builder::StrategyBuilder;

    // Instantiate Strategy tests

    #[test]
    fn test_instantiate_strategy_with_single_action_succeeds() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = OptimalSwap {
            routes: vec![SwapRoute::Fin(harness.fin_addr.clone())],
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 50,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let manager_addr = harness.manager_addr.clone();
        let owner = harness.owner.clone();

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .instantiate(&[]);

        strategy_handler.assert_config(StrategyConfig {
            manager: manager_addr,
            escrowed: HashSet::from([swap_action.minimum_receive_amount.denom.clone()]),
            strategy: Strategy {
                owner: owner.clone(),
                action: Action::OptimalSwap(swap_action),
                state: Idle {
                    contract_address: strategy_handler.strategy_addr.clone(),
                },
            },
        });
    }

    #[test]
    fn test_instantiate_strategy_with_all_action_types_succeeds() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = OptimalSwap {
            routes: vec![SwapRoute::Fin(harness.fin_addr.clone())],
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 50,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let limit_order_action = LimitOrder {
            pair_address: harness.fin_addr.clone(),
            bid_denom: fin_pair.denoms.base().to_string(),
            bid_amount: None,
            side: Side::Base,
            strategy: OrderPriceStrategy::Fixed(Decimal::percent(100)),
            current_price: None,
            scheduler: harness.scheduler_addr.clone(),
            execution_rebate: vec![],
        };

        let schedule_action = Schedule {
            scheduler: harness.scheduler_addr.clone(),
            execution_rebate: vec![],
            cadence: Cadence::Blocks {
                interval: 5,
                previous: None,
            },
            action: Box::new(Action::SetLimitOrder(limit_order_action.clone())),
        };

        let conditional_action = Conditional {
            action: Box::new(Action::Schedule(schedule_action.clone())),
            conditions: vec![Condition::StrategyBalanceAvailable {
                amount: Coin::new(1000u128, fin_pair.denoms.base()),
            }],
            threshold: Threshold::All,
        };

        let fin_swap_action = FinSwap {
            pair_address: harness.fin_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let thor_swap_action = ThorSwap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 50,
            adjustment: SwapAmountAdjustment::Fixed,
            streaming_interval: None,
            max_streaming_quantity: None,
            affiliate_code: None,
            affiliate_bps: None,
            previous_swap: None,
        };

        let many_action = Action::Many(vec![
            Action::OptimalSwap(swap_action),
            Action::FinSwap(fin_swap_action),
            Action::ThorSwap(thor_swap_action),
            Action::Schedule(schedule_action),
            Action::Conditional(conditional_action),
        ]);

        let strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(many_action)
            .instantiate(&[]);

        println!("{:#?}", strategy_handler.config());
    }

    #[test]
    fn test_instantiate_strategy_with_nested_conditional_actions_succeeds() {}

    #[test]
    fn test_instantiate_strategy_with_nested_schedule_actions_succeeds() {}

    #[test]
    fn test_instantiate_strategy_with_empty_many_action_fails() {}

    // ThorSwap Action tests

    #[test]
    fn test_instantiate_thor_swap_action_with_zero_swap_amount_fails() {}

    #[test]
    fn test_instantiate_thor_swap_action_with_invalid_maximum_slippage_bps_amount_fails() {}

    #[test]
    fn test_instantiate_thor_swap_action_with_non_secured_swap_denom_fails() {}

    #[test]
    fn test_instantiate_thor_swap_action_with_non_secured_receive_denom_fails() {}

    #[test]
    fn test_instantiate_thor_swap_action_with_invalid_streaming_interval_fails() {}

    #[test]
    fn test_instantiate_thor_swap_action_with_invalid_max_streaming_quantity_fails() {}

    #[test]
    fn test_instantiate_thor_swap_action_succeeds() {}

    // FinSwap Action tests

    fn default_fin_swap(pair_address: Addr) -> FinSwap {
        let fin_pair = CalcTestApp::setup().query_fin_config(&pair_address);
        FinSwap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            pair_address: pair_address.clone(),
            adjustment: SwapAmountAdjustment::Fixed,
        }
    }

    #[test]
    fn test_instantiate_fin_swap_action_with_zero_swap_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_fin_swap(harness.fin_addr.clone());

        let swap_action = FinSwap {
            swap_amount: Coin::new(0u128, default_swap.swap_amount.denom.clone()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::FinSwap(swap_action.clone()))
            .try_instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_fin_swap_action_with_invalid_maximum_slippage_bps_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_fin_swap(harness.fin_addr.clone());

        let swap_action = FinSwap {
            maximum_slippage_bps: 10_001,
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::FinSwap(swap_action.clone()))
            .try_instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_fin_swap_action_with_invalid_pair_address_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_fin_swap(harness.fin_addr.clone());

        let swap_action = FinSwap {
            pair_address: Addr::unchecked("not-a-fin-pair"),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::FinSwap(swap_action.clone()))
            .try_instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_fin_swap_action_with_mismatched_pair_and_swap_denom_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_fin_swap(harness.fin_addr.clone());

        let swap_action = FinSwap {
            swap_amount: Coin::new(1000u128, "invalid-denom".to_string()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::FinSwap(swap_action.clone()))
            .try_instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_fin_swap_action_with_mismatched_pair_and_receive_denom_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_fin_swap(harness.fin_addr.clone());

        let swap_action = FinSwap {
            minimum_receive_amount: Coin::new(1000u128, "invalid-denom".to_string()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::FinSwap(swap_action.clone()))
            .try_instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_fin_swap_action_executes_immediately() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_fin_swap(harness.fin_addr.clone());

        let manager_addr = harness.manager_addr.clone();
        let owner = harness.owner.clone();

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::FinSwap(swap_action.clone()))
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .assert_config(StrategyConfig {
                manager: manager_addr.clone(),
                escrowed: HashSet::from([swap_action.minimum_receive_amount.denom.clone()]),
                strategy: Strategy {
                    owner: owner.clone(),
                    action: Action::FinSwap(swap_action.clone()),
                    state: Idle {
                        contract_address: strategy_handler.strategy_addr.clone(),
                    },
                },
            })
            .assert_balances(vec![Coin::new(
                swap_action
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            )])
            .assert_stats(Statistics {
                swapped: vec![Coin::new(
                    swap_action.swap_amount.amount,
                    swap_action.swap_amount.denom.clone(),
                )],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_fin_swap_action_with_swap_amount_scaled_to_zero_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_fin_swap(harness.fin_addr.clone());

        let swap_action = FinSwap {
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

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::FinSwap(swap_action.clone()))
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .execute()
            .assert_balances(vec![Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )])
            .assert_stats(Statistics {
                swapped: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_fin_swap_action_with_slippage_higher_than_maximum_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_fin_swap(harness.fin_addr.clone());

        let swap_action = FinSwap {
            maximum_slippage_bps: 99,
            ..default_swap_action
        };

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::FinSwap(swap_action.clone()))
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .execute()
            .assert_balances(vec![Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )])
            .assert_stats(Statistics {
                swapped: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_fin_swap_action_with_receive_amount_lower_than_minimum_threshold_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_fin_swap(harness.fin_addr.clone());

        let swap_action = FinSwap {
            minimum_receive_amount: Coin::new(
                10000000u128,
                default_swap_action.minimum_receive_amount.denom.clone(),
            ),
            ..default_swap_action
        };

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::FinSwap(swap_action.clone()))
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .execute()
            .assert_balances(vec![Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )])
            .assert_stats(Statistics {
                swapped: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_fin_swap_action_with_zero_balance_skips() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_fin_swap(harness.fin_addr.clone());

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::FinSwap(swap_action.clone()))
            .instantiate(&[]);

        strategy_handler
            .execute()
            .assert_balances(vec![])
            .assert_stats(Statistics {
                swapped: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_fin_swap_action_with_less_balance_than_swap_amount_executes() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_fin_swap(harness.fin_addr.clone());

        let balance = Coin::new(
            swap_action.swap_amount.amount / Uint128::new(2),
            swap_action.swap_amount.denom.clone(),
        );

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::FinSwap(swap_action.clone()))
            .instantiate(&[balance.clone()]);

        strategy_handler
            .execute()
            .assert_balances(vec![Coin::new(
                balance.amount.mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            )])
            .assert_stats(Statistics {
                swapped: vec![balance],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_fin_swap_action_with_swap_amount_scaled_to_minimum_swap_amount_executes() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_fin_swap(harness.fin_addr.clone());
        let minimum_swap_amount = Coin::new(100u128, default_swap_action.swap_amount.denom.clone());

        let swap_action = FinSwap {
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

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::FinSwap(swap_action.clone()))
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .assert_balances(vec![
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
                swapped: vec![minimum_swap_amount],
                ..Statistics::default()
            });
    }

    // OptimalSwap Action tests

    fn default_optimal_swap(pair_address: Addr) -> OptimalSwap {
        let fin_pair = CalcTestApp::setup().query_fin_config(&pair_address);
        OptimalSwap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(pair_address.clone())],
        }
    }

    #[test]
    fn test_instantiate_optimal_swap_action_with_zero_swap_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_optimal_swap(harness.fin_addr.clone());

        let swap_action = OptimalSwap {
            swap_amount: Coin::new(0u128, default_swap.swap_amount.denom.clone()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .try_instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_optimal_swap_action_with_invalid_maximum_slippage_bps_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_optimal_swap(harness.fin_addr.clone());

        let swap_action = OptimalSwap {
            maximum_slippage_bps: 10_001,
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .try_instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_optimal_swap_action_with_no_routes_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_optimal_swap(harness.fin_addr.clone());

        let swap_action = OptimalSwap {
            routes: vec![],
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .try_instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_optimal_swap_action_immediately_executes() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_optimal_swap(harness.fin_addr.clone());

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount * Uint128::new(10),
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .assert_balance(Coin::new(
                swap_action
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            ))
            .assert_stats(Statistics {
                swapped: vec![swap_action.swap_amount],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_single_route_succeeds() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_route = OptimalSwap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(harness.fin_addr.clone())],
        };

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_route.clone()))
            .instantiate(&[Coin::new(
                swap_route.swap_amount.amount * Uint128::new(10),
                swap_route.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .execute()
            .assert_balance(Coin::new(
                swap_route
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99))
                    * Uint128::new(2),
                swap_route.minimum_receive_amount.denom.clone(),
            ))
            .assert_stats(Statistics {
                swapped: vec![Coin::new(
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

        let swap_route = OptimalSwap {
            swap_amount: Coin::new(10000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![
                SwapRoute::Fin(harness.fin_addr.clone()),
                SwapRoute::Thorchain {
                    streaming_interval: Some(3),
                    max_streaming_quantity: Some(100),
                    affiliate_code: None,
                    affiliate_bps: None,
                    previous_swap: None,
                },
            ],
        };

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_route.clone()))
            .instantiate(&[Coin::new(
                swap_route.swap_amount.amount * Uint128::new(10),
                swap_route.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .execute()
            .assert_balance(Coin::new(
                swap_route
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99))
                    * Uint128::new(2),
                swap_route.minimum_receive_amount.denom.clone(),
            ))
            .assert_stats(Statistics {
                swapped: vec![Coin::new(
                    swap_route.swap_amount.amount * Uint128::new(2),
                    swap_route.swap_amount.denom.clone(),
                )],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_swap_amount_scaled_to_zero_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_optimal_swap(harness.fin_addr.clone());

        let swap_action = OptimalSwap {
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

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .execute()
            .assert_balances(vec![Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )])
            .assert_stats(Statistics {
                swapped: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_slippage_higher_than_maximum_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_optimal_swap(harness.fin_addr.clone());

        let swap_action = OptimalSwap {
            maximum_slippage_bps: 99,
            ..default_swap_action
        };

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .execute()
            .assert_balances(vec![Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )])
            .assert_stats(Statistics {
                swapped: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_receive_amount_lower_than_minimum_threshold_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_optimal_swap(harness.fin_addr.clone());

        let swap_action = OptimalSwap {
            minimum_receive_amount: Coin::new(
                10000000u128,
                default_swap_action.minimum_receive_amount.denom.clone(),
            ),
            ..default_swap_action
        };

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .execute()
            .assert_balances(vec![Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )])
            .assert_stats(Statistics {
                swapped: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_zero_balance_skips() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_optimal_swap(harness.fin_addr.clone());

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .instantiate(&[]);

        strategy_handler
            .execute()
            .assert_balances(vec![])
            .assert_stats(Statistics {
                swapped: vec![],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_less_balance_than_swap_amount_executes() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_optimal_swap(harness.fin_addr.clone());

        let balance = Coin::new(
            swap_action.swap_amount.amount / Uint128::new(2),
            swap_action.swap_amount.denom.clone(),
        );

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .instantiate(&[balance.clone()]);

        strategy_handler
            .execute()
            .assert_balances(vec![Coin::new(
                balance.amount.mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            )])
            .assert_stats(Statistics {
                swapped: vec![balance],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_optimal_swap_action_with_swap_amount_scaled_to_minimum_swap_amount_executes() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_optimal_swap(harness.fin_addr.clone());
        let minimum_swap_amount = Coin::new(100u128, default_swap_action.swap_amount.denom.clone());

        let swap_action = OptimalSwap {
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

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .assert_balances(vec![
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
                swapped: vec![minimum_swap_amount],
                ..Statistics::default()
            });
    }

    // LimitOrder Action tests

    #[test]
    fn test_instantiate_limit_order_action_with_bid_amount_too_small_fails() {}

    #[test]
    fn test_instantiate_limit_order_action_with_preset_current_price_fails() {}

    #[test]
    fn test_instantiate_limit_order_action_succeeds() {}

    // Many Action tests

    #[test]
    fn test_instantiate_empty_many_action_fails() {}

    #[test]
    fn test_instantiate_many_action_with_too_many_actions_fails() {}

    #[test]
    fn test_instantiate_many_action_succeeds() {}

    // FundStrategy Action tests

    #[test]
    fn test_instantiate_fund_strategy_with_empty_denoms_fails() {}

    #[test]
    fn test_instantiate_fund_strategy_with_non_strategy_destination_fails() {}

    #[test]
    fn test_instantiate_fund_strategy_succeeds() {}

    // Distribution Action tests

    #[test]
    fn test_instantiate_distribution_with_empty_denoms_fails() {}

    #[test]
    fn test_instantiate_distribution_with_empty_destinations_fails() {}

    #[test]
    fn test_instantiate_distribution_with_zero_shares_destination_fails() {}

    #[test]
    fn test_instantiate_distribution_with_invalid_destination_address_fails() {}

    #[test]
    fn test_instantiate_distribution_with_native_denom_and_deposit_destination_fails() {}

    #[test]
    fn test_instantiate_distribution_succeeds() {}

    // Conditional Action tests

    #[test]
    fn test_instantiate_conditional_action_with_empty_conditions_fails() {}

    #[test]
    fn test_instantiate_conditional_action_with_too_many_nested_actions_fails() {}

    #[test]
    fn test_instantiate_conditional_action_succeeds() {}

    // Schedule Action tests

    #[test]
    fn test_instantiate_schedule_action_with_invalid_cron_expression_fails() {}

    #[test]
    fn test_instantiate_schedule_action_with_too_many_nested_actions_fails() {}

    #[test]
    fn test_execute_simple_swap_strategy_updates_balances_and_stats() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_route = OptimalSwap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(harness.fin_addr.clone())],
        };

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_route.clone()))
            .instantiate(&[Coin::new(
                swap_route.swap_amount.amount * Uint128::new(10),
                swap_route.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .assert_balance(Coin::new(
                swap_route
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99)),
                swap_route.minimum_receive_amount.denom.clone(),
            ))
            .assert_stats(Statistics {
                swapped: vec![swap_route.swap_amount],
                ..Statistics::default()
            });
    }

    #[test]
    fn test_execute_strategy_with_unsatisfied_condition_does_nothing() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = OptimalSwap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(harness.fin_addr.clone())],
        };

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount - Uint128::one(),
            fin_pair.denoms.base(),
        )];

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::Conditional(Conditional {
                conditions: vec![Condition::StrategyBalanceAvailable {
                    amount: swap_action.swap_amount.clone(),
                }],
                threshold: Threshold::All,
                action: Box::new(Action::OptimalSwap(swap_action.clone())),
            }))
            .instantiate(&funds);

        strategy_handler
            .assert_balances(funds)
            .assert_stats(Statistics::default());
    }

    #[test]
    fn test_pause_strategy_cancels_open_limit_orders() {
        let mut harness = CalcTestApp::setup();

        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        let order_price = Decimal::one();

        let order_action = LimitOrder {
            pair_address: harness.fin_addr.clone(),
            side: Side::Base,
            bid_denom: fin_pair.denoms.base().to_string(),
            bid_amount: Some(Uint128::new(1000u128)),
            strategy: OrderPriceStrategy::Fixed(order_price),
            current_price: None,
            scheduler: harness.scheduler_addr.clone(),
            execution_rebate: vec![],
        };

        let manager_addr = harness.manager_addr.clone();
        let owner = harness.owner.clone();

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::SetLimitOrder(order_action.clone()))
            .instantiate(&[Coin::new(
                order_action.bid_amount.unwrap(),
                order_action.bid_denom.clone(),
            )]);

        let strategy_addr = strategy_handler.strategy_addr.clone();

        strategy_handler
            .assert_config(StrategyConfig {
                manager: manager_addr.clone(),
                escrowed: HashSet::from([fin_pair.denoms.quote().to_string()]),
                strategy: Strategy {
                    owner: owner.clone(),
                    action: Action::SetLimitOrder(LimitOrder {
                        current_price: Some(order_price),
                        ..order_action.clone()
                    }),
                    state: Idle {
                        contract_address: strategy_addr.clone(),
                    },
                },
            })
            .assert_status(StrategyStatus::Active)
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    order_price,
                    order_action.bid_amount.unwrap(),
                    order_action.bid_amount.unwrap(),
                    Uint128::zero(),
                )],
            )
            .pause()
            .assert_config(StrategyConfig {
                manager: manager_addr,
                escrowed: HashSet::from([fin_pair.denoms.quote().to_string()]),
                strategy: Strategy {
                    owner: owner.clone(),
                    action: Action::SetLimitOrder(order_action.clone()),
                    state: Idle {
                        contract_address: strategy_addr,
                    },
                },
            })
            .assert_status(StrategyStatus::Paused)
            .assert_fin_orders(&order_action.pair_address, vec![]);
    }

    #[test]
    fn test_resume_strategy_re_executes_and_places_orders() {
        let mut harness = CalcTestApp::setup();

        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        let order_price = Decimal::one();

        let order_action = LimitOrder {
            pair_address: harness.fin_addr.clone(),
            side: Side::Base,
            bid_denom: fin_pair.denoms.base().to_string(),
            bid_amount: Some(Uint128::new(1000u128)),
            strategy: OrderPriceStrategy::Fixed(order_price),
            current_price: None,
            scheduler: harness.scheduler_addr.clone(),
            execution_rebate: vec![],
        };

        let manager_addr = harness.manager_addr.clone();
        let owner = harness.owner.clone();

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::SetLimitOrder(order_action.clone()))
            .instantiate(&[Coin::new(
                order_action.bid_amount.unwrap().u128(),
                order_action.bid_denom.clone(),
            )]);

        let strategy_addr = strategy_handler.strategy_addr.clone();

        strategy_handler
            .assert_config(StrategyConfig {
                manager: manager_addr.clone(),
                escrowed: HashSet::from([fin_pair.denoms.quote().to_string()]),
                strategy: Strategy {
                    owner: owner.clone(),
                    action: Action::SetLimitOrder(LimitOrder {
                        current_price: Some(order_price),
                        ..order_action.clone()
                    }),
                    state: Idle {
                        contract_address: strategy_addr.clone(),
                    },
                },
            })
            .assert_status(StrategyStatus::Active)
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    order_price,
                    order_action.bid_amount.unwrap(),
                    order_action.bid_amount.unwrap(),
                    Uint128::zero(),
                )],
            )
            .pause()
            .assert_config(StrategyConfig {
                manager: manager_addr.clone(),
                escrowed: HashSet::from([fin_pair.denoms.quote().to_string()]),
                strategy: Strategy {
                    owner: owner.clone(),
                    action: Action::SetLimitOrder(order_action.clone()),
                    state: Idle {
                        contract_address: strategy_addr.clone(),
                    },
                },
            })
            .assert_fin_orders(&order_action.pair_address, vec![])
            .resume()
            .assert_config(StrategyConfig {
                manager: manager_addr,
                escrowed: HashSet::from([fin_pair.denoms.quote().to_string()]),
                strategy: Strategy {
                    owner: owner.clone(),
                    action: Action::SetLimitOrder(LimitOrder {
                        current_price: Some(order_price),
                        ..order_action.clone()
                    }),
                    state: Idle {
                        contract_address: strategy_addr.clone(),
                    },
                },
            })
            .assert_status(StrategyStatus::Active)
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    order_price,
                    order_action.bid_amount.unwrap(),
                    order_action.bid_amount.unwrap(),
                    Uint128::zero(),
                )],
            );
    }

    #[test]
    fn test_schedule_action_with_blocks_cadence_schedules_correctly() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        let scheduler_addr = harness.scheduler_addr.clone();

        let swap_action = FinSwap {
            pair_address: harness.fin_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let scheduled_swap_action = Action::Schedule(Schedule {
            scheduler: scheduler_addr.clone(),
            cadence: Cadence::Blocks {
                interval: 5,
                previous: None,
            },
            execution_rebate: vec![],
            action: Box::new(Action::FinSwap(swap_action.clone())),
        });

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(scheduled_swap_action)
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount * Uint128::new(10),
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .assert_balance(Coin::new(
                swap_action.swap_amount.amount * Uint128::new(9),
                swap_action.swap_amount.denom.clone(),
            ))
            .assert_balance(Coin::new(
                swap_action
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            ))
            .assert_swapped(vec![swap_action.swap_amount.clone()])
            .advance_blocks(2)
            .assert_swapped(vec![swap_action.swap_amount.clone()])
            .advance_blocks(4)
            .assert_balances(vec![
                Coin::new(
                    swap_action.swap_amount.amount * Uint128::new(8),
                    swap_action.swap_amount.denom.clone(),
                ),
                Coin::new(
                    swap_action
                        .swap_amount
                        .amount
                        .mul_floor(Decimal::percent(99))
                        * Uint128::new(2),
                    swap_action.minimum_receive_amount.denom.clone(),
                ),
            ])
            .assert_swapped(vec![Coin::new(
                swap_action.swap_amount.amount * Uint128::new(2),
                swap_action.swap_amount.denom.clone(),
            )]);
    }

    #[test]
    fn test_schedule_action_with_time_duration_cadence_schedules_correctly() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        let scheduler_addr = harness.scheduler_addr.clone();

        let swap_action = FinSwap {
            pair_address: harness.fin_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let scheduled_swap_action = Action::Schedule(Schedule {
            scheduler: scheduler_addr.clone(),
            cadence: Cadence::Time {
                duration: Duration::from_secs(5),
                previous: None,
            },
            execution_rebate: vec![],
            action: Box::new(Action::FinSwap(swap_action.clone())),
        });

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(scheduled_swap_action)
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount * Uint128::new(10),
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .assert_balance(Coin::new(
                swap_action.swap_amount.amount * Uint128::new(9),
                swap_action.swap_amount.denom.clone(),
            ))
            .assert_balance(Coin::new(
                swap_action
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            ))
            .assert_swapped(vec![swap_action.swap_amount.clone()])
            .advance_time(2)
            .assert_swapped(vec![swap_action.swap_amount.clone()])
            .advance_time(4)
            .assert_balances(vec![
                Coin::new(
                    swap_action.swap_amount.amount * Uint128::new(8),
                    swap_action.swap_amount.denom.clone(),
                ),
                Coin::new(
                    swap_action
                        .swap_amount
                        .amount
                        .mul_floor(Decimal::percent(99))
                        * Uint128::new(2),
                    swap_action.minimum_receive_amount.denom.clone(),
                ),
            ])
            .assert_swapped(vec![Coin::new(
                swap_action.swap_amount.amount * Uint128::new(2),
                swap_action.swap_amount.denom.clone(),
            )]);
    }

    #[test]
    fn test_schedule_action_with_cron_cadence_schedules_correctly() {
        let mut harness = CalcTestApp::setup();
        let scheduler_addr = harness.scheduler_addr.clone();

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = FinSwap {
            pair_address: harness.fin_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let schedule_action = Action::Schedule(Schedule {
            scheduler: scheduler_addr.clone(),
            cadence: Cadence::Cron {
                expr: "*/10 * * * * *".to_string(),
                previous: None,
            },
            execution_rebate: vec![],
            action: Box::new(Action::FinSwap(swap_action.clone())),
        });

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(schedule_action)
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount * Uint128::new(10),
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .assert_swapped(vec![swap_action.swap_amount.clone()])
            .advance_time(2)
            .assert_swapped(vec![swap_action.swap_amount.clone()])
            .advance_time(10)
            .assert_swapped(vec![Coin::new(
                swap_action.swap_amount.amount * Uint128::new(2),
                swap_action.swap_amount.denom.clone(),
            )]);
    }

    #[test]
    fn test_all_conditions_action_only_executes_when_all_satisfied() {
        let mut harness = CalcTestApp::setup();

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = FinSwap {
            pair_address: harness.fin_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let conditional = Action::Conditional(Conditional {
            conditions: vec![
                Condition::StrategyBalanceAvailable {
                    amount: swap_action.swap_amount.clone(),
                },
                Condition::BlocksCompleted(harness.app.block_info().height + 5),
            ],
            threshold: Threshold::All,
            action: Box::new(Action::FinSwap(swap_action.clone())),
        });

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(conditional)
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount * Uint128::new(10),
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .execute()
            .assert_swapped(vec![])
            .advance_blocks(2)
            .execute()
            .assert_swapped(vec![])
            .advance_blocks(10)
            .execute()
            .assert_swapped(vec![Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )]);
    }

    #[test]
    fn test_any_conditions_action_always_executes_when_any_satisfied() {
        let mut harness = CalcTestApp::setup();

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = FinSwap {
            pair_address: harness.fin_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let conditional = Action::Conditional(Conditional {
            conditions: vec![
                Condition::StrategyBalanceAvailable {
                    amount: swap_action.swap_amount.clone(),
                },
                Condition::BlocksCompleted(harness.app.block_info().height + 5),
            ],
            threshold: Threshold::Any,
            action: Box::new(Action::FinSwap(swap_action.clone())),
        });

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(conditional)
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount * Uint128::new(10),
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy_handler
            .assert_swapped(vec![Coin::new(
                swap_action.swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            )])
            .execute()
            .assert_swapped(vec![Coin::new(
                swap_action.swap_amount.amount * Uint128::new(2),
                swap_action.swap_amount.denom.clone(),
            )])
            .advance_blocks(10)
            .execute()
            .assert_swapped(vec![Coin::new(
                swap_action.swap_amount.amount * Uint128::new(3),
                swap_action.swap_amount.denom.clone(),
            )]);
    }

    #[test]
    fn test_update_strategy_from_unauthorized_sender_fails() {
        let mut harness = CalcTestApp::setup();
        let unauthorized_sender = harness.app.api().addr_make("unauthorized");

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = OptimalSwap {
            routes: vec![SwapRoute::Fin(harness.fin_addr.clone())],
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 50,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .instantiate(&[]);

        let res = strategy_handler.harness.update_strategy_status(
            &unauthorized_sender,
            &strategy_handler.strategy_addr,
            StrategyStatus::Paused,
        );

        assert!(res.is_err());
        let err = res.unwrap_err();
        assert_eq!(
            err.source().unwrap().to_string(),
            ContractError::Unauthorized {}.to_string()
        );

        strategy_handler.assert_status(StrategyStatus::Active);
    }

    #[test]
    fn test_withdraw_from_unauthorized_sender_fails() {
        let mut harness = CalcTestApp::setup();
        let unauthorized_sender = harness.app.api().addr_make("unauthorized");

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = OptimalSwap {
            routes: vec![SwapRoute::Fin(harness.fin_addr.clone())],
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 50,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let funds_to_send = &[Coin::new(1000u128, fin_pair.denoms.base())];

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .instantiate(funds_to_send);

        assert_eq!(
            strategy_handler
                .withdraw(&unauthorized_sender, funds_to_send)
                .unwrap_err()
                .source()
                .unwrap()
                .to_string(),
            ContractError::Unauthorized {}.to_string()
        );
    }

    #[test]
    fn test_withdraw_escrowed_funds_fails() {
        let mut harness = CalcTestApp::setup();

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = OptimalSwap {
            routes: vec![SwapRoute::Fin(harness.fin_addr.clone())],
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let funds_to_send = &[swap_action.swap_amount.clone()];
        let owner = harness.owner.clone();

        let mut strategy_handler = StrategyBuilder::new(&mut harness)
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .instantiate(funds_to_send);

        let res = strategy_handler.withdraw(&owner, &[swap_action.minimum_receive_amount.clone()]);

        assert_eq!(
            res.unwrap_err().source().unwrap().to_string(),
            format!(
                "Generic error: Cannot withdraw escrowed denom: {}",
                swap_action.minimum_receive_amount.denom
            )
        );
    }

    #[test]
    fn test_instantiate_with_invalid_cron_string_fails() {
        let mut harness = CalcTestApp::setup();

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = FinSwap {
            pair_address: harness.fin_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let schedule_action = Action::Schedule(Schedule {
            scheduler: harness.scheduler_addr.clone(),
            cadence: Cadence::Cron {
                expr: "invalid cron string".to_string(),
                previous: None,
            },
            execution_rebate: vec![],
            action: Box::new(Action::FinSwap(swap_action.clone())),
        });

        let result = StrategyBuilder::new(&mut harness)
            .with_action(schedule_action.clone())
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_with_deep_recursion_fails() {
        let mut harness = CalcTestApp::setup();

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = FinSwap {
            pair_address: harness.fin_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let mut nested_behaviour = Action::FinSwap(swap_action.clone());

        for _ in 0..11 {
            nested_behaviour = Action::Conditional(Conditional {
                conditions: vec![],
                threshold: Threshold::All,
                action: Box::new(nested_behaviour),
            });
        }

        let result = StrategyBuilder::new(&mut harness)
            .with_action(nested_behaviour.clone())
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_strategy_with_no_action_fails() {}

    #[test]
    fn test_instantiate_strategy_with_empty_label_fails() {}

    #[test]
    fn test_instantiate_strategy_with_maximum_actions_succeeds() {}

    #[test]
    fn test_instantiate_strategy_exceeding_maximum_actions_fails() {}

    #[test]
    fn test_instantiate_strategy_with_zero_funds_succeeds() {}

    #[test]
    fn test_execute_strategy_with_no_funds_does_nothing() {}

    #[test]
    fn test_execute_strategy_with_multiple_nested_conditionals() {}

    #[test]
    fn test_withdraw_all_funds_succeeds() {}

    #[test]
    fn test_withdraw_escrowed_denom_fails() {}

    #[test]
    fn test_pause_already_paused_strategy_is_idempotent() {}

    #[test]
    fn test_resume_already_active_strategy_is_idempotent() {}

    #[test]
    fn test_strategy_with_multiple_destinations_distributes_correctly() {}

    #[test]
    fn test_strategy_with_multiple_affiliates_distributes_correctly() {}

    #[test]
    fn test_strategy_with_invalid_affiliate_bps_fails() {}

    #[test]
    fn test_strategy_with_schedule_and_invalid_cadence_fails() {}

    #[test]
    fn test_strategy_with_schedule_and_valid_cadence_succeeds() {}

    #[test]
    fn test_strategy_with_many_actions_executes_all() {}

    #[test]
    fn test_strategy_with_many_actions_and_one_fails_aborts_all() {}

    #[test]
    fn test_strategy_with_empty_distribution_destinations_fails() {}

    #[test]
    fn test_strategy_with_action_that_requires_external_contract_fails_gracefully() {}
}
