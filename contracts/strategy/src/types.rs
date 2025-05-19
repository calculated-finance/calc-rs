use calc_rs::types::{ContractError, ContractResult, StrategyConfig};
use cosmwasm_std::{Deps, DepsMut, Env, MessageInfo, Reply, StdError, StdResult};

pub trait Validatable {
    fn validate(&self, deps: Deps, info: MessageInfo) -> StdResult<()>;
}

impl Validatable for StrategyConfig {
    fn validate(&self, deps: Deps, info: MessageInfo) -> StdResult<()> {
        match self {
            StrategyConfig::Dca(s) => s.validate(deps, info),
            StrategyConfig::New(_) => Err(StdError::generic_err("New strategy not implemented")),
        }
    }
}

pub trait Executable {
    fn can_execute(&self, deps: Deps, env: Env) -> Result<(), String>;
    fn execute(&self, deps: Deps, env: Env) -> ContractResult;
    fn handle_reply(&self, env: Env, reply: Reply) -> ContractResult;
}

impl Executable for StrategyConfig {
    fn can_execute(&self, deps: Deps, env: Env) -> Result<(), String> {
        match self {
            StrategyConfig::Dca(s) => s.can_execute(deps, env),
            StrategyConfig::New(_) => Err("New strategy not implemented".to_string()),
        }
    }

    fn execute(&self, deps: Deps, env: Env) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.execute(deps, env),
            StrategyConfig::New(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }

    fn handle_reply(&self, env: Env, reply: Reply) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.handle_reply(env, reply),
            StrategyConfig::New(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }
}

pub trait Schedulable {
    fn can_schedule(&self, deps: Deps, env: Env) -> Result<(), String>;
    fn schedule(&self, deps: DepsMut, env: Env) -> ContractResult;
}

impl Schedulable for StrategyConfig {
    fn can_schedule(&self, deps: Deps, env: Env) -> Result<(), String> {
        match self {
            StrategyConfig::Dca(s) => s.can_schedule(deps, env),
            StrategyConfig::New(_) => Err("New strategy not implemented".to_string()),
        }
    }

    fn schedule(&self, deps: DepsMut, env: Env) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.schedule(deps, env),
            StrategyConfig::New(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }
}

pub trait Withdrawable {
    fn withdraw(&self, deps: Deps, env: Env, denoms: Vec<String>) -> ContractResult;
}

impl Withdrawable for StrategyConfig {
    fn withdraw(&self, deps: Deps, env: Env, denoms: Vec<String>) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.withdraw(deps, env, denoms),
            StrategyConfig::New(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }
}
