use calc_rs::{
    core::{Condition, DEPOSIT_FEE},
    distributor::{DistributorConfig, DistributorStatistics, Recipient},
};
use cosmwasm_std::{Coin, DepsMut, Env, StdError, StdResult, Storage, Uint128};
use cw_storage_plus::Item;

pub struct ConfigStore {
    item: Item<DistributorConfig>,
}

impl ConfigStore {
    pub fn save(
        &self,
        deps: &mut DepsMut,
        env: &Env,
        msg: &mut DistributorConfig,
    ) -> StdResult<()> {
        deps.api
            .addr_validate(&msg.owner.to_string())
            .map_err(|_| StdError::generic_err(format!("Invalid owner address: {}", msg.owner)))?;

        let destinations = msg
            .mutable_destinations
            .iter()
            .chain(msg.immutable_destinations.iter())
            .collect::<Vec<_>>();

        if destinations.is_empty() {
            return Err(StdError::generic_err(
                "Must provide at least one destination",
            ));
        }

        if destinations.len() > 20 {
            return Err(StdError::generic_err(
                "Cannot provide more than 20 total destinations",
            ));
        }

        let has_native_denoms = msg.denoms.iter().any(|d| !d.contains("-"));
        let mut total_shares = Uint128::zero();
        let mut required_rune_balance = 0_u128;

        for destination in destinations.clone() {
            if destination.shares.is_zero() {
                return Err(StdError::generic_err(
                    "Shares for each destination must be greater than zero",
                ));
            }

            match destination.recipient.clone() {
                Recipient::Bank { address, .. } | Recipient::Wasm { address, .. } => {
                    deps.api.addr_validate(&address.to_string()).map_err(|_| {
                        StdError::generic_err(format!("Invalid destination address: {}", address))
                    })?;
                }
                Recipient::Deposit { memo } => {
                    if has_native_denoms {
                        return Err(StdError::generic_err(format!(
                            "Only secured assets can be deposited with memo {}",
                            memo
                        )));
                    }

                    required_rune_balance += DEPOSIT_FEE;
                }
            }

            total_shares += destination.shares;
        }

        if total_shares < Uint128::new(10_000) {
            return Err(StdError::generic_err(
                "Total shares must be at least 10,000",
            ));
        }

        // The contract needs to have enough RUNE to cover the deposit fee(s)
        if required_rune_balance > 0 {
            msg.conditions.push(Condition::BalanceAvailable {
                address: env.contract.address.clone(),
                amount: Coin::new(required_rune_balance, "rune"),
            });
        }

        self.item.save(deps.storage, &msg)
    }

    pub fn load(&self, store: &dyn Storage) -> StdResult<DistributorConfig> {
        self.item.load(store)
    }
}

pub const CONFIG: ConfigStore = ConfigStore {
    item: Item::new("config"),
};

pub const STATS: Item<DistributorStatistics> = Item::new("statistics");

#[cfg(test)]
mod save_config_tests {
    use crate::test::{default_config, default_destination};

    use super::*;
    use calc_rs::distributor::Destination;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Addr,
    };
    use rstest::rstest;

    #[rstest]
    #[case(
        DistributorConfig {
            owner: Addr::unchecked("owner"),
            ..default_config()
        },
        "Generic error: Invalid owner address: owner"
    )]
    #[case(
        DistributorConfig {
            mutable_destinations: vec![],
            immutable_destinations: vec![],
            ..default_config()
        },
        "Generic error: Must provide at least one destination"
    )]
    #[case(
        DistributorConfig {
            mutable_destinations: (0..30).map(|_| default_destination()).collect(),
            ..default_config()
        },
        "Generic error: Cannot provide more than 20 total destinations"
    )]
    #[case(
        DistributorConfig {
            mutable_destinations: vec![
                Destination {
                    shares: Uint128::zero(),
                    ..default_destination()
                },
                Destination {
                    shares: Uint128::new(10_000),
                    ..default_destination()
                }
            ],
            immutable_destinations: vec![],
            ..default_config()
        },
        "Generic error: Shares for each destination must be greater than zero"
    )]
    #[case(
        DistributorConfig {
            mutable_destinations: vec![Destination {
                shares: Uint128::new(10_000),
                recipient: Recipient::Bank {
                    address: Addr::unchecked("invalid_address"),
                },
                ..default_destination()
            }],
            immutable_destinations: vec![],
            ..default_config()
        },
        "Generic error: Invalid destination address: invalid_address"
    )]
    #[case(
        DistributorConfig {
            mutable_destinations: vec![Destination {
                shares: Uint128::new(10_000),
                recipient: Recipient::Deposit {
                    memo: "-secure:evm-address".to_string(),
                },
                ..default_destination()
            }],
            immutable_destinations: vec![],
            denoms: vec!["rune".to_string(), "eth-eth".to_string()],
            ..default_config()
        },
        "Generic error: Only secured assets can be deposited with memo -secure:evm-address"
    )]
    #[case(
        DistributorConfig {
            mutable_destinations: vec![Destination {
                shares: Uint128::new(5000),
                ..default_destination()
            }],
            immutable_destinations: vec![],
            ..default_config()
        },
        "Generic error: Total shares must be at least 10,000"
    )]
    fn invalid_config_fails(#[case] mut msg: DistributorConfig, #[case] expected_error: &str) {
        let mut deps = mock_dependencies();

        assert_eq!(
            CONFIG
                .save(&mut deps.as_mut(), &mock_env(), &mut msg)
                .unwrap_err()
                .to_string(),
            expected_error
        );
    }

    #[rstest]
    fn valid_config_succeeds() {
        let mut deps = mock_dependencies();
        let mut msg = default_config();

        assert!(CONFIG
            .save(&mut deps.as_mut(), &mock_env(), &mut msg)
            .is_ok());
        assert_eq!(CONFIG.load(deps.as_mut().storage).unwrap(), msg);
    }

    #[rstest]
    fn appends_rune_balance_condition_for_deposit_recipients() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        let mut msg = DistributorConfig {
            denoms: vec!["eth-eth".to_string()],
            mutable_destinations: vec![
                Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Deposit {
                        memo: "-secure:contract1".to_string(),
                    },
                    label: None,
                },
                Destination {
                    shares: Uint128::new(10_000),
                    recipient: Recipient::Deposit {
                        memo: "-secure:contract2".to_string(),
                    },
                    label: None,
                },
            ],
            ..default_config()
        };

        CONFIG.save(&mut deps.as_mut(), &env, &mut msg).unwrap();

        let config = CONFIG.load(&deps.storage).unwrap();

        assert_eq!(
            config.conditions,
            vec![Condition::BalanceAvailable {
                address: env.contract.address,
                amount: Coin::new(DEPOSIT_FEE * 2, "rune"),
            }]
        );
    }
}
