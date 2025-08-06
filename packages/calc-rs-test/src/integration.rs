#[cfg(test)]
mod integration_tests {
    use calc_rs::{
        actions::{
            distribution::{Destination, Distribution, Recipient},
            limit_orders::fin_limit_order::{Direction, Offset, StaleOrder},
            swaps::{fin::FinRoute, thor::ThorchainRoute},
        },
        cadence::Cadence,
        conditions::{condition::Condition, schedule::Schedule},
        constants::BASE_FEE_BPS,
        manager::{Affiliate, StrategyStatus},
        scheduler::{CreateTriggerMsg, SchedulerExecuteMsg},
        strategy::Node,
    };

    use std::{collections::HashSet, str::FromStr, time::Duration, vec};

    use calc_rs::{
        actions::{
            action::Action,
            swaps::swap::{Swap, SwapAmountAdjustment, SwapRoute},
        },
        strategy::StrategyConfig,
    };
    use cosmwasm_std::{to_json_binary, Addr, Binary, Coin, Coins, Decimal, Timestamp, Uint128};
    use rujira_rs::fin::{Price, Side};

    use calc_rs::actions::limit_orders::fin_limit_order::{FinLimitOrder, PriceStrategy};

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

    fn default_limit_order_action(harness: &CalcTestApp) -> FinLimitOrder {
        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        FinLimitOrder {
            pair_address: harness.fin_addr.clone(),
            bid_denom: fin_pair.denoms.base().to_string(),
            bid_amount: None,
            side: Side::Base,
            strategy: PriceStrategy::Fixed(Decimal::percent(100)),
            current_order: None,
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[]);

        strategy.assert_config(StrategyConfig {
            manager: manager_addr,
            owner: owner.clone(),
            nodes: vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }],
            denoms: HashSet::from([
                swap_action.swap_amount.denom.clone(),
                swap_action.minimum_receive_amount.denom.clone(),
            ]),
        });
    }

    #[test]
    fn test_instantiate_strategy_with_all_node_types_succeeds() {
        let mut harness = CalcTestApp::setup();

        let swap_action = default_swap_action(&harness);
        let limit_order_action = default_limit_order_action(&harness);
        let distribution_action = default_distribution_action(&harness);

        let nodes = vec![
            Node::Action {
                action: Action::Swap(swap_action),
                index: 0,
                next: Some(1),
            },
            Node::Condition {
                condition: Condition::TimestampElapsed(Timestamp::from_seconds(1)),
                index: 1,
                on_success: 2,
                on_failure: Some(3),
            },
            Node::Condition {
                condition: Condition::BlocksCompleted(1),
                index: 2,
                on_success: 3,
                on_failure: Some(4),
            },
            Node::Condition {
                condition: Condition::Schedule(Schedule {
                    scheduler: harness.scheduler_addr.clone(),
                    contract_address: harness.manager_addr.clone(),
                    msg: Some(Binary::default()),
                    cadence: Cadence::Blocks {
                        interval: 1,
                        previous: None,
                    },
                    execution_rebate: vec![],
                    executors: vec![],
                    jitter: None,
                    next: None,
                }),
                index: 3,
                on_success: 4,
                on_failure: Some(5),
            },
            Node::Condition {
                condition: Condition::CanSwap(Swap {
                    swap_amount: Coin::new(1000u128, "rune"),
                    minimum_receive_amount: Coin::new(1u128, "rune"),
                    maximum_slippage_bps: 100,
                    adjustment: SwapAmountAdjustment::Fixed,
                    routes: vec![SwapRoute::Fin(FinRoute {
                        pair_address: harness.fin_addr.clone(),
                    })],
                }),
                index: 4,
                on_success: 5,
                on_failure: Some(6),
            },
            Node::Condition {
                condition: Condition::FinLimitOrderFilled {
                    owner: None,
                    pair_address: harness.fin_addr.clone(),
                    side: Side::Base,
                    price: Decimal::percent(100),
                },
                index: 5,
                on_success: 6,
                on_failure: Some(7),
            },
            Node::Condition {
                condition: Condition::BalanceAvailable {
                    address: None,
                    amount: Coin::new(1000u128, "rune"),
                },
                index: 6,
                on_success: 7,
                on_failure: Some(8),
            },
            Node::Condition {
                condition: Condition::StrategyStatus {
                    manager_contract: harness.manager_addr.clone(),
                    contract_address: Addr::unchecked("strategy"),
                    status: StrategyStatus::Active,
                },
                index: 7,
                on_success: 9,
                on_failure: Some(10),
            },
            Node::Condition {
                condition: Condition::OraclePrice {
                    asset: "rune".to_string(),
                    direction: Direction::Above,
                    price: Decimal::percent(100),
                },
                index: 8,
                on_success: 10,
                on_failure: Some(10),
            },
            Node::Action {
                action: Action::LimitOrder(limit_order_action),
                index: 9,
                next: Some(10),
            },
            Node::Action {
                action: Action::Distribute(distribution_action),
                index: 10,
                next: None,
            },
        ];

        assert!(StrategyBuilder::new(&mut harness)
            .with_nodes(nodes)
            .try_instantiate(&[])
            .is_ok());
    }

    #[test]
    fn test_instantiate_strategy_with_empty_nodes_succeeds() {
        assert!(StrategyBuilder::new(&mut CalcTestApp::setup())
            .with_nodes(vec![])
            .try_instantiate(&[])
            .is_ok());
    }

    #[test]
    fn instantiate_strategy_with_cyclic_graph_fails() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let nodes = vec![
            Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: Some(1),
            },
            Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 1,
                next: Some(0),
            },
        ];

        assert!(StrategyBuilder::new(&mut harness)
            .with_nodes(nodes)
            .try_instantiate(&[])
            .is_err());
    }

    #[test]
    fn test_instantiate_strategy_with_out_of_bounds_next_index_fails() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let nodes = vec![Node::Action {
            action: Action::Swap(swap_action.clone()),
            index: 0,
            next: Some(1),
        }];

        assert!(StrategyBuilder::new(&mut harness)
            .with_nodes(nodes)
            .try_instantiate(&[])
            .is_err());
    }

    #[test]
    fn test_instantiate_strategy_with_out_of_bounds_on_success_index_fails() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let nodes = vec![
            Node::Condition {
                condition: Condition::TimestampElapsed(Timestamp::from_seconds(1)),
                index: 0,
                on_success: 2,
                on_failure: Some(1),
            },
            Node::Action {
                action: Action::Swap(swap_action),
                index: 1,
                next: None,
            },
        ];

        assert!(StrategyBuilder::new(&mut harness)
            .with_nodes(nodes)
            .try_instantiate(&[])
            .is_err());
    }

    #[test]
    fn test_instantiate_strategy_with_out_of_bounds_on_failure_index_fails() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let nodes = vec![
            Node::Condition {
                condition: Condition::TimestampElapsed(Timestamp::from_seconds(1)),
                index: 0,
                on_success: 1,
                on_failure: Some(2),
            },
            Node::Action {
                action: Action::Swap(swap_action),
                index: 1,
                next: None,
            },
        ];

        assert!(StrategyBuilder::new(&mut harness)
            .with_nodes(nodes)
            .try_instantiate(&[])
            .is_err());
    }

    #[test]
    fn test_instantiate_strategy_with_mismatched_index_fails() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let nodes = vec![Node::Action {
            action: Action::Swap(swap_action.clone()),
            index: 1,
            next: None,
        }];

        assert!(StrategyBuilder::new(&mut harness)
            .with_nodes(nodes)
            .try_instantiate(&[])
            .is_err());
    }

    #[test]
    fn test_instantiate_strategy_with_affiliate_fee_too_high_fails() {
        let harness = CalcTestApp::setup();
        let address = harness.app.api().addr_make("affiliate");
        let action = Action::Swap(default_swap_action(&harness));

        assert!(StrategyBuilder::new(&mut CalcTestApp::setup())
            .with_nodes(vec![Node::Action {
                action: action.clone(),
                index: 0,
                next: None,
            }])
            .try_instantiate_with_affiliates(
                vec![Affiliate {
                    label: "Bad actor".to_string(),
                    address: address.clone(),
                    bps: 201
                }],
                &[]
            )
            .is_err());

        assert!(StrategyBuilder::new(&mut CalcTestApp::setup())
            .with_nodes(vec![Node::Action {
                action: action.clone(),
                index: 0,
                next: None,
            }])
            .try_instantiate_with_affiliates(
                vec![Affiliate {
                    label: "Less bad actor".to_string(),
                    address: address.clone(),
                    bps: 200
                }],
                &[]
            )
            .is_ok());

        assert!(StrategyBuilder::new(&mut CalcTestApp::setup())
            .with_nodes(vec![Node::Action {
                action: action.clone(),
                index: 0,
                next: None,
            }])
            .try_instantiate_with_affiliates(
                vec![Affiliate {
                    label: "Good actor".to_string(),
                    address: address.clone(),
                    bps: 20
                }],
                &[]
            )
            .is_ok());

        assert!(StrategyBuilder::new(&mut CalcTestApp::setup())
            .with_nodes(vec![Node::Action {
                action: action.clone(),
                index: 0,
                next: None,
            }])
            .try_instantiate_with_affiliates(
                vec![Affiliate {
                    label: "Weird actor".to_string(),
                    address,
                    bps: 0
                }],
                &[]
            )
            .is_ok());
    }

    // Thorchain Swap Action tests

    #[test]
    fn test_instantiate_thor_swap_action_with_zero_swap_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_thor(&harness);

        let swap_action = Swap {
            swap_amount: Coin::new(0u128, default_swap.swap_amount.denom.clone()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_execute_thor_swap_action_with_zero_balance_succeeds() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action_thor(&harness);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[]);

        strategy.execute().assert_bank_balances(&vec![]);
    }

    #[test]
    fn test_instantiate_thor_swap_action_executes_immediately() {
        // TODO: implement when deposit msg balance changes implemented in test harness
    }

    #[test]
    fn test_execute_thor_swap_action_with_swap_amount_scaled_to_zero_skips() {
        // TODO: implement when deposit msg balance changes implemented in test harness
    }

    #[test]
    fn test_execute_thor_swap_action_with_slippage_higher_than_maximum_skips() {
        // TODO: implement when deposit msg balance changes implemented in test harness
    }

    #[test]
    fn test_execute_thor_swap_action_with_receive_amount_lower_than_minimum_threshold_skips() {
        // TODO: implement when deposit msg balance changes implemented in test harness
    }

    #[test]
    fn test_execute_thor_swap_action_with_less_balance_than_swap_amount_executes() {
        // TODO: implement when deposit msg balance changes implemented in test harness
    }

    #[test]
    fn test_execute_thor_swap_action_with_swap_amount_scaled_to_minimum_swap_amount_executes() {
        // TODO: implement when deposit msg balance changes implemented in test harness
    }

    // FIN Swap Action tests

    #[test]
    fn test_instantiate_fin_swap_action_with_zero_swap_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action_fin(&harness);

        let swap_action = Swap {
            swap_amount: Coin::new(0u128, default_swap.swap_amount.denom.clone()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .assert_config(StrategyConfig {
                manager: manager_addr.clone(),
                owner: owner.clone(),
                nodes: vec![Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 0,
                    next: None,
                }],
                denoms: HashSet::from([
                    swap_action.swap_amount.denom.clone(),
                    swap_action.minimum_receive_amount.denom.clone(),
                ]),
            })
            .assert_bank_balances(&vec![Coin::new(
                swap_action
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            )]);
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(&vec![swap_action.swap_amount.clone()]);
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(&vec![swap_action.swap_amount.clone()]);
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(&vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_fin_swap_action_with_zero_balance_skips() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action_fin(&harness);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[]);

        strategy.execute().assert_bank_balances(&vec![]);
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[balance.clone()]);

        strategy.execute().assert_bank_balances(&vec![Coin::new(
            balance.amount.mul_floor(Decimal::percent(99)),
            swap_action.minimum_receive_amount.denom.clone(),
        )]);
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy.assert_bank_balances(&vec![
            Coin::new(
                swap_action.swap_amount.amount - minimum_swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            ),
            Coin::new(
                minimum_swap_amount.amount.mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            ),
        ]);
    }

    // Swap Action tests

    #[test]
    fn test_instantiate_swap_action_with_zero_swap_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action(&harness);

        let swap_action = Swap {
            swap_amount: Coin::new(0u128, default_swap.swap_amount.denom.clone()),
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_swap_action_with_invalid_maximum_slippage_bps_amount_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action(&harness);

        let swap_action = Swap {
            maximum_slippage_bps: 10_001,
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_swap_action_with_no_routes_fails() {
        let mut harness = CalcTestApp::setup();
        let default_swap = default_swap_action(&harness);

        let swap_action = Swap {
            routes: vec![],
            ..default_swap
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .try_instantiate(&[swap_action.swap_amount.clone()]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_swap_action_immediately_executes() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[Coin::new(
                swap_action.swap_amount.amount * Uint128::new(10),
                swap_action.swap_amount.denom.clone(),
            )]);

        strategy.assert_bank_balance(&Coin::new(
            swap_action
                .swap_amount
                .amount
                .mul_floor(Decimal::percent(99)),
            swap_action.minimum_receive_amount.denom.clone(),
        ));
    }

    #[test]
    fn test_execute_swap_action_with_single_route_succeeds() {
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_route.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[Coin::new(
                swap_route.swap_amount.amount * Uint128::new(10),
                swap_route.swap_amount.denom.clone(),
            )]);

        strategy.execute().assert_bank_balance(&Coin::new(
            swap_route
                .swap_amount
                .amount
                .mul_floor(Decimal::percent(99))
                * Uint128::new(2),
            swap_route.minimum_receive_amount.denom.clone(),
        ));
    }

    #[test]
    fn test_execute_swap_action_with_multiple_routes_succeeds() {
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_route.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[Coin::new(
                swap_route.swap_amount.amount * Uint128::new(10),
                swap_route.swap_amount.denom.clone(),
            )]);

        strategy.execute().assert_bank_balance(&Coin::new(
            swap_route
                .swap_amount
                .amount
                .mul_floor(Decimal::percent(99))
                * Uint128::new(2),
            swap_route.minimum_receive_amount.denom.clone(),
        ));
    }

    #[test]
    fn test_execute_swap_action_with_swap_amount_scaled_to_zero_skips() {
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(&vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_swap_action_with_slippage_higher_than_maximum_skips() {
        let mut harness = CalcTestApp::setup();
        let default_swap_action = default_swap_action(&harness);

        let swap_action = Swap {
            maximum_slippage_bps: 99,
            ..default_swap_action
        };

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(&vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_swap_action_with_receive_amount_lower_than_minimum_threshold_skips() {
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy
            .execute()
            .assert_bank_balances(&vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_swap_action_with_zero_balance_skips() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[]);

        strategy.execute().assert_bank_balances(&vec![]);
    }

    #[test]
    fn test_execute_swap_action_with_less_balance_than_swap_amount_executes() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let balance = Coin::new(
            swap_action.swap_amount.amount / Uint128::new(2),
            swap_action.swap_amount.denom.clone(),
        );

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[balance.clone()]);

        strategy.execute().assert_bank_balances(&vec![Coin::new(
            balance.amount.mul_floor(Decimal::percent(99)),
            swap_action.minimum_receive_amount.denom.clone(),
        )]);
    }

    #[test]
    fn test_execute_swap_action_with_swap_amount_scaled_to_minimum_swap_amount_executes() {
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
            .with_nodes(vec![Node::Action {
                action: Action::Swap(swap_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[swap_action.swap_amount.clone()]);

        strategy.assert_bank_balances(&vec![
            Coin::new(
                swap_action.swap_amount.amount - minimum_swap_amount.amount,
                swap_action.swap_amount.denom.clone(),
            ),
            Coin::new(
                minimum_swap_amount.amount.mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone(),
            ),
        ]);
    }

    // FIN limit order action tests

    #[test]
    fn test_instantiate_limit_order_action_with_bid_amount_too_small_fails() {
        let mut harness = CalcTestApp::setup();

        let order_action = FinLimitOrder {
            bid_amount: Some(Uint128::new(999)),
            ..default_limit_order_action(&harness)
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::LimitOrder(order_action.clone()),
                index: 0,
                next: None,
            }])
            .try_instantiate(&[Coin::new(1000000u128, order_action.bid_denom.clone())]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_limit_order_action_with_preset_current_price_fails() {
        let mut harness = CalcTestApp::setup();

        let order_action = FinLimitOrder {
            current_order: Some(StaleOrder {
                price: Decimal::one(),
            }),
            ..default_limit_order_action(&harness)
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::LimitOrder(order_action.clone()),
                index: 0,
                next: None,
            }])
            .try_instantiate(&[Coin::new(1000000u128, order_action.bid_denom.clone())]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_limit_order_action_executes_immediately() {
        let mut harness = CalcTestApp::setup();
        let order_action = default_limit_order_action(&harness);
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::LimitOrder(order_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[starting_balance.clone()]);

        strategy.assert_bank_balances(&vec![]).assert_fin_orders(
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

        let order_action = FinLimitOrder {
            strategy: PriceStrategy::Fixed(Decimal::percent(50)),
            ..default_limit_order_action(&harness)
        };

        let starting_balance = Coin::new(1_000_000u128, order_action.bid_denom.clone());
        let pair = harness.query_fin_config(&order_action.pair_address);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::LimitOrder(order_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[starting_balance.clone()]);

        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));
        let remaining_amount = Uint128::new(800_000);

        strategy
            .assert_bank_balances(&[])
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
            .with_nodes(vec![Node::Action {
                action: Action::LimitOrder(order_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[starting_balance.clone()]);

        strategy
            .assert_bank_balances(&vec![])
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
            .execute()
            .assert_bank_balances(&vec![])
            .assert_fin_orders(
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
    fn test_execute_limit_order_action_with_fixed_price_strategy_claims_filled_amount() {
        let mut harness = CalcTestApp::setup();

        let order_action = FinLimitOrder {
            strategy: PriceStrategy::Fixed(Decimal::percent(50)),
            ..default_limit_order_action(&harness)
        };

        let starting_balance = Coin::new(1_000_000u128, order_action.bid_denom.clone());
        let pair = harness.query_fin_config(&order_action.pair_address);

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::LimitOrder(order_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[starting_balance.clone()]);

        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));
        let remaining_amount = Uint128::new(800_000);

        strategy
            .assert_bank_balances(&vec![])
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
            .execute()
            .assert_bank_balance(&filled_amount.clone());
    }

    #[test]
    fn test_execute_limit_order_action_with_additional_balance_deploys_it() {
        let mut harness = CalcTestApp::setup();
        let order_action = default_limit_order_action(&harness);
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::LimitOrder(order_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[starting_balance.clone()]);

        strategy
            .assert_bank_balances(&vec![])
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
            .assert_bank_balances(&vec![])
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

        let order_action = FinLimitOrder {
            strategy: PriceStrategy::Offset {
                direction: Direction::Below,
                offset: Offset::Percent(10),
                tolerance: Some(Offset::Exact(Decimal::percent(1))),
            },
            pair_address: harness.fin_addr.clone(),
            side: Side::Quote,
            bid_denom: pair.denoms.quote().to_string(),
            ..default_limit_order_action(&harness)
        };

        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());
        let unknown = harness.unknown.clone();

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::LimitOrder(order_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[starting_balance.clone()]);

        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));

        strategy.assert_bank_balances(&vec![]).assert_fin_orders(
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
            .assert_bank_balances(&vec![])
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

        let order_action = FinLimitOrder {
            strategy: PriceStrategy::Offset {
                direction: Direction::Below,
                offset: Offset::Percent(10),
                tolerance: Some(Offset::Exact(Decimal::percent(90))),
            },
            pair_address: harness.fin_addr.clone(),
            side: Side::Quote,
            bid_denom: pair.denoms.quote().to_string(),
            ..default_limit_order_action(&harness)
        };

        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());
        let unknown = harness.unknown.clone();

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::LimitOrder(order_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[starting_balance.clone()]);

        strategy.assert_bank_balances(&vec![]).assert_fin_orders(
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
            .assert_bank_balances(&vec![])
            .execute()
            .assert_bank_balances(&vec![])
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
    fn test_withdraw_limit_order_action_with_unrelated_denoms_does_nothing() {
        let mut harness = CalcTestApp::setup();
        let pair = harness.query_fin_config(&harness.fin_addr);

        let price = Decimal::percent(50);
        let order_action = FinLimitOrder {
            strategy: PriceStrategy::Fixed(price),
            ..default_limit_order_action(&harness)
        };
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::LimitOrder(order_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[starting_balance.clone()]);

        let remaining_amount = Uint128::new(800_000);
        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));

        strategy
            .assert_bank_balances(&vec![])
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
            .withdraw(vec![Coin::new(100u128, "unrelated_denom".to_string())])
            .assert_bank_balances(&vec![])
            .assert_fin_orders(
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    price,                   // price
                    starting_balance.amount, // offer
                    remaining_amount,        // remaining
                    filled_amount.amount,    // filled
                )],
            );
    }

    #[test]
    fn test_pause_limit_order_action_with_filled_amount_withdraws_and_claims() {
        let mut harness = CalcTestApp::setup();
        let pair = harness.query_fin_config(&harness.fin_addr);

        let price = Decimal::percent(50);
        let order_action = FinLimitOrder {
            strategy: PriceStrategy::Fixed(price),
            ..default_limit_order_action(&harness)
        };
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::LimitOrder(order_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[starting_balance.clone()]);

        let remaining_amount = Uint128::new(800_000);
        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));

        strategy
            .assert_bank_balances(&vec![])
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
            .assert_bank_balance(&Coin::new(remaining_amount, order_action.bid_denom.clone()));
    }

    #[test]
    fn test_resume_limit_order_action_with_bid_denom_balance_deploys_it() {
        let mut harness = CalcTestApp::setup();
        let pair = harness.query_fin_config(&harness.fin_addr);

        let price = Decimal::percent(50);
        let order_action = FinLimitOrder {
            strategy: PriceStrategy::Fixed(price),
            ..default_limit_order_action(&harness)
        };
        let starting_balance = Coin::new(1000000u128, order_action.bid_denom.clone());

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::LimitOrder(order_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&[starting_balance.clone()]);

        let remaining_amount = Uint128::new(800_000);
        let filled_amount = Coin::new(100_000u128, pair.denoms.ask(&order_action.side));

        strategy
            .assert_bank_balances(&vec![])
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

    // Distribution Action tests

    #[test]
    fn test_instantiate_distribution_with_empty_denoms_fails() {
        let mut harness = CalcTestApp::setup();
        let distribution_action = Distribution {
            denoms: vec![],
            ..default_distribution_action(&harness)
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Distribute(distribution_action),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Distribute(distribution_action),
                index: 0,
                next: None,
            }])
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_distribution_with_zero_shares_destination_failures() {
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
            .with_nodes(vec![Node::Action {
                action: Action::Distribute(distribution_action),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Distribute(distribution_action),
                index: 0,
                next: None,
            }])
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
            .with_nodes(vec![Node::Action {
                action: Action::Distribute(distribution_action),
                index: 0,
                next: None,
            }])
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_distribution_with_native_denom_and_deposit_destination_failures() {
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
        };

        let result = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Distribute(distribution_action),
                index: 0,
                next: None,
            }])
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_distribution_with_native_denom_and_non_deposit_recipients_succeeds() {
        let mut harness = CalcTestApp::setup();
        let scheduler = harness.scheduler_addr.clone();
        let fee_collector = harness.fee_collector_addr.clone();

        let destinations = vec![
            Destination {
                recipient: Recipient::Bank {
                    address: harness.app.api().addr_make(&"test1"),
                },
                shares: Uint128::new(5_000),
                label: None,
            },
            Destination {
                recipient: Recipient::Bank {
                    address: harness.app.api().addr_make(&"test2"),
                },
                shares: Uint128::new(10_000),
                label: None,
            },
            Destination {
                recipient: Recipient::Contract {
                    address: scheduler.clone(),
                    msg: to_json_binary(&SchedulerExecuteMsg::Create(CreateTriggerMsg {
                        condition: Condition::BlocksCompleted(100),
                        msg: Binary::default(),
                        contract_address: Addr::unchecked("test_contract"),
                        executors: vec![],
                        jitter: None,
                    }))
                    .unwrap(),
                },
                shares: Uint128::new(5_000),
                label: None,
            },
        ];

        let total_shares_with_fees = destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares)
            .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

        let fee_collector_destination = Destination {
            recipient: Recipient::Bank {
                address: fee_collector.clone(),
            },
            shares: total_shares_with_fees.mul_floor(Decimal::bps(BASE_FEE_BPS)),
            label: None,
        };

        let distribution_action = Distribution {
            denoms: vec!["x/ruji".to_string()],
            destinations: destinations.clone(),
        };

        let starting_balances = vec![Coin::new(120_000u128, "x/ruji")];

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Distribute(distribution_action),
                index: 0,
                next: None,
            }])
            .instantiate(&starting_balances);

        strategy.assert_bank_balance(&Coin::new(0u128, "x/ruji"));

        for destination in [destinations, vec![fee_collector_destination]].concat() {
            let distributed = starting_balances
                .iter()
                .map(|b| {
                    Coin::new(
                        b.amount.mul_floor(Decimal::from_ratio(
                            destination.shares,
                            total_shares_with_fees,
                        )),
                        b.denom.clone(),
                    )
                })
                .collect::<Vec<_>>();

            match destination.recipient {
                Recipient::Bank { address } => {
                    harness.assert_address_balances(&address, &distributed);
                }
                Recipient::Contract { address, .. } => {
                    harness.assert_address_balances(&address, &distributed);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_instantiate_distribution_with_secured_denom_and_all_recipient_types_succeeds() {
        let mut harness = CalcTestApp::setup();
        let scheduler = harness.scheduler_addr.clone();
        let fee_collector = harness.fee_collector_addr.clone();

        let existing_scheduler_balance = harness
            .app
            .wrap()
            .query_balance(scheduler.clone(), "eth-usdc")
            .unwrap();

        println!(
            "Existing scheduler balance: {}",
            existing_scheduler_balance.to_string(),
        );

        let destinations = vec![
            Destination {
                recipient: Recipient::Deposit {
                    memo: "-secure:eth-usdc".to_string(),
                },
                shares: Uint128::new(10_000),
                label: None,
            },
            Destination {
                recipient: Recipient::Bank {
                    address: harness.app.api().addr_make(&"test1"),
                },
                shares: Uint128::new(10_000),
                label: None,
            },
            Destination {
                recipient: Recipient::Contract {
                    address: scheduler,
                    msg: to_json_binary(&SchedulerExecuteMsg::Create(CreateTriggerMsg {
                        condition: Condition::BlocksCompleted(100),
                        msg: Binary::default(),
                        contract_address: Addr::unchecked("test_contract"),
                        executors: vec![],
                        jitter: None,
                    }))
                    .unwrap(),
                },
                shares: Uint128::new(5_000),
                label: None,
            },
        ];

        let total_shares_with_fees = destinations
            .iter()
            .fold(Uint128::zero(), |acc, d| acc + d.shares)
            .mul_ceil(Decimal::bps(10_000 + BASE_FEE_BPS));

        let fee_collector_destination = Destination {
            recipient: Recipient::Bank {
                address: fee_collector.clone(),
            },
            shares: total_shares_with_fees.mul_ceil(Decimal::bps(BASE_FEE_BPS)),
            label: None,
        };

        let distribution_action = Distribution {
            denoms: vec!["eth-usdc".to_string()],
            destinations: destinations.clone(),
        };

        let starting_balances = vec![Coin::new(100_000u128, "eth-usdc")];

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![Node::Action {
                action: Action::Distribute(distribution_action.clone()),
                index: 0,
                next: None,
            }])
            .instantiate(&starting_balances);

        // TODO: Enable when MsgDeposit mock handler moves bank funds
        // strategy.assert_bank_balance(&Coin::new(0u128, "eth-usdc"));

        for destination in [destinations, vec![fee_collector_destination]].concat() {
            let distributed = starting_balances
                .iter()
                .map(|b| {
                    Coin::new(
                        b.amount.mul_floor(Decimal::from_ratio(
                            destination.shares,
                            total_shares_with_fees,
                        )),
                        b.denom.clone(),
                    )
                })
                .collect::<Vec<_>>();

            match destination.recipient {
                Recipient::Bank { address } => {
                    harness.assert_address_balances(&address, &distributed);
                }
                Recipient::Contract { address, .. } => {
                    let mut total_balance = Coins::try_from(distributed).unwrap();

                    total_balance
                        .add(existing_scheduler_balance.clone())
                        .unwrap();

                    harness.assert_address_balances(&address, &total_balance.to_vec());
                }
                _ => {
                    // TODO: Enable when MsgDeposit mock handler moves bank funds
                }
            }
        }
    }

    // Condition node tests

    #[test]
    fn test_execute_condition_node_with_satisfied_conditions_executes_on_success() {
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
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::BalanceAvailable {
                        address: None,
                        amount: swap_action.swap_amount.clone(),
                    },
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balance(&Coin::new(1u128, swap_action.swap_amount.denom.clone()));
    }

    #[test]
    fn test_execute_condition_node_with_unsatisfied_conditions_and_no_on_failure_skips() {
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
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::BalanceAvailable {
                        address: None,
                        amount: Coin::new(funds[0].amount + Uint128::one(), funds[0].denom.clone()),
                    },
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&funds);
    }

    #[test]
    fn test_execute_condition_node_with_unsatisfied_conditions_executes_on_failure() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let first_swap_action = Swap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
        };

        let second_swap_action = Swap {
            swap_amount: Coin::new(100u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: harness.fin_addr.clone(),
            })],
        };

        let funds = vec![Coin::new(
            first_swap_action.swap_amount.amount,
            fin_pair.denoms.base(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::BalanceAvailable {
                        address: None,
                        amount: Coin::new(funds[0].amount + Uint128::one(), funds[0].denom.clone()),
                    },
                    index: 0,
                    on_success: 1,
                    on_failure: Some(2),
                },
                Node::Action {
                    action: Action::Swap(first_swap_action.clone()),
                    index: 1,
                    next: None,
                },
                Node::Action {
                    action: Action::Swap(second_swap_action.clone()),
                    index: 2,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&vec![Coin::new(
                funds[0].amount - second_swap_action.swap_amount.amount,
                fin_pair.denoms.base(),
            )]);
    }

    #[test]
    fn test_execute_condition_node_respects_timestamp_elapsed_condition() {
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
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::TimestampElapsed(block_time.plus_seconds(60)),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&funds)
            .advance_time(61)
            .execute()
            .assert_bank_balances(&vec![]);
    }

    #[test]
    fn test_execute_condition_node_respects_block_elapsed_condition() {
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
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::BlocksCompleted(block_height + 60),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&funds)
            .advance_blocks(61)
            .execute()
            .assert_bank_balances(&vec![]);
    }

    #[test]
    fn test_execute_condition_node_respects_can_swap_condition() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = default_swap_action(&harness);

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount + Uint128::one(),
            fin_pair.denoms.base(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::CanSwap(swap_action.clone()),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&vec![]);

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::CanSwap(Swap {
                        maximum_slippage_bps: 0,
                        ..swap_action.clone()
                    }),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_condition_node_respects_balance_available_condition() {
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
            swap_action.swap_amount.amount,
            fin_pair.denoms.base(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::BalanceAvailable {
                        address: None,
                        amount: funds[0].clone(),
                    },
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&vec![]);

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::BalanceAvailable {
                        address: None,
                        amount: Coin::new(
                            funds[0].amount + Uint128::one(),
                            swap_action.swap_amount.denom.clone(),
                        ),
                    },
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_condition_node_respects_strategy_status_condition() {
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

        let strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![])
            .instantiate(&[Coin::new(100_000u128, "x/ruji")]);

        StrategyBuilder::new(strategy.harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::StrategyStatus {
                        manager_contract: manager.clone(),
                        contract_address: strategy.strategy_addr.clone(),
                        status: StrategyStatus::Active,
                    },
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: strategy_action.clone(),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&vec![]);

        StrategyBuilder::new(strategy.harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::StrategyStatus {
                        manager_contract: manager.clone(),
                        contract_address: strategy.strategy_addr.clone(),
                        status: StrategyStatus::Paused,
                    },
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: strategy_action.clone(),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&vec![swap_action.swap_amount.clone()]);
    }

    #[test]
    fn test_execute_condition_node_respects_oracle_price_condition() {
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
            swap_action.swap_amount.amount,
            fin_pair.denoms.base(),
        )];

        let swap_action = default_swap_action(&harness);

        let strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![])
            .instantiate(&[Coin::new(100_000u128, "x/ruji")]);

        // BTC-BTC oracle price stubbed at $100,100.00

        StrategyBuilder::new(strategy.harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::OraclePrice {
                        asset: "btc-btc".to_string(),
                        direction: Direction::Below,
                        price: Decimal::from_str("100000").unwrap(),
                    },
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&funds);

        StrategyBuilder::new(strategy.harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::OraclePrice {
                        asset: "btc-btc".to_string(),
                        direction: Direction::Above,
                        price: Decimal::from_str("100000").unwrap(),
                    },
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&[]);

        StrategyBuilder::new(strategy.harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::OraclePrice {
                        asset: "btc-btc".to_string(),
                        direction: Direction::Above,
                        price: Decimal::from_str("100200").unwrap(),
                    },
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&funds);

        StrategyBuilder::new(strategy.harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::OraclePrice {
                        asset: "btc-btc".to_string(),
                        direction: Direction::Below,
                        price: Decimal::from_str("100200").unwrap(),
                    },
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&[]);
    }

    #[test]
    fn test_instantiate_schedule_condition_with_invalid_cron_expression_fails() {
        let mut harness = CalcTestApp::setup();
        let scheduler = harness.scheduler_addr.clone();
        let manager = harness.manager_addr.clone();

        let swap_action = default_swap_action(&harness);

        let result = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::Schedule(Schedule {
                        scheduler,
                        contract_address: manager,
                        msg: None,
                        cadence: Cadence::Cron {
                            expr: "bad cron".to_string(),
                            previous: None,
                        },
                        execution_rebate: vec![],
                        executors: vec![],
                        jitter: None,
                        next: None,
                    }),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action),
                    index: 1,
                    next: None,
                },
            ])
            .try_instantiate(&[]);

        assert!(result.is_err());
    }

    #[test]
    fn test_instantiate_schedule_condition_with_time_cadence_due_executes_immediately() {
        let mut harness = CalcTestApp::setup();
        let scheduler = harness.scheduler_addr.clone();
        let manager = harness.manager_addr.clone();

        let swap_action = default_swap_action(&harness);
        let funds = vec![swap_action.swap_amount.clone()];

        let schedule = Schedule {
            scheduler,
            executors: vec![],
            jitter: None,
            next: None,
            contract_address: manager,
            msg: None,
            cadence: Cadence::Time {
                duration: Duration::from_secs(60),
                previous: None,
            },
            execution_rebate: vec![],
        };

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::Schedule(schedule),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&[]);
    }

    #[test]
    fn test_instantiate_schedule_condition_with_time_cadence_not_due_executes_after_duration() {
        let mut harness = CalcTestApp::setup();
        let scheduler = harness.scheduler_addr.clone();
        let manager = harness.manager_addr.clone();

        let swap_action = default_swap_action(&harness);
        let funds = vec![swap_action.swap_amount.clone()];

        let schedule = Schedule {
            scheduler,
            executors: vec![],
            jitter: None,
            next: None,
            contract_address: manager,
            msg: None,
            cadence: Cadence::Time {
                duration: Duration::from_secs(60),
                previous: Some(harness.app.block_info().time),
            },
            execution_rebate: vec![],
        };

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::Schedule(schedule),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&funds)
            .advance_time(61)
            .execute()
            .assert_bank_balances(&[]);
    }

    #[test]
    fn test_instantiate_schedule_condition_with_block_cadence_due_executes_immediately() {
        let mut harness = CalcTestApp::setup();
        let scheduler = harness.scheduler_addr.clone();
        let manager = harness.manager_addr.clone();

        let swap_action = default_swap_action(&harness);
        let funds = vec![swap_action.swap_amount.clone()];

        let schedule = Schedule {
            scheduler,
            executors: vec![],
            jitter: None,
            next: None,
            contract_address: manager,
            msg: None,
            cadence: Cadence::Blocks {
                interval: 60,
                previous: None,
            },
            execution_rebate: vec![],
        };

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::Schedule(schedule),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&[]);
    }

    #[test]
    fn test_instantiate_schedule_condition_with_block_cadence_not_due_executes_after_interval() {
        let mut harness = CalcTestApp::setup();
        let scheduler = harness.scheduler_addr.clone();
        let manager = harness.manager_addr.clone();

        let swap_action = default_swap_action(&harness);
        let funds = vec![swap_action.swap_amount.clone()];

        let schedule = Schedule {
            scheduler,
            executors: vec![],
            jitter: None,
            next: None,
            contract_address: manager,
            msg: None,
            cadence: Cadence::Blocks {
                interval: 60,
                previous: Some(harness.app.block_info().height),
            },
            execution_rebate: vec![],
        };

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::Schedule(schedule),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&funds)
            .advance_blocks(61)
            .execute()
            .assert_bank_balances(&[]);
    }

    #[test]
    fn test_instantiate_schedule_condition_with_cron_cadence_due_executes_immediately() {
        let mut harness = CalcTestApp::setup();
        let scheduler = harness.scheduler_addr.clone();
        let manager = harness.manager_addr.clone();

        let swap_action = default_swap_action(&harness);
        let funds = vec![swap_action.swap_amount.clone()];

        let schedule = Schedule {
            scheduler,
            executors: vec![],
            jitter: None,
            next: None,
            contract_address: manager,
            msg: None,
            cadence: Cadence::Cron {
                expr: "0 * * * * *".to_string(),
                previous: None,
            },
            execution_rebate: vec![],
        };

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::Schedule(schedule),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&[]);
    }

    #[test]
    fn test_instantiate_schedule_condition_with_cron_cadence_not_due_executes_after_next() {
        let mut harness = CalcTestApp::setup();
        let scheduler = harness.scheduler_addr.clone();
        let manager = harness.manager_addr.clone();

        let swap_action = default_swap_action(&harness);
        let funds = vec![swap_action.swap_amount.clone()];

        let schedule = Schedule {
            scheduler,
            executors: vec![],
            jitter: None,
            next: None,
            contract_address: manager,
            msg: None,
            cadence: Cadence::Cron {
                expr: "0 * * * * *".to_string(),
                previous: Some(harness.app.block_info().time),
            },
            execution_rebate: vec![],
        };

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::Schedule(schedule),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&funds)
            .advance_time(61)
            .execute()
            .assert_bank_balances(&[]);
    }

    #[test]
    fn test_instantiate_schedule_condition_with_limit_order_cadence_does_not_execute() {
        let mut harness = CalcTestApp::setup();
        let scheduler = harness.scheduler_addr.clone();
        let manager = harness.manager_addr.clone();

        let swap_action = default_swap_action(&harness);
        let funds = vec![swap_action.swap_amount.clone()];

        let schedule = Schedule {
            scheduler,
            executors: vec![],
            jitter: None,
            next: None,
            contract_address: manager,
            msg: None,
            cadence: Cadence::LimitOrder {
                pair_address: harness.fin_addr.clone(),
                side: Side::Base,
                previous: None,
                strategy: PriceStrategy::Fixed(Decimal::one()),
            },
            execution_rebate: vec![],
        };

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::Schedule(schedule),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&funds);
    }

    #[test]
    fn test_crank_time_schedule_sets_and_resets_triggers() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let schedule = Schedule {
            scheduler: harness.scheduler_addr.clone(),
            executors: vec![],
            jitter: None,
            next: None,
            contract_address: harness.manager_addr.clone(),
            msg: None,
            cadence: Cadence::Time {
                duration: Duration::from_secs(60),
                previous: Some(harness.app.block_info().time),
            },
            execution_rebate: vec![],
        };

        let funds = vec![Coin::new(
            swap_action.swap_amount.amount * Uint128::new(20),
            swap_action.swap_amount.denom.clone(),
        )];

        StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::Schedule(schedule),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds)
            .assert_bank_balances(&funds)
            .advance_time(60)
            .advance_time(60)
            .advance_time(60)
            .advance_time(60)
            .advance_time(60)
            .assert_bank_balances(&[Coin::new(
                funds[0].amount - swap_action.swap_amount.amount * Uint128::new(5),
                swap_action.swap_amount.denom.clone(),
            )]);
    }

    #[test]
    fn test_schedule_condition_deposits_execution_rebate() {
        let mut harness = CalcTestApp::setup();
        let swap_action = default_swap_action(&harness);

        let schedule = Schedule {
            scheduler: harness.scheduler_addr.clone(),
            executors: vec![],
            jitter: None,
            next: None,
            contract_address: harness.manager_addr.clone(),
            msg: None,
            cadence: Cadence::Time {
                duration: Duration::from_secs(60),
                previous: Some(harness.app.block_info().time),
            },
            execution_rebate: vec![Coin::new(1u128, "x/ruji")],
        };

        let funds = vec![
            Coin::new(
                swap_action.swap_amount.amount * Uint128::new(20),
                swap_action.swap_amount.denom.clone(),
            ),
            Coin::new(10u128, "x/ruji"),
        ];

        let mut strategy = StrategyBuilder::new(&mut harness)
            .with_nodes(vec![
                Node::Condition {
                    condition: Condition::Schedule(schedule),
                    index: 0,
                    on_success: 1,
                    on_failure: None,
                },
                Node::Action {
                    action: Action::Swap(swap_action.clone()),
                    index: 1,
                    next: None,
                },
            ])
            .instantiate(&funds);

        strategy
            .assert_bank_balances(&funds)
            .advance_time(60)
            .advance_time(60)
            .advance_time(60)
            .advance_time(60)
            .advance_time(60)
            .assert_bank_balances(&[
                Coin::new(
                    funds[0].amount - swap_action.swap_amount.amount * Uint128::new(5),
                    swap_action.swap_amount.denom.clone(),
                ),
                Coin::new(5u128, "x/ruji"),
            ]);

        let keeper = strategy.keeper.clone();

        let keeper_balance = strategy
            .harness
            .app
            .wrap()
            .query_balance(keeper, "x/ruji")
            .unwrap();

        assert_eq!(keeper_balance, Coin::new(5u128, "x/ruji"));
    }
}
