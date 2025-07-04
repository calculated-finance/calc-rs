use cosmwasm_std::{Deps, DepsMut, StdError, StdResult};
use cw_storage_plus::Item;

use crate::types::ExchangeConfig;

pub struct ConfigStore {
    pub config: Item<ExchangeConfig>,
}

impl ConfigStore {
    pub fn save(&self, deps: DepsMut, mut config: ExchangeConfig) -> StdResult<()> {
        deps.api.addr_validate(config.scheduler_address.as_str())?;

        if let Some(affiliate_code) = &config.affiliate_code {
            if affiliate_code.len() > 5 {
                return Err(StdError::generic_err(
                    "Affiliate code must be 5 characters or less",
                ));
            }

            if let Some(affiliate_bps) = config.affiliate_bps {
                if affiliate_bps > 10 {
                    return Err(StdError::generic_err("Affiliate bps must be 10 or less"));
                }

                config.affiliate_code = Some(affiliate_code.clone());
                config.affiliate_bps = Some(affiliate_bps);
            } else {
                return Err(StdError::generic_err(
                    "Affiliate code provided but affiliate_bps is not set",
                ));
            }
        }

        self.config.save(deps.storage, &config)
    }

    pub fn load(&self, deps: Deps) -> StdResult<ExchangeConfig> {
        self.config.load(deps.storage)
    }
}

pub const CONFIG: ConfigStore = ConfigStore {
    config: Item::new("config"),
};

#[cfg(test)]
mod config_tests {
    use super::*;
    use cosmwasm_std::{testing::mock_dependencies, Addr};

    #[test]
    fn fails_with_invalid_scheduler_address() {
        let mut deps = mock_dependencies();

        let result = CONFIG.save(
            deps.as_mut(),
            ExchangeConfig {
                scheduler_address: Addr::unchecked("invalid_address"),
                affiliate_code: None,
                affiliate_bps: None,
            },
        );

        assert!(result.is_err());
    }

    #[test]
    fn fails_with_long_affiliate_code() {
        let mut deps = mock_dependencies();

        let scheduler_address = deps.api.addr_make("scheduler");

        let result = CONFIG
            .save(
                deps.as_mut(),
                ExchangeConfig {
                    scheduler_address,
                    affiliate_code: Some("longcode".to_string()),
                    affiliate_bps: None,
                },
            )
            .unwrap_err();

        assert_eq!(
            result,
            StdError::generic_err("Affiliate code must be 5 characters or less",)
        );
    }

    #[test]
    fn fails_with_high_affiliate_bps() {
        let mut deps = mock_dependencies();

        let scheduler_address = deps.api.addr_make("scheduler");
        let result = CONFIG
            .save(
                deps.as_mut(),
                ExchangeConfig {
                    scheduler_address,
                    affiliate_code: Some("code".to_string()),
                    affiliate_bps: Some(11),
                },
            )
            .unwrap_err();

        assert_eq!(
            result,
            StdError::generic_err("Affiliate bps must be 10 or less")
        );
    }

    #[test]
    fn saves_valid_config() {
        let mut deps = mock_dependencies();

        let scheduler_address = deps.api.addr_make("scheduler");
        CONFIG
            .save(
                deps.as_mut(),
                ExchangeConfig {
                    scheduler_address,
                    affiliate_code: Some("code".to_string()),
                    affiliate_bps: Some(5),
                },
            )
            .unwrap();

        assert_eq!((), ());
    }
}
