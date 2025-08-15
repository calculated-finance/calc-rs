use cosmwasm_std::{
    Addr, Binary, CheckedFromRatioError, CheckedMultiplyRatioError, Coin, CoinsError, CosmosMsg,
    Instantiate2AddressError, OverflowError, Response, StdError, WasmMsg,
};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Instantiate2Address(#[from] Instantiate2AddressError),

    #[error("{0}")]
    CheckedMultiplyRatioError(#[from] CheckedMultiplyRatioError),

    #[error("{0}")]
    OverflowError(#[from] OverflowError),

    #[error("{0}")]
    CheckedFromRatioError(#[from] CheckedFromRatioError),

    #[error("{0}")]
    CoinsError(#[from] CoinsError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Generic error: {0}")]
    Generic(&'static str),
}

impl ContractError {
    pub fn generic_err(msg: impl Into<String>) -> Self {
        ContractError::Std(StdError::generic_err(msg.into()))
    }
}

pub type ContractResult = Result<Response, ContractError>;

pub struct Contract(pub Addr);

impl Contract {
    pub fn call(&self, msg: Binary, funds: Vec<Coin>) -> CosmosMsg {
        WasmMsg::Execute {
            contract_addr: self.0.clone().into(),
            msg,
            funds,
        }
        .into()
    }
}
