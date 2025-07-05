#[cfg(test)]
mod integration_tests {
    use calc_rs::{
        actions::{
            action::Action,
            behaviour::Behaviour,
            swap::{Swap, SwapAmountAdjustment},
        },
        conditions::Threshold,
        exchanger::ExchangerInstantiateMsg,
        manager::{ManagerConfig, ManagerExecuteMsg},
        scheduler::SchedulerInstantiateMsg,
        statistics::Statistics,
        strategy::{StrategyConfig, StrategyExecuteMsg, StrategyQueryMsg},
    };
    use cosmwasm_std::{Addr, Coin, StdError, StdResult};
    use cw_multi_test::{
        error::{anyhow, AnyResult},
        App, AppResponse, ContractWrapper, Executor,
    };

    use crate::contract::{execute, instantiate, query, reply};

    // --- TEST HARNESS SETUP ---

    pub struct CalcTestApp {
        pub app: cw_multi_test::App,
        pub manager_addr: Addr,
        pub scheduler_addr: Addr,
        pub exchanger_addr: Addr,
        pub strategy_code_id: u64,
    }

    impl CalcTestApp {
        pub fn setup() -> Self {
            let mut app = App::default();

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

            let manager_addr = app
                .instantiate_contract(
                    manager_code_id,
                    Addr::unchecked("admin"),
                    &ManagerConfig {
                        strategy_code_id,
                        fee_collector: Addr::unchecked("fee_collector"),
                    },
                    &[],
                    "calc-manager",
                    Some(Addr::unchecked("admin").to_string()),
                )
                .unwrap();

            let scheduler_addr = app
                .instantiate_contract(
                    scheduler_code_id,
                    Addr::unchecked("admin"),
                    &SchedulerInstantiateMsg {},
                    &[],
                    "calc-scheduler",
                    Some(Addr::unchecked("admin").to_string()),
                )
                .unwrap();

            let exchanger_addr = app
                .instantiate_contract(
                    exchanger_code_id,
                    Addr::unchecked("admin"),
                    &ExchangerInstantiateMsg {
                        scheduler_address: scheduler_addr.clone(),
                    },
                    &[],
                    "calc-exchanger",
                    Some(Addr::unchecked("admin").to_string()),
                )
                .unwrap();

            Self {
                app,
                manager_addr,
                scheduler_addr,
                exchanger_addr,
                strategy_code_id,
            }
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
                strategy_addr.clone(),
                &StrategyExecuteMsg::Execute {},
                &[],
            )
        }

        pub fn fund_contract(&mut self, target: &Addr, sender: &Addr, funds: &[Coin]) {
            self.app
                .send_tokens(target.clone(), sender.clone(), funds)
                .unwrap();
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

    // --- TEST CASES ---

    // Feature: Strategy Lifecycle & Core Operations

    #[test]
    fn test_instantiate_simple_swap_strategy_succeeds() {
        // 1. ARRANGE: Set up the test app and define the strategy components.
        let mut harness = CalcTestApp::setup();
        let owner = Addr::unchecked("owner");
        let creator = Addr::unchecked("creator"); // The user initiating the transaction

        let swap_action = Action::Perform(Swap {
            exchange_contract: harness.exchanger_addr.clone(),
            swap_amount: Coin::new(1000u128, "uatom"),
            minimum_receive_amount: Coin::new(1u128, "uosmo"),
            maximum_slippage_bps: 50, // 0.5%
            adjustment: SwapAmountAdjustment::Fixed,
            route: None, // Let the exchanger decide
        });

        let strategy_behaviour = Action::Exhibit(Behaviour {
            actions: vec![swap_action],
            threshold: Threshold::All,
        });

        // 2. ACT: Call the manager to instantiate the new strategy.
        let strategy_addr_result = harness.create_strategy(
            &creator,
            &owner,
            "Simple ATOM->OSMO Swap",
            strategy_behaviour.clone(),
        );

        // 3. ASSERT: Verify the outcome.

        // Ensure the strategy was created successfully.
        assert!(strategy_addr_result.is_ok());
        let strategy_addr = strategy_addr_result.unwrap();

        // Query the new strategy's config to verify its state.
        let config = harness.query_strategy_config(&strategy_addr);

        assert_eq!(config.owner, owner);
        assert_eq!(config.manager, harness.manager_addr);
        assert_eq!(config.action, strategy_behaviour);
        assert!(config.escrowed.contains("uatom"));
        assert_eq!(config.escrowed.len(), 1);
    }

    #[test]
    fn test_execute_simple_swap_strategy_updates_balances_and_stats() {
        // TODO: Implement test
    }

    #[test]
    fn test_execute_strategy_with_insufficient_funds_does_nothing() {
        // TODO: Implement test
    }

    #[test]
    fn test_pause_strategy_cancels_open_limit_orders() {
        // TODO: Implement test
    }

    #[test]
    fn test_resume_strategy_re_executes_and_places_orders() {
        // TODO: Implement test
    }

    // Feature: Recurring & Scheduled Actions (TWAP/DCA)

    #[test]
    fn test_crank_action_with_block_cadence_executes_and_schedules_next() {
        // TODO: Implement test
    }

    #[test]
    fn test_crank_action_with_cron_cadence_schedules_correctly() {
        // TODO: Implement test
    }

    #[test]
    fn test_execute_trigger_before_due_fails() {
        // TODO: Implement test
    }

    // Feature: Composite Actions & Error Handling

    #[test]
    fn test_composite_all_threshold_halts_on_first_error() {
        // TODO: Implement test
    }

    #[test]
    fn test_composite_any_threshold_executes_all_and_handles_partial_failure() {
        // TODO: Implement test
    }

    // Feature: Security & Edge Cases

    #[test]
    fn test_update_strategy_from_unauthorized_sender_fails() {
        // TODO: Implement test
    }

    #[test]
    fn test_withdraw_from_unauthorized_sender_fails() {
        // TODO: Implement test
    }

    #[test]
    fn test_withdraw_escrowed_funds_fails() {
        // TODO: Implement test
    }

    #[test]
    fn test_instantiate_with_invalid_cron_string_fails() {
        // TODO: Implement test
    }

    #[test]
    fn test_instantiate_with_deep_recursion_fails() {
        // TODO: Implement test
    }
}
