use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Coin, Decimal, HexBinary};
use rujira_rs::CallbackData;

use crate::types::{Condition, ConditionFilter, Status, Strategy, StrategyConfig};

#[cw_serde]
pub struct FactoryInstantiateMsg {
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
    Execute {
        contract_address: Addr,
    },
    UpdateStatus {
        status: Status,
        reason: String,
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
    Schedule {},
    Withdraw { denoms: Vec<String> },
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
}

#[cw_serde]
pub enum SchedulerExecuteMsg {
    Create {
        condition: Condition,
        to: Addr,
        callback: CallbackData,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum SchedulerQueryMsg {
    #[returns(Vec<Condition>)]
    Get {
        filter: ConditionFilter,
        limit: Option<usize>,
    },
}
