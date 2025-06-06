use calc_rs::types::{
    ContractError, ContractResult, NewStatistics, StrategyConfig, StrategyStatistics,
};
use cosmwasm_std::{Coin, Deps, Env, MessageInfo, Reply, StdError, StdResult, Uint128};

pub trait Runnable {
    fn validate(&self, deps: Deps) -> StdResult<()>;
    fn instantiate(&mut self, deps: Deps, env: Env, info: MessageInfo) -> ContractResult;
    fn update(&mut self, deps: Deps, env: Env, info: StrategyConfig) -> ContractResult;
    fn can_execute(&self, deps: Deps, env: &Env) -> StdResult<()>;
    fn execute(&mut self, deps: Deps, env: Env) -> ContractResult;
    fn handle_reply(&mut self, deps: Deps, env: Env, reply: Reply) -> ContractResult;
    fn deposit(&mut self, deps: Deps, env: Env, info: MessageInfo) -> ContractResult;
    fn withdraw(&mut self, deps: Deps, env: Env, amounts: Vec<Coin>) -> ContractResult;
    fn pause(&mut self, deps: Deps, env: Env) -> ContractResult;
    fn resume(&mut self, deps: Deps, env: Env) -> ContractResult;
    fn statistics(&self) -> StrategyStatistics;
}

impl Runnable for StrategyConfig {
    fn validate(&self, deps: Deps) -> StdResult<()> {
        match self {
            StrategyConfig::Dca(s) => s.validate(deps),
            StrategyConfig::Custom(_) => Err(StdError::generic_err("New strategy not implemented")),
        }
    }

    fn instantiate(&mut self, deps: Deps, env: Env, info: MessageInfo) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.instantiate(deps, env, info),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn update(&mut self, deps: Deps, env: Env, info: StrategyConfig) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.update(deps, env, info),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn can_execute(&self, deps: Deps, env: &Env) -> StdResult<()> {
        match self {
            StrategyConfig::Dca(s) => s.can_execute(deps, env),
            StrategyConfig::Custom(_) => Err(StdError::generic_err("New strategy not implemented")),
        }
    }

    fn execute(&mut self, deps: Deps, env: Env) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.execute(deps, env),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn handle_reply(&mut self, deps: Deps, env: Env, reply: Reply) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.handle_reply(deps, env, reply),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn deposit(&mut self, deps: Deps, env: Env, info: MessageInfo) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.deposit(deps, env, info),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn withdraw(&mut self, deps: Deps, env: Env, amounts: Vec<Coin>) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.withdraw(deps, env, amounts),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn pause(&mut self, deps: Deps, env: Env) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.pause(deps, env),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn resume(&mut self, deps: Deps, env: Env) -> ContractResult {
        match self {
            StrategyConfig::Dca(s) => s.resume(deps, env),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
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
