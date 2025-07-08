#[cfg(test)]
mod integration_tests {
    use std::{collections::HashSet, vec};

    use calc_rs::{
        actions::{
            action::Action,
            fin_swap::FinSwap,
            schedule::Schedule,
            swap::{OptimalSwap, SwapAmountAdjustment, SwapRoute},
        },
        cadence::Cadence,
        conditions::{Condition, Conditions, Threshold},
        statistics::Statistics,
        strategy::{Idle, Strategy, StrategyConfig},
    };
    use cosmwasm_std::{Coin, Decimal, Uint128};
    use rujira_fin::order;
    use rujira_rs::fin::{self, OrderResponse, OrdersResponse, Price, Side};

    use calc_rs::actions::limit_order::{LimitOrder, OrderPriceStrategy};
    use calc_rs::manager::StrategyStatus;

    use crate::harness::CalcTestApp;
    use crate::strategy_builder::StrategyBuilder;

    #[test]
    fn test_instantiate_strategy_succeeds() {
        let mut harness = CalcTestApp::setup();
        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = OptimalSwap {
            routes: vec![SwapRoute::Fin(harness.fin_addr.clone())],
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 50,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let owner = harness.owner.clone();
        let mut strategy_handler = StrategyBuilder::new(&mut harness, owner.clone(), "Simple Swap")
            .with_action(Action::OptimalSwap(swap_action.clone()))
            .instantiate();

        let manager_addr = harness.manager_addr.clone();

        strategy_handler.assert_config(
            &mut harness,
            StrategyConfig {
                manager: manager_addr,
                escrowed: HashSet::from([swap_action.minimum_receive_amount.denom.clone()]),
                strategy: Strategy {
                    owner: owner.clone(),
                    action: Action::OptimalSwap(swap_action),
                    state: Idle {
                        contract_address: strategy_handler.strategy_addr.clone(),
                    },
                },
            },
        );
    }

    #[test]
    fn test_execute_simple_swap_strategy_updates_balances_and_stats() {
        let mut harness = CalcTestApp::setup();
        let keeper = harness.app.api().addr_make("keeper");
        let owner = harness.owner.clone();

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_route = OptimalSwap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(harness.fin_addr.clone())],
        };

        let mut strategy_handler =
            StrategyBuilder::new(&mut harness, owner.clone(), "Simple ATOM->OSMO Swap")
                .with_action(Action::OptimalSwap(swap_route.clone()))
                .with_funds(vec![swap_route.swap_amount.clone()])
                .with_keeper(keeper)
                .instantiate();

        strategy_handler
            .execute(&mut harness, &owner, &[])
            .assert_balances(
                &mut harness,
                vec![Coin::new(
                    swap_route
                        .swap_amount
                        .amount
                        .mul_floor(Decimal::percent(99)),
                    swap_route.minimum_receive_amount.denom.clone(),
                )],
            )
            .assert_stats(
                &mut harness,
                Statistics {
                    swapped: vec![swap_route.swap_amount],
                    ..Statistics::default()
                },
            );
    }

    #[test]
    fn test_execute_strategy_with_unsatisfied_condition_does_nothing() {
        let mut harness = CalcTestApp::setup();
        let keeper = harness.app.api().addr_make("keeper");
        let owner = harness.owner.clone();

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

        let mut strategy_handler =
            StrategyBuilder::new(&mut harness, owner.clone(), "Simple ATOM->OSMO Swap")
                .with_action(Action::Conditional((
                    Conditions {
                        conditions: vec![Condition::OwnBalanceAvailable {
                            amount: swap_action.swap_amount.clone(),
                        }],
                        threshold: Threshold::All,
                    },
                    Box::new(Action::OptimalSwap(swap_action.clone())),
                )))
                .with_funds(funds.clone())
                .with_keeper(keeper)
                .instantiate();

        strategy_handler
            .execute(&mut harness, &owner, &[])
            .assert_balances(&mut harness, funds)
            .assert_stats(&mut harness, Statistics::default());
    }

    #[test]
    fn test_pause_strategy_cancels_open_limit_orders() {
        let mut harness = CalcTestApp::setup();
        let owner = harness.app.api().addr_make("owner");

        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        let order_price = Decimal::one();

        let order_action = LimitOrder {
            pair_address: harness.fin_addr.clone(),
            side: Side::Base,
            bid_denom: fin_pair.denoms.base().to_string(),
            bid_amount: Some(Uint128::new(1000u128)),
            strategy: OrderPriceStrategy::Fixed(order_price),
            current_price: None,
        };

        let mut strategy_handler =
            StrategyBuilder::new(&mut harness, owner.clone(), "Limit Order Strategy")
                .with_action(Action::SetLimitOrder(order_action.clone()))
                .with_funds(vec![Coin::new(
                    order_action.bid_amount.unwrap(),
                    order_action.bid_denom.clone(),
                )])
                .instantiate();

        let strategy_addr = strategy_handler.strategy_addr.clone();
        let manager_addr = harness.manager_addr.clone();

        strategy_handler
            .execute(&mut harness, &owner, &[])
            .assert_config(
                &mut harness,
                StrategyConfig {
                    manager: manager_addr.clone(),
                    escrowed: HashSet::from([fin_pair.denoms.quote().to_string()]),
                    strategy: Strategy {
                        owner: owner.clone(),
                        action: Action::SetLimitOrder(LimitOrder {
                            current_price: Some(Price::Fixed(order_price)),
                            ..order_action.clone()
                        }),
                        state: Idle {
                            contract_address: strategy_addr.clone(),
                        },
                    },
                },
            )
            .assert_status(&mut harness, StrategyStatus::Active)
            .assert_fin_orders(
                &mut harness,
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    order_price,
                    order_action.bid_amount.unwrap(),
                    order_action.bid_amount.unwrap(),
                    Uint128::zero(),
                )],
            )
            .pause(&mut harness, &owner)
            .assert_config(
                &mut harness,
                StrategyConfig {
                    manager: manager_addr,
                    escrowed: HashSet::from([fin_pair.denoms.quote().to_string()]),
                    strategy: Strategy {
                        owner: owner.clone(),
                        action: Action::SetLimitOrder(order_action.clone()),
                        state: Idle {
                            contract_address: strategy_addr,
                        },
                    },
                },
            )
            .assert_status(&mut harness, StrategyStatus::Paused)
            .assert_fin_orders(&mut harness, &order_action.pair_address, vec![]);
    }

    #[test]
    fn test_resume_strategy_re_executes_and_places_orders() {
        let mut harness = CalcTestApp::setup();
        let owner = harness.app.api().addr_make("owner");

        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        let order_price = Decimal::one();

        let order_action = LimitOrder {
            pair_address: harness.fin_addr.clone(),
            side: Side::Base,
            bid_denom: fin_pair.denoms.base().to_string(),
            bid_amount: Some(Uint128::new(1000u128)),
            strategy: OrderPriceStrategy::Fixed(order_price),
            current_price: None,
        };

        let mut strategy_handler =
            StrategyBuilder::new(&mut harness, owner.clone(), "Limit Order Strategy")
                .with_action(Action::SetLimitOrder(order_action.clone()))
                .with_funds(vec![Coin::new(
                    order_action.bid_amount.unwrap().u128(),
                    order_action.bid_denom.clone(),
                )])
                .instantiate();

        let manager_addr = harness.manager_addr.clone();
        let strategy_addr = strategy_handler.strategy_addr.clone();

        strategy_handler
            .execute(&mut harness, &owner, &[])
            .assert_config(
                &mut harness,
                StrategyConfig {
                    manager: manager_addr.clone(),
                    escrowed: HashSet::from([fin_pair.denoms.quote().to_string()]),
                    strategy: Strategy {
                        owner: owner.clone(),
                        action: Action::SetLimitOrder(LimitOrder {
                            current_price: Some(Price::Fixed(order_price)),
                            ..order_action.clone()
                        }),
                        state: Idle {
                            contract_address: strategy_addr.clone(),
                        },
                    },
                },
            )
            .assert_status(&mut harness, StrategyStatus::Active)
            .assert_fin_orders(
                &mut harness,
                &order_action.pair_address,
                vec![(
                    order_action.side.clone(),
                    order_price,
                    order_action.bid_amount.unwrap(),
                    order_action.bid_amount.unwrap(),
                    Uint128::zero(),
                )],
            )
            .pause(&mut harness, &owner)
            .assert_config(
                &mut harness,
                StrategyConfig {
                    manager: manager_addr.clone(),
                    escrowed: HashSet::from([fin_pair.denoms.quote().to_string()]),
                    strategy: Strategy {
                        owner: owner.clone(),
                        action: Action::SetLimitOrder(order_action.clone()),
                        state: Idle {
                            contract_address: strategy_addr.clone(),
                        },
                    },
                },
            )
            .assert_fin_orders(&mut harness, &order_action.pair_address, vec![])
            .resume(&mut harness, &owner)
            .assert_config(
                &mut harness,
                StrategyConfig {
                    manager: manager_addr,
                    escrowed: HashSet::from([fin_pair.denoms.quote().to_string()]),
                    strategy: Strategy {
                        owner: owner.clone(),
                        action: Action::SetLimitOrder(LimitOrder {
                            current_price: Some(Price::Fixed(order_price)),
                            ..order_action.clone()
                        }),
                        state: Idle {
                            contract_address: strategy_addr.clone(),
                        },
                    },
                },
            )
            .assert_status(&mut harness, StrategyStatus::Active)
            .assert_fin_orders(
                &mut harness,
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
        let keeper = harness.app.api().addr_make("keeper");

        let fin_pair = harness.query_fin_config(&harness.fin_addr);
        let owner = harness.owner.clone();
        let scheduler_addr = harness.scheduler_addr.clone();

        let swap_action = FinSwap {
            pair_address: harness.fin_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let mut strategy_handler =
            StrategyBuilder::new(&mut harness, owner.clone(), "Scheduled Strategy")
                .with_action(Action::Schedule(Schedule {
                    scheduler: scheduler_addr.clone(),
                    cadence: Cadence::Blocks {
                        interval: 5,
                        previous: None,
                    },
                    execution_rebate: vec![],
                    action: Box::new(Action::FinSwap(swap_action.clone())),
                }))
                .with_funds(vec![Coin::new(
                    swap_action.swap_amount.amount * Uint128::new(10),
                    swap_action.swap_amount.denom.clone(),
                )])
                .with_keeper(keeper.clone())
                .instantiate();

        strategy_handler
            .execute(&mut harness, &owner, &[])
            .assert_balances(
                &mut harness,
                vec![
                    Coin::new(
                        swap_action.swap_amount.amount * Uint128::new(9),
                        swap_action.swap_amount.denom.clone(),
                    ),
                    Coin::new(
                        swap_action
                            .swap_amount
                            .amount
                            .mul_floor(Decimal::percent(99)),
                        swap_action.minimum_receive_amount.denom.clone(),
                    ),
                ],
            )
            .assert_swapped(&mut harness, vec![swap_action.swap_amount.clone()])
            .advance_blocks(&mut harness, 2)
            .execute(&mut harness, &keeper, &[])
            .assert_swapped(&mut harness, vec![swap_action.swap_amount.clone()])
            .advance_blocks(&mut harness, 4)
            .execute(&mut harness, &keeper, &[])
            .assert_balances(
                &mut harness,
                vec![
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
                ],
            )
            .assert_swapped(
                &mut harness,
                vec![Coin::new(
                    swap_action.swap_amount.amount * Uint128::new(2),
                    swap_action.swap_amount.denom.clone(),
                )],
            );
    }

    #[test]
    fn test_schedule_action_with_cron_cadence_schedules_correctly() {
        let mut harness = CalcTestApp::setup();
        let owner = harness.app.api().addr_make("owner");
        let keeper = harness.app.api().addr_make("keeper");
        let scheduler_addr = harness.scheduler_addr.clone();

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = FinSwap {
            pair_address: harness.fin_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let crank_action = Action::Schedule(Schedule {
            scheduler: scheduler_addr.clone(),
            cadence: Cadence::Cron {
                expr: "*/10 * * * * *".to_string(),
                previous: None,
            },
            execution_rebate: vec![],
            action: Box::new(Action::FinSwap(swap_action.clone())),
        });

        let mut strategy_handler =
            StrategyBuilder::new(&mut harness, owner.clone(), "Crank Cron Strategy")
                .with_action(crank_action)
                .with_funds(vec![Coin::new(
                    swap_action.swap_amount.amount * Uint128::new(10),
                    swap_action.swap_amount.denom.clone(),
                )])
                .with_keeper(keeper.clone())
                .instantiate();

        strategy_handler
            .advance_time(&mut harness, 10)
            .execute(&mut harness, &owner, &[])
            .assert_balances(
                &mut harness,
                vec![
                    Coin::new(
                        swap_action.swap_amount.amount * Uint128::new(9),
                        swap_action.swap_amount.denom.clone(),
                    ),
                    Coin::new(
                        swap_action
                            .swap_amount
                            .amount
                            .mul_floor(Decimal::percent(99)),
                        swap_action.minimum_receive_amount.denom.clone(),
                    ),
                ],
            )
            .assert_swapped(&mut harness, vec![swap_action.swap_amount.clone()])
            .advance_time(&mut harness, 2)
            .execute(&mut harness, &keeper, &[])
            .assert_swapped(&mut harness, vec![swap_action.swap_amount.clone()])
            .advance_time(&mut harness, 10)
            .execute(&mut harness, &keeper, &[])
            .assert_balances(
                &mut harness,
                vec![
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
                ],
            )
            .assert_swapped(
                &mut harness,
                vec![Coin::new(
                    swap_action.swap_amount.amount * Uint128::new(2),
                    swap_action.swap_amount.denom.clone(),
                )],
            );
    }

    // #[test]
    // fn test_composite_all_threshold_halts_on_first_error() {
    //     let mut harness = CalcTestApp::setup();
    //     let owner = harness.app.api().addr_make("owner");
    //     let creator = harness.app.api().addr_make("creator");
    //     let keeper = harness.app.api().addr_make("keeper");

    //     let fin_pair = harness.query_fin_config(&harness.fin_addr);

    //     let valid_swap_action = Swap {
    //         exchange_contract: harness.exchanger_addr.clone(),
    //         swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
    //         minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
    //         maximum_slippage_bps: 101,
    //         adjustment: SwapAmountAdjustment::Fixed,
    //         route: Some(Route::FinMarket {
    //             address: harness.fin_addr.clone(),
    //         }),
    //     };

    //     // This swap will fail due to insufficient funds (we won't fund it)
    //     let invalid_swap_action = Swap {
    //         exchange_contract: harness.exchanger_addr.clone(),
    //         swap_amount: Coin::new(5000u128, fin_pair.denoms.base()),
    //         minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
    //         maximum_slippage_bps: 101,
    //         adjustment: SwapAmountAdjustment::Fixed,
    //         route: Some(Route::FinMarket {
    //             address: harness.fin_addr.clone(),
    //         }),
    //     };

    //     let strategy_addr = harness
    //         .create_strategy(
    //             &creator,
    //             &owner,
    //             "Composite All Threshold Strategy",
    //             ActionConfig::Compose(Behaviour {
    //                 actions: vec![
    //                     ActionConfig::Swap(valid_swap_action.clone()),
    //                     ActionConfig::Swap(invalid_swap_action.clone()),
    //                 ],
    //                 threshold: Threshold::All,
    //             }),
    //         )
    //         .unwrap();

    //     // Fund only for the first swap
    //     harness.fund_contract(
    //         &strategy_addr,
    //         &owner,
    //         &[valid_swap_action.swap_amount.clone()],
    //     );

    //     let initial_balances = harness.query_balances(&strategy_addr);
    //     let initial_stats = harness.query_strategy_stats(&strategy_addr);

    //     // Execute the strategy, expect an error because the second swap will fail
    //     let res = harness.execute_strategy(&keeper, &strategy_addr, &[]);
    //     assert!(res.is_err());

    //     // Assert that balances and statistics remain unchanged, as Threshold::All should revert all actions on failure
    //     assert_eq!(harness.query_balances(&strategy_addr), initial_balances);
    //     assert_eq!(harness.query_strategy_stats(&strategy_addr), initial_stats);
    // }

    // #[test]
    // fn test_composite_any_threshold_executes_all_and_handles_partial_failure() {
    //     let mut harness = CalcTestApp::setup();
    //     let owner = harness.app.api().addr_make("owner");
    //     let creator = harness.app.api().addr_make("creator");
    //     let keeper = harness.app.api().addr_make("keeper");

    //     let fin_pair = harness.query_fin_config(&harness.fin_addr);

    //     let valid_swap_action = Swap {
    //         exchange_contract: harness.exchanger_addr.clone(),
    //         swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
    //         minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
    //         maximum_slippage_bps: 101,
    //         adjustment: SwapAmountAdjustment::Fixed,
    //         route: Some(Route::FinMarket {
    //             address: harness.fin_addr.clone(),
    //         }),
    //     };

    //     // This swap will fail due to insufficient funds (we won't fund it)
    //     let invalid_swap_action = Swap {
    //         exchange_contract: harness.exchanger_addr.clone(),
    //         swap_amount: Coin::new(5000u128, fin_pair.denoms.base()),
    //         minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
    //         maximum_slippage_bps: 101,
    //         adjustment: SwapAmountAdjustment::Fixed,
    //         route: Some(Route::FinMarket {
    //             address: harness.fin_addr.clone(),
    //         }),
    //     };

    //     let strategy_addr = harness
    //         .create_strategy(
    //             &creator,
    //             &owner,
    //             "Composite Any Threshold Strategy",
    //             ActionConfig::Compose(Behaviour {
    //                 actions: vec![
    //                     ActionConfig::Swap(valid_swap_action.clone()),
    //                     ActionConfig::Swap(invalid_swap_action.clone()),
    //                 ],
    //                 threshold: Threshold::Any,
    //             }),
    //         )
    //         .unwrap();

    //     // Fund only for the first swap
    //     harness.fund_contract(
    //         &strategy_addr,
    //         &owner,
    //         &[valid_swap_action.swap_amount.clone()],
    //     );

    //     // Execute the strategy, it should succeed because Threshold::Any allows partial failures
    //     let res = harness.execute_strategy(&keeper, &strategy_addr, &[]);
    //     assert!(res.is_ok());

    //     // Verify the valid swap occurred
    //     assert_eq!(
    //         harness.query_balances(&strategy_addr),
    //         vec![Coin::new(
    //             valid_swap_action
    //                 .swap_amount
    //                 .amount
    //                 .mul_floor(Decimal::percent(99)),
    //             valid_swap_action.minimum_receive_amount.denom.clone()
    //         )]
    //     );

    //     // Verify statistics updated for the valid swap
    //     assert_eq!(
    //         harness.query_strategy_stats(&strategy_addr),
    //         Statistics {
    //             swapped: vec![valid_swap_action.swap_amount.clone()],
    //             ..Statistics::default()
    //         }
    //     );
    // }

    // #[test]
    // fn test_update_strategy_from_unauthorized_sender_fails() {
    //     let mut harness = CalcTestApp::setup();
    //     let owner = harness.app.api().addr_make("owner");
    //     let creator = harness.app.api().addr_make("creator");
    //     let unauthorized_sender = harness.app.api().addr_make("unauthorized");

    //     let swap_action = Swap {
    //         exchange_contract: harness.exchanger_addr.clone(),
    //         swap_amount: Coin::new(1000u128, "uatom"),
    //         minimum_receive_amount: Coin::new(1u128, "rune"),
    //         maximum_slippage_bps: 50,
    //         adjustment: SwapAmountAdjustment::Fixed,
    //         route: None,
    //     };

    //     let strategy_behaviour = ActionConfig::Compose(Behaviour {
    //         actions: vec![ActionConfig::Swap(swap_action.clone())],
    //         threshold: Threshold::All,
    //     });

    //     let strategy_addr = harness
    //         .create_strategy(
    //             &creator,
    //             &owner,
    //             "Simple ATOM->OSMO Swap",
    //             strategy_behaviour.clone(),
    //         )
    //         .unwrap();

    //     // Attempt to update the strategy status from an unauthorized sender
    //     let res = harness.app.execute_contract(
    //         unauthorized_sender.clone(),
    //         harness.manager_addr.clone(),
    //         &ManagerExecuteMsg::UpdateStrategyStatus {
    //             contract_address: strategy_addr.clone(),
    //             status: StrategyStatus::Paused,
    //         },
    //         &[],
    //     );

    //     assert!(res.is_err());
    //     assert!(res.unwrap_err().to_string().contains("Unauthorized"));

    //     // Verify the strategy status remains unchanged
    //     let strategy = harness.query_strategy(&strategy_addr);
    //     assert_eq!(strategy.status, StrategyStatus::Active);
    // }

    // #[test]
    // fn test_withdraw_from_unauthorized_sender_fails() {
    //     let mut harness = CalcTestApp::setup();
    //     let owner = harness.app.api().addr_make("owner");
    //     let creator = harness.app.api().addr_make("creator");
    //     let unauthorized_sender = harness.app.api().addr_make("unauthorized");

    //     let swap_action = Swap {
    //         exchange_contract: harness.exchanger_addr.clone(),
    //         swap_amount: Coin::new(1000u128, "uatom"),
    //         minimum_receive_amount: Coin::new(1u128, "rune"),
    //         maximum_slippage_bps: 50,
    //         adjustment: SwapAmountAdjustment::Fixed,
    //         route: None,
    //     };

    //     let strategy_behaviour = ActionConfig::Compose(Behaviour {
    //         actions: vec![ActionConfig::Swap(swap_action.clone())],
    //         threshold: Threshold::All,
    //     });

    //     let strategy_addr = harness
    //         .create_strategy(
    //             &creator,
    //             &owner,
    //             "Simple ATOM->OSMO Swap",
    //             strategy_behaviour.clone(),
    //         )
    //         .unwrap();

    //     let funds_to_send = &[Coin::new(1000u128, "uatom")];
    //     harness.fund_contract(&strategy_addr, &owner, funds_to_send);

    //     let initial_balances = harness.query_balances(&strategy_addr);

    //     // Attempt to withdraw funds from an unauthorized sender
    //     let res = harness.app.execute_contract(
    //         unauthorized_sender.clone(),
    //         strategy_addr.clone(),
    //         &StrategyExecuteMsg::Withdraw(vec![funds_to_send[0].clone()]),
    //         &[],
    //     );

    //     assert!(res.is_err());
    //     assert!(res.unwrap_err().to_string().contains("Unauthorized"));

    //     // Verify the balances remain unchanged
    //     assert_eq!(harness.query_balances(&strategy_addr), initial_balances);
    // }

    // #[test]
    // fn test_withdraw_escrowed_funds_fails() {
    //     let mut harness = CalcTestApp::setup();
    //     let owner = harness.app.api().addr_make("owner");
    //     let creator = harness.app.api().addr_make("creator");

    //     let fin_pair = harness.query_fin_config(&harness.fin_addr);

    //     let swap_action = Swap {
    //         exchange_contract: harness.exchanger_addr.clone(),
    //         swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
    //         minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
    //         maximum_slippage_bps: 101,
    //         adjustment: SwapAmountAdjustment::Fixed,
    //         route: Some(Route::FinMarket {
    //             address: harness.fin_addr.clone(),
    //         }),
    //     };

    //     let strategy_addr = harness
    //         .create_strategy(
    //             &creator,
    //             &owner,
    //             "Simple ATOM->OSMO Swap",
    //             ActionConfig::Compose(Behaviour {
    //                 actions: vec![ActionConfig::Swap(swap_action.clone())],
    //                 threshold: Threshold::All,
    //             }),
    //         )
    //         .unwrap();

    //     let funds_to_send = &[swap_action.swap_amount.clone()];
    //     harness.fund_contract(&strategy_addr, &owner, funds_to_send);

    //     let initial_balances = harness.query_balances(&strategy_addr);

    //     // Attempt to withdraw escrowed funds (minimum_receive_amount.denom is escrowed)
    //     let res = harness.app.execute_contract(
    //         owner.clone(),
    //         strategy_addr.clone(),
    //         &StrategyExecuteMsg::Withdraw(vec![swap_action.minimum_receive_amount.clone()]),
    //         &[],
    //     );

    //     assert!(res.is_err());
    //     assert_eq!(
    //         res.unwrap_err().to_string(),
    //         format!(
    //             "Generic error: Cannot withdraw escrowed funds: {}",
    //             swap_action.minimum_receive_amount.denom
    //         )
    //     );

    //     // Verify the balances remain unchanged
    //     assert_eq!(harness.query_balances(&strategy_addr), initial_balances);
    // }

    // #[test]
    // fn test_instantiate_with_invalid_cron_string_fails() {
    //     let mut harness = CalcTestApp::setup();
    //     let owner = harness.app.api().addr_make("owner");
    //     let creator = harness.app.api().addr_make("creator");

    //     let swap_action = Swap {
    //         exchange_contract: harness.exchanger_addr.clone(),
    //         swap_amount: Coin::new(1000u128, "uatom"),
    //         minimum_receive_amount: Coin::new(1u128, "rune"),
    //         maximum_slippage_bps: 50,
    //         adjustment: SwapAmountAdjustment::Fixed,
    //         route: None,
    //     };

    //     let crank_action = ActionConfig::ExecuteStrategy(Schedule {
    //         scheduler: harness.scheduler_addr.clone(),
    //         cadence: Cadence::Cron("invalid cron string".to_string()),
    //         execution_rebate: vec![Coin::new(1u128, harness.base_denom.clone())],
    //     });

    //     let res = harness.create_strategy(
    //         &creator,
    //         &owner,
    //         "Invalid Cron Strategy",
    //         ActionConfig::Compose(Behaviour {
    //             actions: vec![crank_action.clone(), ActionConfig::Swap(swap_action.clone())],
    //             threshold: Threshold::All,
    //         }),
    //     );

    //     assert!(res.is_err());
    //     assert_eq!(
    //         res.unwrap_err().to_string(),
    //         "Generic error: Failed to parse cron string: invalid cron string"
    //     );
    // }

    // #[test]
    // fn test_instantiate_with_deep_recursion_fails() {
    //     let mut harness = CalcTestApp::setup();
    //     let owner = harness.app.api().addr_make("owner");
    //     let creator = harness.app.api().addr_make("creator");

    //     let swap_action = Swap {
    //         exchange_contract: harness.exchanger_addr.clone(),
    //         swap_amount: Coin::new(1000u128, "uatom"),
    //         minimum_receive_amount: Coin::new(1u128, "rune"),
    //         maximum_slippage_bps: 50,
    //         adjustment: SwapAmountAdjustment::Fixed,
    //         route: None,
    //     };

    //     // Create a deeply nested behaviour
    //     let mut deep_behaviour = ActionConfig::Swap(swap_action.clone());
    //     for _ in 0..200 {
    //         // A large number to trigger recursion limit
    //         deep_behaviour = ActionConfig::Compose(Behaviour {
    //             actions: vec![deep_behaviour],
    //             threshold: Threshold::All,
    //         });
    //     }

    //     let res =
    //         harness.create_strategy(&creator, &owner, "Deep Recursion Strategy", deep_behaviour);

    //     assert!(res.is_err());
    //     assert_eq!(
    //         res.unwrap_err().to_string(),
    //         "Generic error: Stack overflow"
    //     );
    // }
}
