use anyhow::Result as AnyResult;
use cosmwasm_std::{
    testing::{MockApi, MockStorage},
    Addr, AnyMsg, Api, BankMsg, Binary, BlockInfo, Coin, CosmosMsg, CustomMsg, CustomQuery, Empty,
    GrpcQuery, Querier, Storage, WasmMsg,
};
use cw_multi_test::{
    App, AppResponse, BankKeeper, CosmosRouter, FailingModule, GovFailingModule, IbcFailingModule,
    Stargate, WasmKeeper,
};
use prost::Message;
use serde::de::DeserializeOwned;

use crate::fixtures::{mock_pool, mock_quote_response};

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

#[derive(Clone, PartialEq, Message)]
struct ProtoAny {
    #[prost(string, tag = "1")]
    type_url: String,
    #[prost(bytes = "vec", tag = "2")]
    value: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
struct ProtoCoin {
    #[prost(string, tag = "1")]
    denom: String,
    #[prost(string, tag = "2")]
    amount: String,
}

#[derive(Clone, PartialEq, Message)]
struct MsgExec {
    #[prost(string, tag = "1")]
    grantee: String,
    #[prost(message, repeated, tag = "2")]
    msgs: Vec<ProtoAny>,
}

#[derive(Clone, PartialEq, Message)]
struct MsgSend {
    #[prost(string, tag = "1")]
    from_address: String,
    #[prost(string, tag = "2")]
    to_address: String,
    #[prost(message, repeated, tag = "3")]
    amount: Vec<ProtoCoin>,
}

#[derive(Clone, PartialEq, Message)]
struct MsgExecuteContract {
    #[prost(string, tag = "1")]
    sender: String,
    #[prost(string, tag = "2")]
    contract: String,
    #[prost(bytes = "vec", tag = "3")]
    msg: Vec<u8>,
    #[prost(message, repeated, tag = "4")]
    funds: Vec<ProtoCoin>,
}

fn decode_coins(coins: Vec<ProtoCoin>) -> AnyResult<Vec<Coin>> {
    coins
        .into_iter()
        .map(|coin| Ok(Coin::new(coin.amount.parse::<u128>()?, coin.denom)))
        .collect()
}

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
        api: &dyn Api,
        storage: &mut dyn Storage,
        router: &dyn CosmosRouter<ExecC = ExecC, QueryC = QueryC>,
        block: &BlockInfo,
        sender: Addr,
        msg: AnyMsg,
    ) -> AnyResult<AppResponse>
    where
        ExecC: CustomMsg + DeserializeOwned + 'static,
        QueryC: CustomQuery + DeserializeOwned + 'static,
    {
        match msg.type_url.clone().as_str() {
            "/cosmos.authz.v1beta1.MsgExec" => {
                let exec = MsgExec::decode(msg.value.as_slice())?;
                if exec.grantee != sender.as_str() {
                    anyhow::bail!(
                        "Authz grantee mismatch: expected {}, got {}",
                        sender,
                        exec.grantee
                    );
                }

                let mut response = AppResponse::default();

                for inner in exec.msgs {
                    let inner_response = match inner.type_url.as_str() {
                        "/cosmos.bank.v1beta1.MsgSend" => {
                            let send = MsgSend::decode(inner.value.as_slice())?;
                            let sender = api.addr_validate(&send.from_address)?;
                            router.execute(
                                api,
                                storage,
                                block,
                                sender,
                                CosmosMsg::Bank(BankMsg::Send {
                                    to_address: send.to_address,
                                    amount: decode_coins(send.amount)?,
                                }),
                            )?
                        }
                        "/cosmwasm.wasm.v1.MsgExecuteContract" => {
                            let execute = MsgExecuteContract::decode(inner.value.as_slice())?;
                            let sender = api.addr_validate(&execute.sender)?;
                            router.execute(
                                api,
                                storage,
                                block,
                                sender,
                                CosmosMsg::Wasm(WasmMsg::Execute {
                                    contract_addr: execute.contract,
                                    msg: execute.msg.into(),
                                    funds: decode_coins(execute.funds)?,
                                }),
                            )?
                        }
                        _ => anyhow::bail!("Unexpected authz inner message: {inner:?}"),
                    };

                    response.events.extend(inner_response.events);
                    response.msg_responses.extend(inner_response.msg_responses);
                    if inner_response.data.is_some() {
                        response.data = inner_response.data;
                    }
                }

                Ok(response)
            }
            "/types.MsgDeposit" => Ok(AppResponse {
                events: vec![],
                data: None,
                msg_responses: vec![],
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
            "/types.Query/Pool" => mock_pool(request.data),
            _ => {
                anyhow::bail!("Unexpected grpc query: request={:?}", request)
            }
        }
    }
}
