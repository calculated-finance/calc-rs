use calc_rs::types::StrategyConfig;
use cosmwasm_std::{DepsMut, Env, MessageInfo, Response, StdResult};

pub trait Executable {
    fn can_execute(&self, deps: DepsMut, info: &MessageInfo) -> StdResult<bool>;
    fn execute(&self, deps: DepsMut, env: Env, info: MessageInfo) -> StdResult<Response>;
}

impl Executable for StrategyConfig {
    fn can_execute(&self, _deps: DepsMut, _info: &MessageInfo) -> StdResult<bool> {
        match self {
            StrategyConfig::DCA { .. } => Ok(true),
            StrategyConfig::New {} => Ok(true),
        }
    }

    fn execute(&self, _deps: DepsMut, _env: Env, _info: MessageInfo) -> StdResult<Response> {
        Ok(match self {
            StrategyConfig::DCA { .. } => Response::new()
                .add_attribute("strategy", "simple_dca")
                .add_attribute("action", "execute"),
            StrategyConfig::New {} => Response::new()
                .add_attribute("strategy", "new")
                .add_attribute("action", "execute"),
        })
    }
}
