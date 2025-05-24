use calc_rs::types::{
    ContractError, ContractResult, NewStatistics, StrategyConfig, StrategyStatistics,
};
use cosmwasm_std::{Coin, Deps, DepsMut, Env, MessageInfo, Reply, StdError, Uint128};

pub trait Runnable {
    fn initialize(&self, deps: DepsMut, env: Env, info: MessageInfo) -> ContractResult;
    fn can_execute(&self, deps: Deps, env: Env) -> Result<(), String>;
    fn execute(&self, deps: Deps, env: Env) -> ContractResult;
    fn handle_execute_reply(&self, deps: DepsMut, env: Env, reply: Reply) -> ContractResult;
    fn can_schedule(&self, deps: Deps, env: Env) -> Result<(), String>;
    fn schedule(&self, deps: DepsMut, env: Env) -> ContractResult;
    fn withdraw(&self, deps: Deps, env: Env, denoms: Vec<String>) -> ContractResult;
    fn pause(&self, deps: Deps, env: Env) -> ContractResult;
    fn statistics(&self) -> StrategyStatistics;
}

impl Runnable for StrategyConfig {
    fn initialize(&self, deps: DepsMut, env: Env, info: MessageInfo) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.initialize(deps, env, info),
            StrategyConfig::New(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }

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

    fn handle_execute_reply(&self, deps: DepsMut, env: Env, reply: Reply) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.handle_execute_reply(deps, env, reply),
            StrategyConfig::New(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }

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

    fn withdraw(&self, deps: Deps, env: Env, denoms: Vec<String>) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.withdraw(deps, env, denoms),
            StrategyConfig::New(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }

    fn pause(&self, deps: Deps, env: Env) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.pause(deps, env),
            StrategyConfig::New(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }

    fn statistics(&self) -> StrategyStatistics {
        match self {
            StrategyConfig::Dca(s) => s.statistics(),
            StrategyConfig::New(_) => StrategyStatistics::New(NewStatistics {
                amount: Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::zero(),
                },
            }),
        }
    }
}
