use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Coin, Decimal, HexBinary};
use rujira_rs::{Asset, CallbackData};

use crate::types::{Condition, ConditionFilter, Status, Strategy, StrategyConfig, Trigger};

#[cw_serde]
pub struct FactoryInstantiateMsg {
    pub checksum: HexBinary,
    pub code_id: u64,
}

#[cw_serde]
pub struct FactoryMigrateMsg {
    pub checksum: HexBinary,
    pub code_id: u64,
}

#[cw_serde]
pub enum FactoryExecuteMsg {
    InstantiateStrategy {
        owner: Addr,
        label: String,
        strategy: StrategyConfig,
    },
    UpdateStatus {
        status: Status,
    },
    Proxy {
        contract_address: Addr,
        msg: StrategyExecuteMsg,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum FactoryQueryMsg {
    #[returns(Strategy)]
    Strategy { address: Addr },
    #[returns(Vec<Strategy>)]
    Strategies {
        owner: Option<Addr>,
        status: Option<Status>,
        start_after: Option<Addr>,
        limit: Option<u32>,
    },
}

#[cw_serde]
pub struct StrategyInstantiateMsg {
    pub strategy: StrategyConfig,
}

#[cw_serde]
pub enum StrategyExecuteMsg {
    Execute {},
    Withdraw { denoms: Vec<String> },
    Pause {},
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
        callback: Option<CallbackData>,
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
