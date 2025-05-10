use calc_rs::types::{ContractError, StrategyConfig};
use cosmwasm_std::{ContractResult, Deps, DepsMut, Env, MessageInfo, Response, StdResult};

pub trait Validatable {
    fn validate(&self, deps: Deps, info: MessageInfo) -> ContractResult<()>;
}

impl Validatable for StrategyConfig {
    fn validate(&self, deps: Deps, info: MessageInfo) -> ContractResult<()> {
        match self {
            StrategyConfig::DCA { owner, .. } => {
                deps.api
                    .addr_validate(&owner.clone().into_string())
                    .map_err(|_err| {
                        return ContractResult::Err("Failed to validate address".to_string());
                    });

                if info.funds.len() > 1 {
                    return ContractResult::Err("Unauthorized".into());
                }

                ContractResult::Ok(())
            }
            StrategyConfig::New {} => ContractResult::Ok(()),
        }
    }
}

pub trait Executable {
    fn can_execute(&self, deps: DepsMut, info: &MessageInfo) -> ContractResult<bool>;
    fn execute(&self, deps: DepsMut, env: Env, info: MessageInfo) -> ContractResult<Response>;
}

impl Executable for StrategyConfig {
    fn can_execute(&self, _deps: DepsMut, _info: &MessageInfo) -> ContractResult<bool> {
        match self {
            StrategyConfig::DCA { .. } => ContractResult::Ok(true),
            StrategyConfig::New {} => ContractResult::Ok(true),
        }
    }

    fn execute(&self, _deps: DepsMut, _env: Env, _info: MessageInfo) -> ContractResult<Response> {
        ContractResult::Ok(match self {
            StrategyConfig::DCA { .. } => Response::new()
                .add_attribute("strategy", "simple_dca")
                .add_attribute("action", "execute"),
            StrategyConfig::New {} => Response::new()
                .add_attribute("strategy", "new")
                .add_attribute("action", "execute"),
        })
    }
}
