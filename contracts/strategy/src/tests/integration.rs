#[cfg(test)]
mod integration_tests {
    use std::{collections::HashSet, u128, vec};

    use calc_rs::{
        actions::{
            action::Action,
            behaviour::Behaviour,
            swap::{Swap, SwapAmountAdjustment},
        },
        conditions::{Cadence, Threshold},
        exchanger::{ExchangerInstantiateMsg, Route},
        manager::{ManagerConfig, ManagerExecuteMsg, ManagerQueryMsg, Strategy},
        scheduler::SchedulerInstantiateMsg,
        statistics::Statistics,
        strategy::{StrategyConfig, StrategyExecuteMsg, StrategyQueryMsg},
    };
    use cosmwasm_std::{Addr, Coin, Decimal, StdError, StdResult, Uint128};
    use cw_multi_test::{error::AnyResult, App, AppResponse, ContractWrapper, Executor};
    use rujira_rs::{
        fin::{
            BookResponse, ConfigResponse, Denoms, ExecuteMsg, InstantiateMsg, Price, QueryMsg,
            Side, Tick,
        },
        Layer1Asset,
    };

    use calc_rs::actions::order::{Order, OrderPriceStrategy};
    use calc_rs::manager::StrategyStatus;

    use crate::contract::{execute, instantiate, query, reply};

    // --- TEST HARNESS SETUP ---

    pub struct CalcTestApp {
        pub app: App,
        pub fin_addr: Addr,
        pub base_denom: String,
        pub quote_denom: String,
        pub manager_addr: Addr,
        pub scheduler_addr: Addr,
        pub exchanger_addr: Addr,
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

            let exchanger_code_id = app.store_code(Box::new(
                ContractWrapper::new(
                    exchanger::contract::execute,
                    exchanger::contract::instantiate,
                    exchanger::contract::query,
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

            let exchanger_addr = app
                .instantiate_contract(
                    exchanger_code_id,
                    admin.clone(),
                    &ExchangerInstantiateMsg {
                        scheduler_address: scheduler_addr.clone(),
                    },
                    &[],
                    "calc-exchanger",
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
                            Coin::new(u128::MAX, base_denom),
                            Coin::new(u128::MAX, quote_denom),
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
                owner,
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
                base_denom: base_denom.to_string(),
                quote_denom: quote_denom.to_string(),
                manager_addr,
                scheduler_addr,
                exchanger_addr,
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

        pub fn create_strategy(
            &mut self,
            sender: &Addr,
            owner: &Addr,
            label: &str,
            action: Action,
        ) -> StdResult<Addr> {
            let msg = ManagerExecuteMsg::InstantiateStrategy {
                owner: owner.clone(),
                label: label.to_string(),
                affiliates: vec![],
                action,
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
        ) -> AnyResult<AppResponse> {
            self.app.execute_contract(
                sender.clone(),
                self.manager_addr.clone(),
                &ManagerExecuteMsg::ExecuteStrategy {
                    contract_address: strategy_addr.clone(),
                },
                &[],
            )
        }

        pub fn fund_contract(&mut self, target: &Addr, sender: &Addr, funds: &[Coin]) {
            self.app
                .send_tokens(sender.clone(), target.clone(), funds)
                .unwrap();
        }

        pub fn query_strategy(&self, strategy_addr: &Addr) -> Strategy {
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
            self.app.wrap().query_all_balances(addr).unwrap()
        }

        pub fn advance_blocks(&mut self, count: u64) {
            self.app.update_block(|block| {
                block.height += count;
            });
        }

        pub fn advance_time_seconds(&mut self, seconds: u64) {
            self.app.update_block(|block| {
                block.time = block.time.plus_seconds(seconds);
            });
        }
    }

    #[test]
    fn test_instantiate_strategy_succeeds() {
        let mut harness = CalcTestApp::setup();
        let owner = harness.app.api().addr_make("owner");
        let creator = harness.app.api().addr_make("creator");

        let swap_action = Swap {
            exchange_contract: harness.exchanger_addr.clone(),
            swap_amount: Coin::new(1000u128, "uatom"),
            minimum_receive_amount: Coin::new(1u128, "rune"),
            maximum_slippage_bps: 50,
            adjustment: SwapAmountAdjustment::Fixed,
            route: None,
        };

        let strategy_behaviour = Action::Exhibit(Behaviour {
            actions: vec![Action::Perform(swap_action.clone())],
            threshold: Threshold::All,
        });

        let strategy_addr = harness
            .create_strategy(
                &creator,
                &owner,
                "Simple ATOM->OSMO Swap",
                strategy_behaviour.clone(),
            )
            .unwrap();

        let config = harness.query_strategy_config(&strategy_addr);

        assert_eq!(
            config,
            StrategyConfig {
                manager: harness.manager_addr.clone(),
                owner: owner.clone(),
                escrowed: HashSet::from([swap_action.minimum_receive_amount.denom.clone()]),
                action: strategy_behaviour
            }
        );
    }

    #[test]
    fn test_execute_simple_swap_strategy_updates_balances_and_stats() {
        let mut harness = CalcTestApp::setup();
        let owner = harness.app.api().addr_make("owner");
        let creator = harness.app.api().addr_make("creator");
        let keeper = harness.app.api().addr_make("keeper");

        let fin_pair = harness.query_fin_config(&harness.fin_addr);

        let swap_action = Swap {
            exchange_contract: harness.exchanger_addr.clone(),
            swap_amount: Coin::new(1000u128, fin_pair.denoms.base()),
            minimum_receive_amount: Coin::new(1u128, fin_pair.denoms.quote()),
            maximum_slippage_bps: 101,
            adjustment: SwapAmountAdjustment::Fixed,
            route: Some(Route::FinMarket {
                address: harness.fin_addr.clone(),
            }),
        };

        let strategy_addr = harness
            .create_strategy(
                &creator,
                &owner,
                "Simple ATOM->OSMO Swap",
                Action::Exhibit(Behaviour {
                    actions: vec![Action::Perform(swap_action.clone())],
                    threshold: Threshold::All,
                }),
            )
            .unwrap();

        harness.fund_contract(&strategy_addr, &owner, &[swap_action.swap_amount.clone()]);
        harness.execute_strategy(&keeper, &strategy_addr).unwrap();

        assert_eq!(
            harness.query_balances(&strategy_addr),
            vec![Coin::new(
                swap_action
                    .swap_amount
                    .amount
                    .mul_floor(Decimal::percent(99)),
                swap_action.minimum_receive_amount.denom.clone()
            )]
        );

        assert_eq!(
            harness.query_strategy_stats(&strategy_addr),
            Statistics {
                swapped: vec![swap_action.swap_amount],
                ..Statistics::default()
            }
        );
    }

    #[test]
    fn test_execute_strategy_with_insufficient_funds_does_nothing() {}

    #[test]
    fn test_pause_strategy_cancels_open_limit_orders() {}

    #[test]
    fn test_resume_strategy_re_executes_and_places_orders() {}

    #[test]
    fn test_crank_action_with_block_cadence_executes_and_schedules_next() {}

    #[test]
    fn test_crank_action_with_cron_cadence_schedules_correctly() {}

    #[test]
    fn test_execute_trigger_before_due_fails() {}

    #[test]
    fn test_composite_all_threshold_halts_on_first_error() {}

    #[test]
    fn test_composite_any_threshold_executes_all_and_handles_partial_failure() {}

    #[test]
    fn test_update_strategy_from_unauthorized_sender_fails() {}

    #[test]
    fn test_withdraw_from_unauthorized_sender_fails() {}

    #[test]
    fn test_withdraw_escrowed_funds_fails() {}

    #[test]
    fn test_instantiate_with_invalid_cron_string_fails() {}

    #[test]
    fn test_instantiate_with_deep_recursion_fails() {}
}
