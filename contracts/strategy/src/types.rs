use calc_rs::types::{
    ContractError, ContractResult, NewStatistics, StrategyConfig, StrategyStatistics,
};
use cosmwasm_std::{Binary, Coin, Deps, Env, MessageInfo, Reply, StdError, StdResult, Uint128};

pub trait Runnable {
    fn instantiate(&mut self, deps: Deps, env: &Env, info: &MessageInfo) -> ContractResult;
    fn validate(&self, deps: Deps) -> StdResult<()>;
    fn update(&mut self, deps: Deps, env: &Env, info: StrategyConfig) -> ContractResult;
    fn can_execute(&self, deps: Deps, env: &Env, msg: Option<Binary>) -> StdResult<()>;
    fn execute(&mut self, deps: Deps, env: &Env, msg: Option<Binary>) -> ContractResult;
    fn handle_reply(&mut self, deps: Deps, env: &Env, reply: Reply) -> ContractResult;
    fn deposit(&mut self, deps: Deps, env: &Env, info: &MessageInfo) -> ContractResult;
    fn withdraw(&mut self, deps: Deps, env: &Env, amounts: Vec<Coin>) -> ContractResult;
    fn pause(&mut self, deps: Deps, env: &Env) -> ContractResult;
    fn resume(&mut self, deps: Deps, env: &Env) -> ContractResult;
    fn statistics(&self) -> StrategyStatistics;
}

impl Runnable for StrategyConfig {
    fn validate(&self, deps: Deps) -> StdResult<()> {
        match self {
            StrategyConfig::Accumulate(s) => s.validate(deps),
            StrategyConfig::Custom(_) => Err(StdError::generic_err("New strategy not implemented")),
        }
    }

    fn instantiate(&mut self, deps: Deps, env: &Env, info: &MessageInfo) -> ContractResult {
        match self {
            StrategyConfig::Accumulate(s) => s.instantiate(deps, env, info),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn update(&mut self, deps: Deps, env: &Env, info: StrategyConfig) -> ContractResult {
        match self {
            StrategyConfig::Accumulate(s) => s.update(deps, env, info),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn can_execute(&self, deps: Deps, env: &Env, msg: Option<Binary>) -> StdResult<()> {
        match self {
            StrategyConfig::Accumulate(s) => s.can_execute(deps, env, msg),
            StrategyConfig::Custom(_) => Err(StdError::generic_err("New strategy not implemented")),
        }
    }

    fn execute(&mut self, deps: Deps, env: &Env, msg: Option<Binary>) -> ContractResult {
        match self {
            StrategyConfig::Accumulate(s) => s.execute(deps, env, msg),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn handle_reply(&mut self, deps: Deps, env: &Env, reply: Reply) -> ContractResult {
        match self {
            StrategyConfig::Accumulate(s) => s.handle_reply(deps, env, reply),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn deposit(&mut self, deps: Deps, env: &Env, info: &MessageInfo) -> ContractResult {
        match self {
            StrategyConfig::Accumulate(s) => s.deposit(deps, env, info),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn withdraw(&mut self, deps: Deps, env: &Env, amounts: Vec<Coin>) -> ContractResult {
        match self {
            StrategyConfig::Accumulate(s) => s.withdraw(deps, env, amounts),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn pause(&mut self, deps: Deps, env: &Env) -> ContractResult {
        match self {
            StrategyConfig::Accumulate(s) => s.pause(deps, env),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn resume(&mut self, deps: Deps, env: &Env) -> ContractResult {
        match self {
            StrategyConfig::Accumulate(s) => s.resume(deps, env),
            StrategyConfig::Custom(_) => {
                ContractResult::Err(ContractError::Generic("New strategy not implemented"))
            }
        }
    }

    fn statistics(&self) -> StrategyStatistics {
        match self {
            StrategyConfig::Accumulate(s) => s.statistics(),
            StrategyConfig::Custom(_) => StrategyStatistics::New(NewStatistics {
                amount: Coin {
                    denom: "uusd".to_string(),
                    amount: Uint128::zero(),
                },
            }),
        }
    }
}
