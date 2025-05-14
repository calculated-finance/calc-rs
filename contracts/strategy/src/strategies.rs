use calc_rs::{
    msg::{ExchangeExecuteMsg, FactoryExecuteMsg},
    types::{Contract, ContractResult, DomainEvent, StrategyConfig, StrategyStatus},
};
use cosmwasm_std::{
    to_json_binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError,
    StdResult, SubMsg, SubMsgResult, WasmMsg,
};

use crate::state::{CONFIG, FACTORY};

pub trait Validatable {
    fn validate(&self, deps: Deps, info: MessageInfo) -> StdResult<()>;
}

impl Validatable for StrategyConfig {
    fn validate(&self, deps: Deps, info: MessageInfo) -> StdResult<()> {
        match self {
            StrategyConfig::Dca { owner, .. } => {
                deps.api.addr_validate(&owner.clone().into_string())?;

                if info.funds.len() > 1 {
                    return Err(StdError::generic_err(
                        "Cannot deposit multiple coins to a DCA strategy",
                    ));
                }

                Ok(())
            }
            StrategyConfig::New {} => Err(StdError::generic_err(
                "New strategy validation not implemented".to_string(),
            )),
        }
    }
}

pub trait Executable {
    fn can_execute(&self, deps: Deps, env: Env) -> Result<(), String>;
    fn execute(&self) -> ContractResult;
    fn handle_result(&self, deps: DepsMut, env: Env, reply: Reply) -> ContractResult;
}

impl Executable for StrategyConfig {
    fn can_execute(&self, deps: Deps, env: Env) -> Result<(), String> {
        match self {
            StrategyConfig::Dca { swap_amount, .. } => {
                match deps
                    .querier
                    .query_balance(env.contract.address.clone(), swap_amount.denom.clone())
                {
                    Ok(balance) => {
                        if balance.amount < swap_amount.amount {
                            return Err(format!(
                                "Insufficient balance of {}: {}",
                                balance.denom, balance.amount
                            ));
                        }
                        Ok(())
                    }
                    Err(e) => Err(format!(
                        "Error querying balance for denom {}: {}",
                        swap_amount.denom, e
                    )),
                }
            }
            StrategyConfig::New {} => Err("New strategy execution not implemented".to_string()),
        }
    }

    fn execute(&self) -> ContractResult {
        let mut sub_messages: Vec<SubMsg> = vec![];

        match self {
            StrategyConfig::Dca {
                exchange_contract,
                minimum_receive_amount,
                swap_amount,
                ..
            } => {
                sub_messages.push(SubMsg::reply_always(
                    Contract(exchange_contract.clone()).call(
                        to_json_binary(&ExchangeExecuteMsg::Swap {
                            minimum_receive_amount: minimum_receive_amount.clone(),
                            route: None,
                            callback: None,
                        })?,
                        vec![swap_amount.clone()],
                    )?,
                    0,
                ));
            }
            _ => {}
        };

        // TODO: take fees

        Ok(Response::new().add_submessages(sub_messages))
    }

    fn handle_result(&self, deps: DepsMut, env: Env, reply: Reply) -> ContractResult {
        match reply.result {
            SubMsgResult::Ok(_) => match self {
                StrategyConfig::Dca {
                    owner,
                    swap_amount,
                    minimum_receive_amount,
                    exchange_contract,
                    interval_blocks,
                    ..
                } => {
                    let mut messages: Vec<CosmosMsg> = vec![];
                    let mut events: Vec<DomainEvent> = vec![];

                    match self.can_execute(deps.as_ref(), env.clone()) {
                        Ok(_) => {
                            // TODO: schedule next swap
                            CONFIG.save(
                                deps.storage,
                                &StrategyConfig::Dca {
                                    next_execution_block: Some(env.block.height + interval_blocks),
                                    owner: owner.clone(),
                                    swap_amount: swap_amount.clone(),
                                    minimum_receive_amount: minimum_receive_amount.clone(),
                                    exchange_contract: exchange_contract.clone(),
                                    interval_blocks: interval_blocks.clone(),
                                },
                            )?;
                        }
                        Err(reason) => {
                            CONFIG.save(
                                deps.storage,
                                &StrategyConfig::Dca {
                                    next_execution_block: None,
                                    owner: owner.clone(),
                                    swap_amount: swap_amount.clone(),
                                    minimum_receive_amount: minimum_receive_amount.clone(),
                                    exchange_contract: exchange_contract.clone(),
                                    interval_blocks: interval_blocks.clone(),
                                },
                            )?;
                            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                                contract_addr: FACTORY.load(deps.storage)?.into_string(),
                                msg: to_json_binary(&FactoryExecuteMsg::UpdateHandle {
                                    status: Some(StrategyStatus::Paused),
                                })?,
                                funds: vec![],
                            }));
                            events.push(DomainEvent::StrategyPaused {
                                contract_address: env.contract.address.clone(),
                                reason,
                            });
                        }
                    }
                    Ok(Response::new().add_messages(messages))
                }
                StrategyConfig::New {} => {
                    Ok(Response::new().add_event(DomainEvent::ExecutionFailed {
                        contract_address: env.contract.address.clone(),
                        reason: "New strategy execution not implemented".to_string(),
                    }))
                }
            },
            SubMsgResult::Err(reason) => {
                Ok(Response::new().add_event(DomainEvent::ExecutionFailed {
                    contract_address: env.contract.address.clone(),
                    reason,
                }))
            }
        }
    }
}
