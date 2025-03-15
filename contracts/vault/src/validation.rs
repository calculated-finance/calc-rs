use calc_rs::types::{ContractError, StrategyConfig};
use cosmwasm_std::{Deps, MessageInfo};

pub trait Validate {
    fn validate(&self, deps: Deps, info: MessageInfo) -> Result<(), ContractError>;
}

impl Validate for StrategyConfig {
    fn validate(&self, deps: Deps, info: MessageInfo) -> Result<(), ContractError> {
        match self {
            StrategyConfig::Regular { owner, .. } => {
                deps.api
                    .addr_validate(&owner.clone().into_string())
                    .map_err(|_err| return ContractError::Unauthorized {})?;

                if info.funds.len() > 1 {
                    return Err(ContractError::Unauthorized {});
                }

                Ok(())
            }
        }
    }
}
