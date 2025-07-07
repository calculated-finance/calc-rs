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
        manager::{ManagerConfig, ManagerExecuteMsg, ManagerQueryMsg, StrategyHandle},
        scheduler::SchedulerInstantiateMsg,
        statistics::Statistics,
        strategy::{Idle, Json, Strategy, StrategyConfig, StrategyQueryMsg},
    };
    use cosmwasm_std::{Addr, Coin, Decimal, StdError, StdResult, Uint128};
    use cw_multi_test::{error::AnyResult, App, AppResponse, ContractWrapper, Executor};
    use rujira_rs::fin::{
        BookResponse, ConfigResponse, Denoms, ExecuteMsg, InstantiateMsg, OrdersResponse, Price,
        QueryMsg, Side, Tick,
    };

    use calc_rs::actions::limit_order::{LimitOrder, OrderPriceStrategy};
    use calc_rs::manager::StrategyStatus;

    use crate::contract::{execute, instantiate, query, reply};

    pub struct CalcTestApp {
        pub app: App,
        pub fin_addr: Addr,
        pub manager_addr: Addr,
        pub scheduler_addr: Addr,
        pub owner: Addr,
    }

    impl CalcTestApp {
        pub fn setup() -> Self {
            let mut app = App::default();

            let fin_code_id = app.store_code(Box::new(ContractWrapper::new(
                rujira_fin::contract::execute,
                rujira_fin::contract::instantiate,
                rujira_fin::contract::query,
            )));

            let manager_code_id = app.store_code(Box::new(ContractWrapper::new(
                manager::contract::execute,
                manager::contract::instantiate,
                manager::contract::query,
            )));

            let strategy_code_id = app.store_code(Box::new(
                ContractWrapper::new(execute, instantiate, query).with_reply(reply),
            ));

            let scheduler_code_id = app.store_code(Box::new(
                ContractWrapper::new(
                    scheduler::contract::execute,
                    scheduler::contract::instantiate,
                    scheduler::contract::query,
                )
                .with_reply(reply),
            ));

            let admin = app.api().addr_make("admin");
            let owner = app.api().addr_make("owner");

            let base_denom = "rune";
            let quote_denom = "x/ruji";

            let fin_addr = app
                .instantiate_contract(
                    fin_code_id,
                    admin.clone(),
                    &InstantiateMsg {
                        denoms: Denoms::new(base_denom, quote_denom),
                        market_maker: None,
                        oracles: None,
                        tick: Tick::new(6u8),
                        fee_taker: Decimal::zero(),
                        fee_maker: Decimal::zero(),
                        fee_address: app.api().addr_make("fee").to_string(),
                    },
                    &[],
                    "Fin Pair",
                    Some(admin.clone().to_string()),
                )
                .unwrap();

            let manager_addr = app
                .instantiate_contract(
                    manager_code_id,
                    admin.clone(),
                    &ManagerConfig {
                        strategy_code_id,
                        fee_collector: Addr::unchecked("fee_collector"),
                    },
                    &[],
                    "calc-manager",
                    Some(admin.clone().to_string()),
                )
                .unwrap();

            let scheduler_addr = app
                .instantiate_contract(
                    scheduler_code_id,
                    admin.clone(),
                    &SchedulerInstantiateMsg {},
                    &[],
                    "calc-scheduler",
                    Some(admin.clone().to_string()),
                )
                .unwrap();

            app.init_modules(|router, _, storage| {
                router
                    .bank
                    .init_balance(
                        storage,
                        &owner,
                        vec![
                            Coin::new(1_000_000_000_000u128, base_denom),
                            Coin::new(1_000_000_000_000u128, quote_denom),
                        ],
                    )
                    .unwrap();
            });

            let orders = vec![
                (
                    Side::Base,
                    Price::Fixed(Decimal::one() + Decimal::percent(1)),
                    Some(Uint128::new(100_000)),
                ),
                (
                    Side::Quote,
                    Price::Fixed(Decimal::one() - Decimal::percent(1)),
                    Some(Uint128::new(100_000)),
                ),
            ];

            app.execute_contract(
                owner.clone(),
                fin_addr.clone(),
                &ExecuteMsg::Order((orders, None)),
                &[
                    Coin::new(100_000u128, base_denom),
                    Coin::new(100_000u128, quote_denom),
                ],
            )
            .unwrap();

            Self {
                app,
                fin_addr,
                manager_addr,
                scheduler_addr,
                owner,
            }
        }

        pub fn place_fin_limit_orders(
            &mut self,
            sender: &Addr,
            pair_address: &Addr,
            orders: Vec<(Side, Price, Option<Uint128>)>,
        ) -> AppResponse {
            let pair = self.query_fin_config(pair_address);

            let funds = orders
                .iter()
                .filter_map(|(side, _, amount)| {
                    let order_denom = pair.denoms.ask(side);
                    amount.map(|amount| Coin::new(amount, order_denom))
                })
                .collect::<Vec<_>>();

            self.app
                .execute_contract(
                    sender.clone(),
                    pair_address.clone(),
                    &ExecuteMsg::Order((orders, None)),
                    &funds,
                )
                .unwrap()
        }

        pub fn query_fin_config(&self, pair_address: &Addr) -> ConfigResponse {
            self.app
                .wrap()
                .query_wasm_smart::<ConfigResponse>(pair_address, &QueryMsg::Config {})
                .unwrap()
        }

        pub fn query_fin_book(
            &self,
            pair_address: &Addr,
            offset: Option<u8>,
            limit: Option<u8>,
        ) -> BookResponse {
            self.app
                .wrap()
                .query_wasm_smart::<BookResponse>(pair_address, &QueryMsg::Book { limit, offset })
                .unwrap()
        }

        pub fn get_fin_orders(
            &self,
            pair_address: &Addr,
            owner: &Addr,
            side: Option<Side>,
            offset: Option<u8>,
            limit: Option<u8>,
        ) -> OrdersResponse {
            self.app
                .wrap()
                .query_wasm_smart::<OrdersResponse>(
                    pair_address,
                    &QueryMsg::Orders {
                        owner: owner.to_string(),
                        side,
                        offset,
                        limit,
                    },
                )
                .unwrap()
        }

        pub fn create_strategy(
            &mut self,
            sender: &Addr,
            owner: &Addr,
            label: &str,
            strategy: Strategy<Json>,
        ) -> StdResult<Addr> {
            let msg = ManagerExecuteMsg::InstantiateStrategy {
                owner: owner.clone(),
                label: label.to_string(),
                affiliates: vec![],
                strategy,
            };

            let response = self
                .app
                .execute_contract(sender.clone(), self.manager_addr.clone(), &msg, &[])
                .unwrap();

            let wasm_event = response
                .events
                .iter()
                .find(|ev| ev.ty == "instantiate")
                .ok_or_else(|| StdError::generic_err("Could not find instantiate event"))?;

            let contract_addr = wasm_event
                .attributes
                .iter()
                .find(|attr| attr.key == "_contract_address")
                .ok_or_else(|| StdError::generic_err("Could not find _contract_address attribute"))?
                .value
                .clone();

            Ok(Addr::unchecked(contract_addr))
        }

        pub fn execute_strategy(
            &mut self,
            sender: &Addr,
            strategy_addr: &Addr,
            funds: &[Coin],
        ) -> AnyResult<AppResponse> {
            self.app.execute_contract(
                sender.clone(),
                self.manager_addr.clone(),
                &ManagerExecuteMsg::ExecuteStrategy {
                    contract_address: strategy_addr.clone(),
                },
                funds,
            )
        }

        pub fn fund_contract(&mut self, target: &Addr, sender: &Addr, funds: &[Coin]) {
            self.app
                .send_tokens(sender.clone(), target.clone(), funds)
                .unwrap();
        }

        pub fn query_strategy(&self, strategy_addr: &Addr) -> StrategyHandle {
            self.app
                .wrap()
                .query_wasm_smart(
                    self.manager_addr.clone(),
                    &ManagerQueryMsg::Strategy {
                        address: strategy_addr.clone(),
                    },
                )
                .unwrap()
        }

        pub fn query_strategy_config(&self, strategy_addr: &Addr) -> StrategyConfig {
            self.app
                .wrap()
                .query_wasm_smart(strategy_addr, &StrategyQueryMsg::Config {})
                .unwrap()
        }

        pub fn query_strategy_stats(&self, strategy_addr: &Addr) -> Statistics {
            self.app
                .wrap()
                .query_wasm_smart(strategy_addr, &StrategyQueryMsg::Statistics {})
                .unwrap()
        }

        pub fn query_balances(&self, addr: &Addr) -> Vec<Coin> {
            #[allow(deprecated)]
            self.app.wrap().query_all_balances(addr).unwrap()
        }

        pub fn advance_blocks(&mut self, count: u64) {
            self.app.update_block(|block| {
                block.height += count;
            });
        }

        pub fn advance_time(&mut self, seconds: u64) {
            self.app.update_block(|block| {
                block.time = block.time.plus_seconds(seconds);
            });
        }
    }

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

        let strategy = Strategy {
            owner: harness.owner.clone(),
            action: Action::OptimalSwap(swap_action.clone()),
            state: Json,
        };

        let strategy_addr = harness
            .create_strategy(
                &harness.owner.clone(),
                &harness.owner.clone(),
                "Simple Swap",
                strategy.clone(),
            )
            .unwrap();

        let config = harness.query_strategy_config(&strategy_addr);

        assert_eq!(
            config,
            StrategyConfig {
                manager: harness.manager_addr.clone(),
                escrowed: HashSet::from([swap_action.minimum_receive_amount.denom.clone()]),
                strategy: Strategy {
                    owner: harness.owner.clone(),
                    action: Action::OptimalSwap(swap_action),
                    state: Idle {
                        contract_address: strategy_addr.clone(),
                    }
                }
            }
        );
    }

    #[test]
    fn test_execute_simple_swap_strategy_updates_balances_and_stats() {
        let mut harness = CalcTestApp::setup();
        let keeper = harness.app.api().addr_make("keeper");

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_route = OptimalSwap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(harness.fin_addr.clone())],
        };

        let strategy_addr = harness
            .create_strategy(
                &harness.owner.clone(),
                &harness.owner.clone(),
                "Simple ATOM->OSMO Swap",
                Strategy {
                    owner: harness.owner.clone(),
                    action: Action::OptimalSwap(swap_route.clone()),
                    state: Json,
                },
            )
            .unwrap();

        harness.fund_contract(
            &strategy_addr,
            &harness.owner.clone(),
            &[swap_route.swap_amount.clone()],
        );

        harness
            .execute_strategy(&keeper, &strategy_addr, &[])
            .unwrap();

        assert_eq!(
            harness.query_balances(&strategy_addr),
            vec![Coin::new(
                swap_route
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99)),
                swap_route.minimum_receive_amount.denom.clone()
            )]
        );

        assert_eq!(
            harness.query_strategy_stats(&strategy_addr),
            Statistics {
                swapped: vec![swap_route.swap_amount],
                ..Statistics::default()
            }
        );
    }

    #[test]
    fn test_execute_strategy_with_unsatisfied_condition_does_nothing() {
        let mut harness = CalcTestApp::setup();
        let keeper = harness.app.api().addr_make("keeper");

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = OptimalSwap {
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            routes: vec![SwapRoute::Fin(harness.fin_addr.clone())],
        };

        let strategy_addr = harness
            .create_strategy(
                &harness.owner.clone(),
                &harness.owner.clone(),
                "Simple ATOM->OSMO Swap",
                Strategy {
                    owner: harness.owner.clone(),
                    action: Action::Conditional((
                        Conditions {
                            conditions: vec![Condition::OwnBalanceAvailable {
                                amount: swap_action.swap_amount.clone(),
                            }],
                            threshold: Threshold::All,
                        },
                        Box::new(Action::OptimalSwap(swap_action.clone())),
                    )),
                    state: Json,
                },
            )
            .unwrap();

        harness.fund_contract(
            &strategy_addr,
            &harness.owner.clone(),
            &[Coin::new(
                swap_action.swap_amount.amount - Uint128::one(),
                fin_pair.denoms.base(),
            )],
        );

        let initial_balances = harness.query_balances(&strategy_addr);
        let initial_stats = harness.query_strategy_stats(&strategy_addr);

        harness
            .execute_strategy(&keeper, &strategy_addr, &[])
            .unwrap();

        assert_eq!(harness.query_balances(&strategy_addr), initial_balances);
        assert_eq!(harness.query_strategy_stats(&strategy_addr), initial_stats);
    }

    #[test]
    fn test_pause_strategy_cancels_open_limit_orders() {
        let mut harness = CalcTestApp::setup();
        let owner = harness.app.api().addr_make("owner");
        let creator = harness.app.api().addr_make("creator");

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let order_action = LimitOrder {
            pair_address: harness.fin_addr.clone(),
            side: Side::Base,
            bid_denom: fin_pair.denoms.base().to_string(),
            bid_amount: Some(Uint128::new(1000u128)),
            strategy: OrderPriceStrategy::Fixed(Decimal::one()),
            current_price: None,
        };

        let strategy_addr = harness
            .create_strategy(
                &creator,
                &owner,
                "Limit Order Strategy",
                Strategy {
                    owner: harness.owner.clone(),
                    action: Action::SetLimitOrder(order_action.clone()),
                    state: Json,
                },
            )
            .unwrap();

        harness.fund_contract(
            &strategy_addr,
            &owner,
            &[Coin::new(
                order_action.bid_amount.unwrap(),
                order_action.bid_denom.clone(),
            )],
        );

        harness
            .execute_strategy(&owner, &strategy_addr, &[])
            .unwrap();

        let orders = harness.get_fin_orders(
            &harness.fin_addr,
            &strategy_addr,
            Some(Side::Base),
            None,
            None,
        );

        assert!(!orders.orders.is_empty());

        harness
            .app
            .execute_contract(
                owner.clone(),
                harness.manager_addr.clone(),
                &ManagerExecuteMsg::UpdateStrategyStatus {
                    contract_address: strategy_addr.clone(),
                    status: StrategyStatus::Paused,
                },
                &[],
            )
            .unwrap();

        let strategy = harness.query_strategy(&strategy_addr);
        assert_eq!(strategy.status, StrategyStatus::Paused);

        let orders = harness.get_fin_orders(
            &harness.fin_addr,
            &strategy_addr,
            Some(Side::Base),
            None,
            None,
        );

        assert!(orders.orders.is_empty());
    }

    #[test]
    fn test_resume_strategy_re_executes_and_places_orders() {
        let mut harness = CalcTestApp::setup();
        let owner = harness.app.api().addr_make("owner");
        let creator = harness.app.api().addr_make("creator");

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let order_action = LimitOrder {
            pair_address: harness.fin_addr.clone(),
            side: Side::Base,
            bid_denom: fin_pair.denoms.base().to_string(),
            bid_amount: Some(Uint128::new(1000u128)),
            strategy: OrderPriceStrategy::Fixed(Decimal::from_ratio(1u128, 1u128)),
            current_price: None,
        };

        let strategy_addr = harness
            .create_strategy(
                &creator,
                &owner,
                "Limit Order Strategy",
                Strategy {
                    owner: harness.owner.clone(),
                    action: Action::SetLimitOrder(order_action.clone()),
                    state: Json,
                },
            )
            .unwrap();

        harness.fund_contract(
            &strategy_addr,
            &owner,
            &[Coin::new(
                order_action.bid_amount.unwrap().u128(),
                order_action.bid_denom.clone(),
            )],
        );

        harness
            .execute_strategy(
                &owner,
                &strategy_addr,
                &[Coin::new(
                    order_action.bid_amount.unwrap().u128(),
                    order_action.bid_denom.clone(),
                )],
            )
            .unwrap();

        let orders = harness.get_fin_orders(
            &harness.fin_addr,
            &strategy_addr,
            Some(Side::Base),
            None,
            None,
        );

        assert!(!orders.orders.is_empty());

        harness
            .app
            .execute_contract(
                owner.clone(),
                harness.manager_addr.clone(),
                &ManagerExecuteMsg::UpdateStrategyStatus {
                    contract_address: strategy_addr.clone(),
                    status: StrategyStatus::Paused,
                },
                &[],
            )
            .unwrap();

        let orders = harness.get_fin_orders(
            &harness.fin_addr,
            &strategy_addr,
            Some(Side::Base),
            None,
            None,
        );

        assert!(orders.orders.is_empty());

        harness
            .app
            .execute_contract(
                owner.clone(),
                harness.manager_addr.clone(),
                &ManagerExecuteMsg::UpdateStrategyStatus {
                    contract_address: strategy_addr.clone(),
                    status: StrategyStatus::Active,
                },
                &[],
            )
            .unwrap();

        let strategy = harness.query_strategy(&strategy_addr);
        assert_eq!(strategy.status, StrategyStatus::Active);

        let orders = harness.get_fin_orders(
            &harness.fin_addr,
            &strategy_addr,
            Some(Side::Base),
            None,
            None,
        );

        assert!(!orders.orders.is_empty());
    }

    #[test]
    fn test_execute_strategy_with_schedule_executes_and_schedules_next() {
        let mut harness = CalcTestApp::setup();
        let keeper = harness.app.api().addr_make("keeper");

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = FinSwap {
            pair_address: harness.fin_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let strategy_addr = harness
            .create_strategy(
                &harness.owner.clone(),
                &harness.owner.clone(),
                "Scheduled Strategy",
                Strategy {
                    owner: harness.owner.clone(),
                    action: Action::Schedule(Schedule {
                        scheduler: harness.scheduler_addr.clone(),
                        cadence: Cadence::Blocks {
                            interval: 5,
                            previous: None,
                        },
                        execution_rebate: vec![],
                        action: Box::new(Action::FinSwap(swap_action.clone())),
                    }),
                    state: Json,
                },
            )
            .unwrap();

        harness.fund_contract(
            &strategy_addr,
            &harness.owner.clone(),
            &[Coin::new(
                swap_action.swap_amount.amount * Uint128::new(10),
                swap_action.swap_amount.denom.clone(),
            )],
        );

        harness
            .execute_strategy(&keeper, &strategy_addr, &[])
            .unwrap();

        assert_eq!(
            harness.query_balances(&strategy_addr),
            vec![
                Coin::new(
                    swap_action.swap_amount.amount * Uint128::new(9),
                    swap_action.swap_amount.denom.clone()
                ),
                Coin::new(
                    swap_action
                        .swap_amount
                        .amount
                        .mul_floor(Decimal::percent(99)),
                    swap_action.minimum_receive_amount.denom.clone()
                )
            ]
        );

        assert_eq!(
            harness.query_strategy_stats(&strategy_addr),
            Statistics {
                swapped: vec![swap_action.swap_amount.clone()],
                ..Statistics::default()
            }
        );

        harness.advance_blocks(2);
        harness
            .execute_strategy(&keeper, &strategy_addr, &[])
            .unwrap();

        assert_eq!(
            harness.query_strategy_stats(&strategy_addr),
            Statistics {
                swapped: vec![swap_action.swap_amount.clone()],
                ..Statistics::default()
            }
        );

        harness.advance_blocks(4);

        harness
            .execute_strategy(&keeper, &strategy_addr, &[])
            .unwrap();

        assert_eq!(
            harness.query_balances(&strategy_addr),
            vec![
                Coin::new(
                    swap_action.swap_amount.amount * Uint128::new(8),
                    swap_action.swap_amount.denom.clone()
                ),
                Coin::new(
                    swap_action
                        .swap_amount
                        .amount
                        .mul_floor(Decimal::percent(99))
                        * Uint128::new(2),
                    swap_action.minimum_receive_amount.denom.clone()
                )
            ]
        );

        assert_eq!(
            harness.query_strategy_stats(&strategy_addr),
            Statistics {
                swapped: vec![Coin::new(
                    swap_action.swap_amount.amount * Uint128::new(2),
                    swap_action.swap_amount.denom.clone()
                ),],
                ..Statistics::default()
            }
        );
    }

    #[test]
    fn test_schedule_action_with_cron_cadence_schedules_correctly() {
        let mut harness = CalcTestApp::setup();
        let owner = harness.app.api().addr_make("owner");
        let creator = harness.app.api().addr_make("creator");
        let keeper = harness.app.api().addr_make("keeper");

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = FinSwap {
            pair_address: harness.fin_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
        };

        let crank_action = Action::Schedule(Schedule {
            scheduler: harness.scheduler_addr.clone(),
            cadence: Cadence::Cron {
                expr: "*/10 * * * * *".to_string(),
                previous: None,
            },
            execution_rebate: vec![],
            action: Box::new(Action::FinSwap(swap_action.clone())),
        });

        let strategy_addr = harness
            .create_strategy(
                &creator,
                &owner,
                "Crank Cron Strategy",
                Strategy {
                    owner: harness.owner.clone(),
                    action: crank_action,
                    state: Json,
                },
            )
            .unwrap();

        harness.fund_contract(
            &strategy_addr,
            &harness.owner.clone(),
            &[Coin::new(
                swap_action.swap_amount.amount * Uint128::new(10),
                swap_action.swap_amount.denom.clone(),
            )],
        );

        harness.advance_time(10);

        harness
            .execute_strategy(&keeper, &strategy_addr, &[])
            .unwrap();

        assert_eq!(
            harness.query_balances(&strategy_addr),
            vec![
                Coin::new(
                    swap_action.swap_amount.amount * Uint128::new(9),
                    swap_action.swap_amount.denom.clone()
                ),
                Coin::new(
                    swap_action
                        .swap_amount
                        .amount
                        .mul_floor(Decimal::percent(99)),
                    swap_action.minimum_receive_amount.denom.clone()
                )
            ]
        );

        assert_eq!(
            harness.query_strategy_stats(&strategy_addr),
            Statistics {
                swapped: vec![swap_action.swap_amount.clone()],
                ..Statistics::default()
            }
        );

        harness.advance_time(2);
        harness
            .execute_strategy(&keeper, &strategy_addr, &[])
            .unwrap();

        assert_eq!(
            harness.query_strategy_stats(&strategy_addr),
            Statistics {
                swapped: vec![swap_action.swap_amount.clone()],
                ..Statistics::default()
            }
        );

        harness.advance_time(10);

        harness
            .execute_strategy(&keeper, &strategy_addr, &[])
            .unwrap();

        assert_eq!(
            harness.query_balances(&strategy_addr),
            vec![
                Coin::new(
                    swap_action.swap_amount.amount * Uint128::new(8),
                    swap_action.swap_amount.denom.clone()
                ),
                Coin::new(
                    swap_action
                        .swap_amount
                        .amount
                        .mul_floor(Decimal::percent(99))
                        * Uint128::new(2),
                    swap_action.minimum_receive_amount.denom.clone()
                )
            ]
        );

        assert_eq!(
            harness.query_strategy_stats(&strategy_addr),
            Statistics {
                swapped: vec![Coin::new(
                    swap_action.swap_amount.amount * Uint128::new(2),
                    swap_action.swap_amount.denom.clone()
                ),],
                ..Statistics::default()
            }
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
