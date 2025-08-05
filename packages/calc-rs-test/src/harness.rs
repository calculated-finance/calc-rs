use std::{collections::HashSet, vec};

use calc_rs::{
    manager::{Affiliate, ManagerConfig, ManagerExecuteMsg, ManagerQueryMsg, Strategy},
    scheduler::{
        ConditionFilter, SchedulerExecuteMsg, SchedulerInstantiateMsg, SchedulerQueryMsg, Trigger,
    },
    strategy::{Node, StrategyConfig, StrategyExecuteMsg, StrategyQueryMsg},
};
use cosmwasm_std::{Addr, Coin, Decimal, StdError, Uint128};
use cw_multi_test::{error::AnyResult, AppResponse, BasicAppBuilder, ContractWrapper, Executor};
use rujira_rs::fin::{
    ConfigResponse, Denoms, ExecuteMsg, InstantiateMsg, OrdersResponse, Price, QueryMsg, Side, Tick,
};

use calc_rs::manager::StrategyStatus;

use strategy::contract::{execute, instantiate, query, reply};

use crate::stargate::{RujiraApp, RujiraStargate};

pub struct CalcTestApp {
    pub app: RujiraApp,
    pub fin_addr: Addr,
    pub manager_addr: Addr,
    pub scheduler_addr: Addr,
    pub fee_collector_addr: Addr,
    pub owner: Addr,
    pub unknown: Addr,
}

impl CalcTestApp {
    pub fn setup() -> Self {
        let mut app = BasicAppBuilder::new()
            .with_stargate(RujiraStargate::default())
            .build(|_, _, _| {});

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
        let fee_collector_addr = app.api().addr_make("fee_collector");
        let owner = app.api().addr_make("owner");
        let unknown = app.api().addr_make("unknown");

        let base_denom = "rune";
        let quote_denom = "eth-usdc";

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
                    fee_collector: fee_collector_addr.clone(),
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
                        Coin::new(1_000_000_000u128, base_denom),
                        Coin::new(1_000_000_000u128, quote_denom),
                        Coin::new(1_000_000_000u128, "x/ruji"),
                    ],
                )
                .unwrap();
            router
                .bank
                .init_balance(
                    storage,
                    &unknown,
                    vec![
                        Coin::new(1_000_000_000u128, base_denom),
                        Coin::new(1_000_000_000u128, quote_denom),
                        Coin::new(1_000_000_000u128, "x/ruji"),
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
            fee_collector_addr,
            owner,
            unknown,
        }
    }

    pub fn set_fin_orders(
        &mut self,
        owner: &Addr,
        pair_address: &Addr,
        orders: Vec<(Side, Price, Option<Uint128>)>,
        funds: &[Coin],
    ) -> AnyResult<AppResponse> {
        self.app
            .execute_contract(
                owner.clone(),
                pair_address.clone(),
                &ExecuteMsg::Order((orders, None)),
                funds,
            )
            .unwrap();

        Ok(AppResponse::default())
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
        owner: &Addr,
        label: &str,
        affiliates: Vec<Affiliate>,
        nodes: Vec<Node>,
        funds: &[Coin],
    ) -> AnyResult<Addr> {
        let msg = ManagerExecuteMsg::Instantiate {
            owner: owner.clone(),
            label: label.to_string(),
            affiliates,
            nodes,
        };

        let response = self.app.execute_contract(
            self.owner.clone(),
            self.manager_addr.clone(),
            &msg,
            funds,
        )?;

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

    pub fn execute_strategy(
        &mut self,
        sender: &Addr,
        strategy_addr: &Addr,
    ) -> AnyResult<AppResponse> {
        self.app.execute_contract(
            sender.clone(),
            self.manager_addr.clone(),
            &ManagerExecuteMsg::Execute {
                contract_address: strategy_addr.clone(),
            },
            &[],
        )
    }

    pub fn fund_contract(&mut self, sender: &Addr, contract_address: &Addr, funds: &[Coin]) {
        self.app
            .send_tokens(sender.clone(), contract_address.clone(), funds)
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

    pub fn query_strategy_balances(
        &self,
        strategy_addr: &Addr,
        denoms: HashSet<String>,
    ) -> Vec<Coin> {
        self.app
            .wrap()
            .query_wasm_smart(strategy_addr, &StrategyQueryMsg::Balances(denoms))
            .unwrap()
    }

    pub fn query_balances(&self, addr: &Addr) -> Vec<Coin> {
        #[allow(deprecated)]
        self.app.wrap().query_all_balances(addr).unwrap()
    }

    pub fn query_balance(&self, addr: &Addr, denom: &str) -> Coin {
        self.app
            .wrap()
            .query_balance(addr, denom)
            .unwrap_or_else(|_| Coin::new(0u128, denom))
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
            &ManagerExecuteMsg::UpdateStatus {
                contract_address: strategy_addr.clone(),
                status,
            },
            &[],
        )
    }

    pub fn withdraw(
        &mut self,
        sender: &Addr,
        strategy_addr: &Addr,
        amounts: Vec<Coin>,
    ) -> AnyResult<AppResponse> {
        self.app.execute_contract(
            sender.clone(),
            strategy_addr.clone(),
            &StrategyExecuteMsg::Withdraw(amounts),
            &[],
        )
    }
}
