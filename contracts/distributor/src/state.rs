use calc_rs::{
    core::Condition,
    distributor::{DistributorConfig, DistributorStatistics, Recipient},
    thorchain::Network,
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
        let mut required_rune_balance = Uint128::zero();

        let mut network: Option<Network> = None;

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

                    if let Some(network) = &network {
                        required_rune_balance += network.native_tx_fee_rune;
                    } else {
                        network = Some(Network::load(deps.querier).map_err(|e| {
                            StdError::generic_err(format!("Failed to load network: {}", e))
                        })?);

                        required_rune_balance += network
                            .as_ref()
                            .expect("Failed to load native tx fee")
                            .native_tx_fee_rune;
                    }
                }
            }

            total_shares += destination.shares;
        }

        if total_shares < Uint128::new(10_000) {
            return Err(StdError::generic_err(
                "Total shares must be at least 10,000",
            ));
        }

        let existing_config = self.load(deps.storage).unwrap_or(msg.clone());

        let total_existing_shares = existing_config
            .mutable_destinations
            .iter()
            .chain(existing_config.immutable_destinations.iter())
            .fold(Uint128::zero(), |acc, d| acc + d.shares);

        if total_shares.ne(&total_existing_shares) {
            // This is to prevent fee %'s from being diluted
            return Err(StdError::generic_err("Total share count must not change"));
        }

        // The contract needs to have enough RUNE to cover the deposit fee(s)
        if required_rune_balance.gt(&Uint128::zero()) {
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
    use super::*;

    use crate::test::{default_config, default_destination};

    use calc_rs::distributor::Destination;
    use calc_rs_test::test::mock_dependencies_with_custom_grpc_querier;
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        Addr, ContractResult, SystemResult,
    };
    use prost::Message;
    use rstest::rstest;
    use rujira_rs::proto::types::QueryNetworkResponse;

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

    #[test]
    fn changing_total_share_count_fails() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        CONFIG
            .save(&mut deps.as_mut(), &env, &mut default_config())
            .unwrap();

        let recipient = deps.api.addr_make("dest");

        assert_eq!(
            CONFIG
                .save(
                    &mut deps.as_mut(),
                    &env,
                    &mut DistributorConfig {
                        mutable_destinations: vec![Destination {
                            recipient: Recipient::Bank { address: recipient },
                            shares: Uint128::new(2736478323223432),
                            label: None
                        }],
                        ..default_config()
                    }
                )
                .unwrap_err(),
            StdError::generic_err("Total share count must not change")
        )
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
        let mut deps = mock_dependencies_with_custom_grpc_querier();
        let env = mock_env();

        let deposit_fee = 2_000_000u128;

        deps.querier.with_grpc_handler(move |_| {
            let response = QueryNetworkResponse {
                bond_reward_rune: "4726527489".to_string(),
                total_bond_units: "277404".to_string(),
                effective_security_bond: "90126604378071".to_string(),
                total_reserve: "4994080222948541".to_string(),
                vaults_migrating: true,
                gas_spent_rune: "0".to_string(),
                gas_withheld_rune: "0".to_string(),
                outbound_fee_multiplier: "30000".to_string(),
                native_outbound_fee_rune: "2000000".to_string(),
                native_tx_fee_rune: deposit_fee.to_string(),
                tns_register_fee_rune: "1000000000".to_string(),
                tns_fee_per_block_rune: "20".to_string(),
                rune_price_in_tor: "1.14130903".to_string(),
                tor_price_in_rune: "0.87618688".to_string(),
            };

            let mut buf = Vec::new();
            response.encode(&mut buf).unwrap();

            SystemResult::Ok(ContractResult::Ok(buf.into()))
        });

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
                amount: Coin::new(deposit_fee * 2, "rune"),
            }]
        );
    }
}
