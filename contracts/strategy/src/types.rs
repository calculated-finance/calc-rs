use calc_rs::types::{
    ContractError, ContractResult, NewStatistics, StrategyConfig, StrategyStatistics,
};
use cosmwasm_std::{Coin, Deps, DepsMut, Env, MessageInfo, Reply, StdError, StdResult, Uint128};

pub trait Runnable {
    fn initialize(&self, deps: DepsMut, env: Env, info: MessageInfo) -> ContractResult;
    fn can_execute(&self, deps: Deps, env: Env) -> StdResult<()>;
    fn get_execution_fee(&self, deps: Deps, env: Env) -> StdResult<Coin>;
    fn execute(&mut self, deps: DepsMut, env: Env) -> ContractResult;
    fn handle_reply(&mut self, deps: DepsMut, env: Env, reply: Reply) -> ContractResult;
    fn withdraw(&self, deps: Deps, env: Env, denoms: Vec<Coin>) -> ContractResult;
    fn pause(&self, deps: Deps, env: Env) -> ContractResult;
    fn statistics(&self) -> StrategyStatistics;
}

impl Runnable for StrategyConfig {
    fn initialize(&self, deps: DepsMut, env: Env, info: MessageInfo) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.initialize(deps, env, info),
            StrategyConfig::Custom(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }

    fn can_execute(&self, deps: Deps, env: Env) -> StdResult<()> {
        match self {
            StrategyConfig::Dca(s) => s.can_execute(deps, env),
            StrategyConfig::Custom(_) => Err(StdError::generic_err(
                "New strategy not implemented".to_string(),
            )),
        }
    }

    fn get_execution_fee(&self, deps: Deps, env: Env) -> StdResult<Coin> {
        match self {
            StrategyConfig::Dca(s) => s.get_execution_fee(deps, env),
            StrategyConfig::Custom(_) => Err(StdError::generic_err(
                "New strategy not implemented".to_string(),
            )),
        }
    }

    fn execute(&mut self, deps: DepsMut, env: Env) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.execute(deps, env),
            StrategyConfig::Custom(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }

    fn handle_reply(&mut self, deps: DepsMut, env: Env, reply: Reply) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.handle_reply(deps, env, reply),
            StrategyConfig::Custom(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }

    fn withdraw(&self, deps: Deps, env: Env, denoms: Vec<Coin>) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.withdraw(deps, env, denoms),
            StrategyConfig::Custom(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }

    fn pause(&self, deps: Deps, env: Env) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.pause(deps, env),
            StrategyConfig::Custom(_) => ContractResult::Err(ContractError::Std(
                StdError::generic_err("New strategy not implemented"),
            )),
        }
    }

    fn statistics(&self) -> StrategyStatistics {
        match self {
            StrategyConfig::Dca(s) => s.statistics(),
            StrategyConfig::Custom(_) => StrategyStatistics::New(NewStatistics {
                amount: Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::zero(),
                },
            }),
        }
    }
}
