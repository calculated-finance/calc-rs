use anyhow::Result as AnyResult;
use cosmwasm_std::{
    testing::{MockApi, MockStorage},
    Addr, AnyMsg, Api, Binary, BlockInfo, CustomMsg, CustomQuery, Empty, GrpcQuery, Querier,
    Storage,
};
use cw_multi_test::{
    App, AppResponse, BankKeeper, CosmosRouter, FailingModule, GovFailingModule, IbcFailingModule,
    Stargate, WasmKeeper,
};
use rujira_rs::proto::types::{QueryQuoteSwapResponse, QuoteFees};
use serde::de::DeserializeOwned;

use prost::Message;

pub type RujiraApp = App<
    BankKeeper,
    MockApi,
    MockStorage,
    FailingModule<Empty, Empty, Empty>,
    WasmKeeper<Empty, Empty>,
    FailingModule<Empty, Empty, Empty>,
    FailingModule<Empty, Empty, Empty>,
    IbcFailingModule,
    GovFailingModule,
    RujiraStargate,
>;

#[derive(Default)]
pub struct RujiraStargate {}

impl Stargate for RujiraStargate {
    fn execute_stargate<ExecC, QueryC>(
        &self,
        _api: &dyn Api,
        _storage: &mut dyn Storage,
        _router: &dyn CosmosRouter<ExecC = ExecC, QueryC = QueryC>,
        _block: &BlockInfo,
        sender: Addr,
        type_url: String,
        value: Binary,
    ) -> AnyResult<AppResponse>
    where
        ExecC: CustomMsg + DeserializeOwned + 'static,
        QueryC: CustomQuery + DeserializeOwned + 'static,
    {
        anyhow::bail!(
            "Unexpected stargate execute: type_url={}, value={} from {}",
            type_url,
            value,
            sender,
        )
    }

    fn query_stargate(
        &self,
        _api: &dyn Api,
        _storage: &dyn Storage,
        _querier: &dyn Querier,
        _block: &BlockInfo,
        path: String,
        data: Binary,
    ) -> AnyResult<Binary> {
        anyhow::bail!("Unexpected stargate query: path={}, data={}", path, data)
    }

    fn execute_any<ExecC, QueryC>(
        &self,
        _api: &dyn Api,
        _storage: &mut dyn Storage,
        _router: &dyn CosmosRouter<ExecC = ExecC, QueryC = QueryC>,
        _block: &BlockInfo,
        sender: Addr,
        msg: AnyMsg,
    ) -> AnyResult<AppResponse>
    where
        ExecC: CustomMsg + DeserializeOwned + 'static,
        QueryC: CustomQuery + DeserializeOwned + 'static,
    {
        match msg.type_url.clone().as_str() {
            "/types.MsgDeposit" => Ok(AppResponse {
                events: vec![],
                data: None,
            }),

            _ => {
                anyhow::bail!("Unexpected any execute: msg={:?} from {}", msg, sender)
            }
        }
    }

    fn query_grpc(
        &self,
        _api: &dyn Api,
        _storage: &dyn Storage,
        _querier: &dyn Querier,
        _block: &BlockInfo,
        request: GrpcQuery,
    ) -> AnyResult<Binary> {
        match request.path.as_str() {
            "/types.Query/QuoteSwap" => mock_quote_response(),
            _ => {
                anyhow::bail!("Unexpected grpc query: request={:?}", request)
            }
        }
    }
}

fn mock_quote_response() -> AnyResult<Binary> {
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
