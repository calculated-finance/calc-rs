#[cfg(test)]
pub fn default_destination() -> calc_rs::distributor::Destination {
    calc_rs::distributor::Destination {
        shares: cosmwasm_std::Uint128::new(10000),
        recipient: calc_rs::distributor::Recipient::Bank {
            address: cosmwasm_std::testing::mock_dependencies()
                .api
                .addr_make("destination1"),
        },
        label: None,
    }
}

#[cfg(test)]
pub fn default_config() -> calc_rs::distributor::DistributorConfig {
    calc_rs::distributor::DistributorConfig {
        owner: cosmwasm_std::testing::mock_dependencies()
            .api
            .addr_make("owner"),
        denoms: vec!["rune".to_string()],
        mutable_destinations: vec![default_destination()],
        immutable_destinations: vec![default_destination()],
        conditions: vec![],
    }
}
