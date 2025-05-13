use calc_rs::types::{ContractResult, StrategyConfig};
use cosmwasm_std::{
    Attribute, Deps, DepsMut, Env, Event, MessageInfo, Response, StdError, StdResult,
};

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
            StrategyConfig::New {} => Ok(()),
        }
    }
}

pub trait Executable {
    fn can_execute(&self, deps: Deps, info: &MessageInfo) -> bool;
    fn execute(&self, deps: DepsMut, env: Env, info: MessageInfo) -> ContractResult;
}

impl Executable for StrategyConfig {
    fn can_execute(&self, _deps: Deps, _info: &MessageInfo) -> bool {
        match self {
            StrategyConfig::Dca { .. } => true,
            StrategyConfig::New {} => true,
        }
    }

    fn execute(&self, _deps: DepsMut, _env: Env, _info: MessageInfo) -> ContractResult {
        let mut event_data: Vec<Attribute> = vec![];

        match self {
            StrategyConfig::Dca { .. } => {
                event_data.push(Attribute::new("type", "dca"));
            }
            StrategyConfig::New {} => event_data.push(Attribute::new("type", "new")),
        };

        Ok(Response::new().add_event(Event::new("execute_strategy").add_attributes(event_data)))
    }
}
