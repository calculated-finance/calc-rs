use anyhow::Error;
use anyhow::Result as AnyResult;
use cosmwasm_std::{Binary, StdError};
use prost::Message;
use rujira_rs::proto;
use rujira_rs::proto::types::{QueryQuoteSwapResponse, QuoteFees};

fn mock_pool_btc() -> Binary {
    let pool = proto::types::QueryPoolResponse {
        asset: "BTC.BTC".to_string(),
        short_code: "b".to_string(),
        status: "Available".to_string(),
        decimals: 8,
        pending_inbound_asset: "156524579".to_string(),
        pending_inbound_rune: "0".to_string(),
        balance_asset: "68602648901".to_string(),
        balance_rune: "1172427071332399".to_string(),
        asset_tor_price: "10010000000000".to_string(),
        pool_units: "613518358320559".to_string(),
        lp_units: "347866097255926".to_string(),
        synth_units: "265652261064633".to_string(),
        synth_supply: "59409628248".to_string(),
        savers_depth: "58882558588".to_string(),
        savers_units: "56192173382".to_string(),
        savers_fill_bps: "8660".to_string(),
        savers_capacity_remaining: "9193020653".to_string(),
        synth_mint_paused: false,
        synth_supply_remaining: "22913550433".to_string(),
        loan_collateral: "167294477784".to_string(),
        loan_collateral_remaining: "0".to_string(),
        loan_cr: "0".to_string(),
        derived_depth_bps: "9639".to_string(),
    };

    let mut buf = Vec::new();
    pool.encode(&mut buf).unwrap();
    buf.into()
}

fn mock_pool_usdc() -> Binary {
    let pool = proto::types::QueryPoolResponse {
        asset: "ETH.USDC-0XA0B86991C6218B36C1D19D4A2E9EB0CE3606EB48".to_string(),
        status: "Available".to_string(),
        decimals: 6,
        pending_inbound_asset: "0".to_string(),
        pending_inbound_rune: "0".to_string(),
        balance_asset: "1068860344382528".to_string(),
        balance_rune: "217689972512615".to_string(),
        asset_tor_price: "100100000".to_string(),
        pool_units: "51619557902356".to_string(),
        lp_units: "33369405984602".to_string(),
        synth_units: "18250151917754".to_string(),
        synth_supply: "755793519221676".to_string(),
        savers_depth: "727032247104330".to_string(),
        savers_units: "646314302227834".to_string(),
        savers_fill_bps: "7071".to_string(),
        savers_capacity_remaining: "313066825160852".to_string(),
        synth_mint_paused: false,
        synth_supply_remaining: "526838894037357".to_string(),
        loan_collateral: "0".to_string(),
        loan_collateral_remaining: "0".to_string(),
        loan_cr: "0".to_string(),
        derived_depth_bps: "0".to_string(),
        short_code: "".to_string(),
    };

    let mut buf = Vec::new();
    pool.encode(&mut buf).unwrap();
    buf.into()
}

pub fn mock_pool(request: Binary) -> Result<Binary, Error> {
    let req = proto::types::QueryPoolRequest::decode(request.as_slice()).unwrap();

    match req.asset.as_str() {
        "BTC.BTC" => Ok(mock_pool_btc()),
        "ETH.USDC-0XA0B86991C6218B36C1D19D4A2E9EB0CE3606EB48" => Ok(mock_pool_usdc()),
        _ => Err(StdError::generic_err("Asset not found").into()),
    }
}

pub fn mock_quote_response() -> AnyResult<Binary> {
    let quote = QueryQuoteSwapResponse {
        inbound_address: "sthor17pfp4qvy5vrmtjar7kntachm0cfm9m9azl3jka".to_string(),
        inbound_confirmation_blocks: 0,
        inbound_confirmation_seconds: 0,
        outbound_delay_blocks: 0,
        outbound_delay_seconds: 0,
        fees: Some(QuoteFees {
            asset: 100.to_string(),
            affiliate: 100.to_string(),
            outbound: 100.to_string(),
            liquidity: 100.to_string(),
            total: 100.to_string(),
            slippage_bps: 100,
            total_bps: 100,
        }),
        router: "0xd31cA16eDF87822278C50716900e264fE2de0200".to_string(),
        expiry: 1,
        warning: "No warning".to_string(),
        notes: "No notes".to_string(),
        dust_threshold: 1.to_string(),
        recommended_min_amount_in: 1.to_string(),
        recommended_gas_rate: 1.to_string(),
        gas_rate_units: "units".to_string(),
        memo: "=:thor.rune:sthor17pfp4qvy5vrmtjar7kntachm0cfm9m9azl3jka:1/5/5:rj:10".to_string(),
        expected_amount_out: "1000".to_string(),
        max_streaming_quantity: 10,
        streaming_swap_blocks: 10,
        streaming_swap_seconds: 10,
        total_swap_seconds: 100,
    };
    let mut buf = Vec::new();
    quote.encode(&mut buf).unwrap();
    Ok(Binary::from(buf))
}
