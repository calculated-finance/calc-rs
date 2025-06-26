#[cfg(test)]
pub fn default_destination() -> calc_rs::types::Destination {
    use calc_rs::types::Recipient;
    use cosmwasm_std::Uint128;

    calc_rs::types::Destination {
        shares: Uint128::new(10000),
        recipient: Recipient::Bank {
            address: cosmwasm_std::testing::mock_dependencies()
                .api
                .addr_make("destination1"),
        },
        label: None,
    }
}

#[cfg(test)]
pub fn default_config() -> calc_rs::types::DistributorConfig {
    calc_rs::types::DistributorConfig {
        owner: cosmwasm_std::testing::mock_dependencies()
            .api
            .addr_make("owner"),
        denoms: vec!["rune".to_string()],
        mutable_destinations: vec![default_destination()],
        immutable_destinations: vec![default_destination()],
        conditions: vec![],
    }
}
