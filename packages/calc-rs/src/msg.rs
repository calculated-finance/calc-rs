use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Coin, Decimal, HexBinary};
use rujira_rs::{Asset, CallbackData};

use crate::types::{
    Affiliate, Condition, ConditionFilter, ManagerConfig, Status, Strategy, StrategyConfig, Trigger,
};

#[cw_serde]
pub struct ManagerInstantiateMsg {
    pub checksum: HexBinary,
    pub code_id: u64,
}

#[cw_serde]
pub struct ManagerMigrateMsg {
    pub checksum: HexBinary,
    pub code_id: u64,
}

#[cw_serde]
pub enum ManagerExecuteMsg {
    InstantiateStrategy {
        owner: Addr,
        label: String,
        strategy: StrategyConfig,
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
pub struct StrategyInstantiateMsg {
    pub strategy: StrategyConfig,
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
        route: Option<Binary>,
    },
    Custom(Binary),
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum ExchangeQueryMsg {
    #[returns(Decimal)]
    GetSpotPrice {
        swap_denom: String,
        target_denom: String,
        period: u64,
        route: Option<Binary>,
    },
    #[returns(Coin)]
    GetExpectedReceiveAmount {
        swap_amount: Coin,
        target_denom: String,
        route: Option<Binary>,
    },
    #[returns(Decimal)]
    GetUsdPrice { asset: Asset },
}

#[cw_serde]
pub enum SchedulerExecuteMsg {
    CreateTrigger {
        condition: Condition,
        to: Addr,
        callback: CallbackData,
    },
    DeleteTriggers {},
    ExecuteTrigger {
        id: u64,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum SchedulerQueryMsg {
    #[returns(Vec<Trigger>)]
    Get {
        filter: ConditionFilter,
        limit: Option<usize>,
    },
    #[returns(bool)]
    CanExecute { id: u64 },
}
