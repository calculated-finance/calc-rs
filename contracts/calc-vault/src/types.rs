use calc_rs::types::StrategyConfig;
use cosmwasm_std::{Deps, MessageInfo, StdError};
use cw_utils::{Duration, Expiration};
use rujira_rs::proto::common::{Asset, Coin};

trait Validate {
    fn validate() -> StdResult<()>;
}

impl Validate for StrategyConfig {
    fn validate(deps: Deps, info: MessageInfo) -> StdResult<()> {
        match self {
            StrategyConfig::Regular {
                owner,
                swap_amount,
                target_denom,
                schedule,
                minimum_receive_amount,
                route,
            } => {
                deps.api.addr_validate(owner)?;

                if (info.funds.len() > 1) {
                    return Err(StdError::verification_err(source));
                }

                Ok(())
            }
        }
    }
}
