use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Coin, Decimal};

use crate::types::{
    Affiliate, Condition, ConditionFilter, DcaSchedule, Destination, ExpectedReturnAmount,
    ManagerConfig, Status, Strategy, StrategyConfig, Trigger,
};

#[cw_serde]
pub struct ManagerInstantiateMsg {
    pub code_id: u64,
    pub fee_collector: Addr,
}

#[cw_serde]
pub struct ManagerMigrateMsg {
    pub code_id: u64,
    pub fee_collector: Addr,
}

#[cw_serde]
pub enum ManagerExecuteMsg {
    InstantiateStrategy {
        owner: Addr,
        label: String,
        strategy: InstantiateStrategyConfig,
    },
    ExecuteStrategy {
        contract_address: Addr,
    },
    PauseStrategy {
        contract_address: Addr,
    },
    ResumeStrategy {
        contract_address: Addr,
    },
    WithdrawFromStrategy {
        contract_address: Addr,
        amounts: Vec<Coin>,
    },
    UpdateStrategy {
        contract_address: Addr,
        update: StrategyConfig,
    },
    UpdateStatus {
        status: Status,
    },
    AddAffiliate {
        affiliate: Affiliate,
    },
    RemoveAffiliate {
        code: String,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum ManagerQueryMsg {
    #[returns(ManagerConfig)]
    Config {},
    #[returns(Strategy)]
    Strategy { address: Addr },
    #[returns(Vec<Strategy>)]
    Strategies {
        owner: Option<Addr>,
        status: Option<Status>,
        start_after: Option<Addr>,
        limit: Option<u16>,
    },
    #[returns(Affiliate)]
    Affiliate { code: String },
    #[returns(Vec<Affiliate>)]
    Affiliates {
        start_after: Option<Addr>,
        limit: Option<u16>,
    },
}

#[cw_serde]
pub enum InstantiateStrategyConfig {
    Dca {
        owner: Addr,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
        schedule: DcaSchedule,
        exchange_contract: Addr,
        scheduler_contract: Addr,
        execution_rebate: Coin,
        affiliate_code: Option<String>,
        mutable_destinations: Vec<Destination>,
        immutable_destinations: Vec<Destination>,
    },
    Custom {},
}

#[cw_serde]
pub struct StrategyInstantiateMsg {
    pub fee_collector: Addr,
    pub strategy: InstantiateStrategyConfig,
}

#[cw_serde]
pub enum StrategyExecuteMsg {
    Execute {},
    Deposit {},
    Withdraw { amounts: Vec<Coin> },
    Pause {},
    Resume {},
    Update { update: StrategyConfig },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum StrategyQueryMsg {
    #[returns(StrategyConfig)]
    Config {},
    #[returns(bool)]
    CanExecute {},
}

#[cw_serde]
pub enum ExchangeExecuteMsg {
    Swap {
        minimum_receive_amount: Coin,
        recipient: Option<Addr>,
    },
    Custom(Binary),
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum ExchangeQueryMsg {
    #[returns(bool)]
    CanSwap {
        swap_amount: Coin,
        minimum_receive_amount: Coin,
    },
    #[returns(Vec<Coin>)]
    Route {
        swap_amount: Coin,
        target_denom: String,
    },
    #[returns(Decimal)]
    SpotPrice {
        swap_denom: String,
        target_denom: String,
    },
    #[returns(ExpectedReturnAmount)]
    ExpectedReceiveAmount {
        swap_amount: Coin,
        target_denom: String,
    },
}

#[cw_serde]
pub struct SchedulerInstantiateMsg {}

#[cw_serde]
pub struct CreateTrigger {
    pub condition: Condition,
    pub to: Addr,
    pub msg: Binary,
}

#[cw_serde]
pub enum SchedulerExecuteMsg {
    CreateTrigger(CreateTrigger),
    DeleteTriggers {},
    SetTriggers(Vec<CreateTrigger>),
    ExecuteTrigger { id: u64 },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum SchedulerQueryMsg {
    #[returns(Vec<Trigger>)]
    Triggers {
        filter: ConditionFilter,
        limit: Option<usize>,
    },
    #[returns(bool)]
    CanExecute { id: u64 },
}
