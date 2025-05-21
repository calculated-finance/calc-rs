use calc_rs::{
    msg::{ExchangeExecuteMsg, FactoryExecuteMsg, SchedulerExecuteMsg, StrategyExecuteMsg},
    types::{
        Condition, Contract, ContractResult, DcaStrategy, DomainEvent, Owned, Status,
        StrategyConfig,
    },
};
use cosmwasm_std::{
    to_json_binary, BankMsg, CosmosMsg, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError,
    StdResult, SubMsg, SubMsgResult, WasmMsg,
};
use rujira_rs::CallbackData;

use crate::{
    state::{FACTORY, STRATEGY},
    types::{Executable, Pausable, Schedulable, Validatable, Withdrawable},
};

impl Validatable for DcaStrategy {
    fn validate(&self, deps: Deps, info: MessageInfo) -> StdResult<()> {
        deps.api.addr_validate(&self.owner.clone().into_string())?;

        if info.funds.len() > 1 {
            return Err(StdError::generic_err(
                "Cannot deposit multiple coins to a DCA strategy",
            ));
        }

        Ok(())
    }
}

impl Executable for DcaStrategy {
    fn can_execute(&self, deps: Deps, env: Env) -> Result<(), String> {
        match deps
            .querier
            .query_balance(env.contract.address.clone(), self.swap_amount.denom.clone())
        {
            Ok(balance) => {
                if balance.amount < self.swap_amount.amount {
                    return Err(format!(
                        "Insufficient balance of {}: {}",
                        balance.denom, balance.amount
                    ));
                }
                Ok(())
            }
            Err(e) => Err(format!(
                "Error querying balance for denom {}: {}",
                self.swap_amount.denom, e
            )),
        }
    }

    fn execute(&self, deps: Deps, env: Env) -> ContractResult {
        let mut sub_messages: Vec<SubMsg> = vec![];
        let mut messages: Vec<CosmosMsg> = vec![];
        let mut events: Vec<DomainEvent> = vec![];

        match self.can_execute(deps, env.clone()) {
            Ok(_) => {
                let swap_msg = SubMsg::reply_always(
                    Contract(self.exchange_contract.clone()).call(
                        to_json_binary(&ExchangeExecuteMsg::Swap {
                            minimum_receive_amount: self.minimum_receive_amount.clone(),
                            route: None,
                            callback: None,
                        })?,
                        vec![self.swap_amount.clone()],
                    )?,
                    0,
                );

                sub_messages.push(swap_msg);

                let schedule_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: env.contract.address.to_string(),
                    msg: to_json_binary(&StrategyExecuteMsg::Schedule {})?,
                    funds: vec![],
                });

                messages.push(schedule_msg);
            }
            Err(reason) => {
                events.push(DomainEvent::ExecutionSkipped {
                    contract_address: env.contract.address,
                    reason,
                });
            }
        }

        Ok(Response::new()
            .add_events(events)
            .add_submessages(sub_messages))
    }

    fn handle_reply(&self, env: Env, reply: Reply) -> ContractResult {
        let mut events: Vec<DomainEvent> = vec![];

        match reply.result {
            SubMsgResult::Ok(_) => {
                events.push(DomainEvent::ExecutionSucceeded {
                    contract_address: env.contract.address.clone(),
                });
            }
            SubMsgResult::Err(reason) => {
                events.push(DomainEvent::ExecutionFailed {
                    contract_address: env.contract.address.clone(),
                    reason,
                });
            }
        }

        Ok(Response::new().add_events(events))
    }
}

impl Schedulable for DcaStrategy {
    fn can_schedule(&self, deps: Deps, env: Env) -> Result<(), String> {
        match deps
            .querier
            .query_balance(env.contract.address.clone(), self.swap_amount.denom.clone())
        {
            Ok(balance) => {
                if balance.amount < self.swap_amount.amount {
                    return Err(format!(
                        "Insufficient balance of {}: {}",
                        balance.denom, balance.amount
                    ));
                }
                Ok(())
            }
            Err(e) => Err(format!(
                "Error querying balance for denom {}: {}",
                self.swap_amount.denom, e
            )),
        }
    }

    fn schedule(&self, deps: DepsMut, env: Env) -> ContractResult {
        let mut messages: Vec<CosmosMsg> = vec![];

        match self.can_schedule(deps.as_ref(), env.clone()) {
            Ok(_) => {
                let create_trigger_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: self.scheduler_contract.to_string(),
                    msg: to_json_binary(&SchedulerExecuteMsg::CreateTrigger {
                        condition: Condition::BlockHeight {
                            height: env.block.height + self.interval_blocks,
                        },
                        to: env.contract.address.clone(),
                        callback: CallbackData(to_json_binary(&StrategyExecuteMsg::Execute {})?),
                    })?,
                    funds: vec![],
                });

                messages.push(create_trigger_msg);
            }
            Err(reason) => {
                STRATEGY.save(
                    deps.storage,
                    &StrategyConfig::Dca(DcaStrategy {
                        conditions: vec![],
                        ..self.clone()
                    }),
                )?;

                let pause_strategy_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: FACTORY.load(deps.storage)?.into_string(),
                    msg: to_json_binary(&FactoryExecuteMsg::UpdateStatus {
                        status: Status::Paused,
                        reason: reason.clone(),
                    })?,
                    funds: vec![],
                });

                messages.push(pause_strategy_msg);
            }
        };

        Ok(Response::new().add_messages(messages))
    }
}

impl Withdrawable for DcaStrategy {
    fn withdraw(&self, deps: Deps, env: Env, denoms: Vec<String>) -> ContractResult {
        let send_assets_msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: STRATEGY.load(deps.storage)?.owner().to_string(),
            amount: denoms
                .iter()
                .map(|denom| {
                    deps.querier
                        .query_balance(env.contract.address.clone(), denom.clone())
                })
                .collect::<StdResult<Vec<_>>>()?,
        });

        let pause_strategy_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: FACTORY.load(deps.storage)?.into_string(),
            msg: to_json_binary(&FactoryExecuteMsg::UpdateStatus {
                status: Status::Paused,
                reason: "User requested withdrawal".into(),
            })?,
            funds: vec![],
        });

        Ok(Response::default()
            .add_message(send_assets_msg)
            .add_message(pause_strategy_msg))
    }
}

impl Pausable for DcaStrategy {
    fn pause(&self, deps: Deps, env: Env) -> ContractResult {
        // delete triggers

        let pause_strategy_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: FACTORY.load(deps.storage)?.into_string(),
            msg: to_json_binary(&FactoryExecuteMsg::UpdateStatus {
                status: Status::Paused,
                reason: "User requested pause".into(),
            })?,
            funds: vec![],
        });

        Ok(Response::default().add_message(pause_strategy_msg))
    }
}
