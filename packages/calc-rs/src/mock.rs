use cosmwasm_schema::serde::de::DeserializeOwned;
use cosmwasm_std::{
    testing::{MockApi, MockQuerier, MockQuerierCustomHandlerResult, MockStorage},
    Empty, GrpcQuery, OwnedDeps, Querier, QuerierResult, QueryRequest, WasmQuery,
};
use std::sync::Arc;

pub type GrpcHandler = Arc<dyn Fn(&GrpcQuery) -> QuerierResult + Send + Sync>;

pub struct CustomMockQuerier<C: DeserializeOwned = Empty> {
    default: MockQuerier<C>,
    grpc_handler: Option<GrpcHandler>,
}

impl<C: DeserializeOwned> CustomMockQuerier<C> {
    pub fn new() -> Self {
        Self {
            default: MockQuerier::new(&[]),
            grpc_handler: None,
        }
    }

    pub fn with_grpc_handler<GH>(&mut self, handler: GH)
    where
        GH: Fn(&GrpcQuery) -> QuerierResult + Send + Sync + 'static,
    {
        self.grpc_handler = Some(Arc::new(handler));
    }

    pub fn update_wasm<WH>(&mut self, handler: WH)
    where
        WH: Fn(&WasmQuery) -> QuerierResult + 'static,
    {
        self.default.update_wasm(handler)
    }

    pub fn with_custom_handler<CH>(self, handler: CH)
    where
        CH: Fn(&C) -> MockQuerierCustomHandlerResult + 'static,
    {
        self.default.with_custom_handler(handler);
    }
}

impl Querier for CustomMockQuerier {
    fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
        let parsed: Result<QueryRequest<Empty>, _> = cosmwasm_std::from_json(bin_request);
        if let Ok(request) = &parsed {
            if let QueryRequest::Grpc(query) = request {
                if let Some(handler) = &self.grpc_handler {
                    return handler(query);
                }
            }
        }

        self.default.raw_query(bin_request)
    }
}

impl Default for CustomMockQuerier {
    fn default() -> Self {
        Self::new()
    }
}

pub fn mock_dependencies_with_custom_querier(
) -> OwnedDeps<MockStorage, MockApi, CustomMockQuerier, Empty> {
    OwnedDeps {
        storage: MockStorage::default(),
        api: MockApi::default(),
        querier: CustomMockQuerier::default(),
        custom_query_type: std::marker::PhantomData,
    }
}
