use cosmwasm_schema::{
    cw_serde,
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
};
use cosmwasm_std::{
    to_json_binary, Addr, Binary, Coin, CosmosMsg, Response, StdError, StdResult, Uint256, WasmMsg,
};
use cw_utils::{Duration, Expiration};
use rujira_rs::Asset;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},
}

pub type ContractResult = core::result::Result<Response, ContractError>;

#[cw_serde]
pub enum Schedule {
    Regular {
        duration: Duration,
        start_time: Option<Expiration>,
    },
}

#[derive()]
#[cw_serde]
pub enum StrategyConfig {
    Dca {
        owner: Addr,
        swap_amount: Coin,
        target_denom: Asset,
        schedule: Schedule,
        minimum_receive_amount: Option<String>,
        route: Option<String>,
    },
    New {},
}

#[cw_serde]
pub enum StrategyStatus {
    Active,
    Paused,
    Archived,
}

#[cw_serde]
pub struct Strategy {
    config: StrategyConfig,
    status: StrategyStatus,
}

#[cw_serde]
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

pub struct Contract(pub Addr);

impl Contract {
    pub fn addr(&self) -> Addr {
        self.0.clone()
    }

    pub fn call(&self, msg: Binary, funds: Vec<Coin>) -> StdResult<CosmosMsg> {
        Ok(WasmMsg::Execute {
            contract_addr: self.addr().into(),
            msg,
            funds,
        }
        .into())
    }
}
