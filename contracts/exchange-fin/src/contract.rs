use calc_rs::msg::{ExchangeExecuteMsg, ExchangeQueryMsg};
use calc_rs::types::{Contract, ContractError, ContractResult};
use cosmwasm_schema::cw_serde;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdError, StdResult,
};
use rujira_rs::fin::{ExecuteMsg as FinExecuteMsg, SwapRequest};

use crate::state::find_pair;
// use cw2::set_contract_version;

/*
// version info for migration info
const CONTRACT_NAME: &str = "crates.io:vault";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
*/

#[cw_serde]
pub struct InstantiateMsg {}

#[entry_point]
pub fn instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: InstantiateMsg,
) -> ContractResult {
    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExchangeExecuteMsg,
) -> ContractResult {
    match msg {
        ExchangeExecuteMsg::Swap {
            minimum_receive_amount,
            callback,
            ..
        } => {
            if info.funds.len() != 1 {
                return Err(ContractError::Std(StdError::generic_err(
                    "Must provide exactly one coin to swap",
                )));
            }

            if info.funds[0].amount.is_zero() {
                return Err(ContractError::Std(StdError::generic_err(
                    "Must provide a non-zero amount to swap",
                )));
            }

            let pair = find_pair(
                deps.storage,
                [
                    info.funds[0].denom.clone(),
                    minimum_receive_amount.denom.clone(),
                ],
            )?;

            Ok(Response::new().add_message(Contract(pair.address).call(
                to_json_binary(&FinExecuteMsg::Swap(SwapRequest {
                    min_return: Some(minimum_receive_amount.amount),
                    to: Some(info.sender.to_string()),
                    callback: Some(callback),
                }))?,
                info.funds,
            )?))
        }
    }
}

#[entry_point]
pub fn query(_deps: Deps, _env: Env, msg: ExchangeQueryMsg) -> StdResult<Binary> {
    match msg {
        ExchangeQueryMsg::GetExpectedReceiveAmount { .. } => {
            unimplemented!()
        }
        ExchangeQueryMsg::GetSpotPrice { .. } => {
            unimplemented!()
        }
    }
}

#[cfg(test)]
mod tests {}
