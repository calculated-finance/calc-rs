use calc_rs::types::{
    AccumulatorExecuteMsg, AccumulatorQueryMsg, ContractError, ContractResult, DcaInstantiateMsg,
    DomainEvent,
};
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_json_binary, BankMsg, Binary, Coin, Decimal, Deps, DepsMut, Env, MessageInfo, Response,
    StdError, StdResult, Uint128,
};

use crate::state::CONFIG;

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: DcaInstantiateMsg,
) -> ContractResult {
    CONFIG.save(
        deps.storage,
        &AccumulatorStrategyConfig {
            owner: msg.owner,
            denom: msg.denom,
            threshold: msg.threshold,
            min_amount: msg.min_amount,
            max_amount: msg.max_amount,
            fee_rate: msg.fee_rate,
        },
    )?;

    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: AccumulatorExecuteMsg,
) -> ContractResult {
    Ok(Response::default())
    // match msg {}
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: AccumulatorQueryMsg) -> StdResult<Binary> {
    Ok(to_json_binary(&"test".to_string())?)
    // match msg {}
}
