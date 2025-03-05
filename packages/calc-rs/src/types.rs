use cosmwasm_std::{StdResult, Uint256};
use cw_utils::{Duration, Expiration};
use rujira_rs::proto::common::{Asset, Coin};

pub enum Schedule {
    Regular {
        duration: Duration,
        start_time: Option<Expiration>,
    },
}

#[derive()]
pub enum StrategyConfig {
    Regular {
        owner: Addr,
        swap_amount: Coin,
        target_denom: Asset,
        schedule: Schedule,
        minimum_receive_amount: Option<String>,
        route: Option<String>,
    },
}

pub enum StrategyStatus {
    Active,
    Paused,
    Archived,
}

pub struct Strategy {
    config: StrategyConfig,
    status: StrategyStatus,
}

pub enum Event {
    VaultCreated {},
    FundsDeposited {},
    ExecutionSucceeded {},
    ExecutionFailed {},
    VaultUpdated {},
}

pub enum Condition {
    Time { time: Expiration },
    MinimumReturnAmount { amount: Uint256 },
    LimitOrder { order_id: Uint256 },
}
