use std::vec;

use calc_rs::{
    manager::{ManagerConfig, ManagerExecuteMsg, ManagerQueryMsg, StrategyHandle},
    scheduler::{
        ConditionFilter, SchedulerExecuteMsg, SchedulerInstantiateMsg, SchedulerQueryMsg, Trigger,
    },
    statistics::Statistics,
    strategy::{Json, Strategy, StrategyConfig, StrategyQueryMsg},
};
use cosmwasm_std::{Addr, Coin, Decimal, StdError, StdResult, Uint128};
use cw_multi_test::{error::AnyResult, App, AppResponse, ContractWrapper, Executor};
use rujira_rs::fin::{
    ConfigResponse, Denoms, ExecuteMsg, InstantiateMsg, OrdersResponse, Price, QueryMsg, Side, Tick,
};

use calc_rs::manager::StrategyStatus;

use strategy::contract::{execute, instantiate, query, reply};

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
                &SchedulerInstantiateMsg {
                    manager: manager_addr.clone(),
                },
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

    pub fn query_fin_config(&self, pair_address: &Addr) -> ConfigResponse {
        self.app
            .wrap()
            .query_wasm_smart::<ConfigResponse>(pair_address, &QueryMsg::Config {})
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
        funds: &[Coin],
    ) -> StdResult<Addr> {
        let msg = ManagerExecuteMsg::InstantiateStrategy {
            owner: owner.clone(),
            label: label.to_string(),
            affiliates: vec![],
            strategy,
        };

        let response = self
            .app
            .execute_contract(sender.clone(), self.manager_addr.clone(), &msg, &funds)
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

    pub fn execute_filtered_triggers(
        &mut self,
        sender: &Addr,
        filter: ConditionFilter,
    ) -> AnyResult<AppResponse> {
        let triggers = self
            .app
            .wrap()
            .query_wasm_smart::<Vec<Trigger>>(
                self.scheduler_addr.clone(),
                &SchedulerQueryMsg::Filtered {
                    filter: filter.clone(),
                    limit: None,
                },
            )
            .unwrap();

        if triggers.is_empty() {
            return Ok(AppResponse::default());
        }

        println!("[CalcTestApp] Executing triggers: {triggers:?}");

        self.app
            .execute_contract(
                sender.clone(),
                self.scheduler_addr.clone(),
                &SchedulerExecuteMsg::Execute(triggers.iter().map(|t| t.id).collect()),
                &[],
            )
            .unwrap();

        Ok(AppResponse::default())
    }

    pub fn execute_owned_triggers(
        &mut self,
        sender: &Addr,
        strategy_addr: &Addr,
    ) -> AnyResult<AppResponse> {
        let triggers = self
            .app
            .wrap()
            .query_wasm_smart::<Vec<Trigger>>(
                self.scheduler_addr.clone(),
                &SchedulerQueryMsg::Owned {
                    owner: strategy_addr.clone(),
                    limit: None,
                    start_after: None,
                },
            )
            .unwrap();

        self.app
            .execute_contract(
                sender.clone(),
                self.scheduler_addr.clone(),
                &SchedulerExecuteMsg::Execute(triggers.iter().map(|t| t.id).collect()),
                &[],
            )
            .unwrap();

        Ok(AppResponse::default())
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

    pub fn update_strategy_status(
        &mut self,
        sender: &Addr,
        strategy_addr: &Addr,
        status: StrategyStatus,
    ) -> AnyResult<AppResponse> {
        self.app.execute_contract(
            sender.clone(),
            self.manager_addr.clone(),
            &ManagerExecuteMsg::UpdateStrategyStatus {
                contract_address: strategy_addr.clone(),
                status,
            },
            &[],
        )
    }
}
