use std::{
    num::{ParseIntError, TryFromIntError},
    str::FromStr,
};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Binary, QuerierWrapper, StdError, Uint128};
use prost::{DecodeError, EncodeError, Message};
use rujira_rs::proto::types::{
    QueryQuoteSwapRequest, QueryQuoteSwapResponse, QuoteFees as QuerySwapQuoteResponseFees,
};
use thiserror::Error;

pub trait QueryablePair {
    type Request: Message + Default;
    type Response: Message + Sized + Default;

    fn grpc_path() -> &'static str;
}

pub trait Queryable: Sized {
    type Pair: QueryablePair;

    fn get(
        querier: QuerierWrapper,
        req: <Self::Pair as QueryablePair>::Request,
    ) -> Result<Self, QueryError>;
}

impl<T> Queryable for T
where
    T: QueryablePair<Response = Self> + Message + Default,
{
    type Pair = T;

    fn get(
        querier: QuerierWrapper,
        req: <Self::Pair as QueryablePair>::Request,
    ) -> Result<Self, QueryError> {
        let mut buf = Vec::new();
        req.encode(&mut buf)?;
        let res = querier
            .query_grpc(Self::grpc_path().to_string(), Binary::from(buf))?
            .to_vec();
        Ok(Self::decode(&*res)?)
    }
}

#[derive(Error, Debug)]
pub enum QueryError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Encode(#[from] EncodeError),

    #[error("{0}")]
    Decode(#[from] DecodeError),
}

impl QueryablePair for QueryQuoteSwapResponse {
    type Request = QueryQuoteSwapRequest;
    type Response = QueryQuoteSwapResponse;

    fn grpc_path() -> &'static str {
        "/types.Query/QuoteSwap"
    }
}

impl SwapQuote {
    pub fn get(q: QuerierWrapper, request: &SwapQuoteRequest) -> Result<Self, SwapQuoteError> {
        let res = QueryQuoteSwapResponse::get(q, QueryQuoteSwapRequest::from(request.clone()))?;
        Ok(Self::try_from(res)?)
    }
}

#[derive(Error, Debug)]
pub enum SwapQuoteError {
    #[error("{0}")]
    TryFromQuoteSwapResponse(#[from] TryFromQuoteSwapResponseError),
    #[error("{0}")]
    Query(#[from] QueryError),
}

impl From<SwapQuoteRequest> for QueryQuoteSwapRequest {
    fn from(value: SwapQuoteRequest) -> Self {
        Self {
            from_asset: value.from_asset.to_string(),
            to_asset: value.to_asset.to_string(),
            amount: value.amount.to_string(),
            streaming_interval: value.streaming_interval.to_string(),
            streaming_quantity: value.streaming_quantity.to_string(),
            destination: value.destination.to_string(),
            refund_address: value.refund_address.to_string(),
            affiliate: value.affiliate,
            affiliate_bps: value.affiliate_bps.iter().map(|x| x.to_string()).collect(),
            height: "".to_string(),
            tolerance_bps: "".to_string(),
            liquidity_tolerance_bps: "".to_string(),
        }
    }
}

#[cw_serde]
pub struct SwapQuoteRequest {
    pub from_asset: String,
    pub to_asset: String,
    pub amount: Uint128,
    pub streaming_interval: Uint128,
    pub streaming_quantity: Uint128,
    pub destination: String,
    pub refund_address: String,
    pub affiliate: Vec<String>,
    pub affiliate_bps: Vec<u64>,
}

#[cw_serde]
pub struct QuoteFees {
    pub asset: String,
    pub affiliate: String,
    pub outbound: Uint128,
    pub liquidity: Uint128,
    pub total: Uint128,
    pub slippage_bps: u64,
    pub total_bps: u64,
}

#[cw_serde]
pub struct SwapQuote {
    pub fees: Option<QuoteFees>,
    pub expiry: u64,
    pub warning: String,
    pub notes: String,
    pub dust_threshold: String,
    pub recommended_min_amount_in: String,
    pub gas_rate_units: String,
    pub memo: String,
    pub expected_amount_out: Uint128,
}

impl TryFrom<QuerySwapQuoteResponseFees> for QuoteFees {
    type Error = TryFromQuoteSwapResponseError;

    fn try_from(value: QuerySwapQuoteResponseFees) -> Result<Self, Self::Error> {
        Ok(Self {
            asset: value.asset,
            affiliate: value.affiliate,
            outbound: Uint128::from_str(value.outbound.as_str())?,
            liquidity: Uint128::from_str(value.liquidity.as_str())?,
            total: Uint128::from_str(value.total.as_str())?,
            slippage_bps: u64::try_from(value.slippage_bps)?,
            total_bps: u64::try_from(value.total_bps)?,
        })
    }
}

impl TryFrom<QueryQuoteSwapResponse> for SwapQuote {
    type Error = TryFromQuoteSwapResponseError;

    fn try_from(value: QueryQuoteSwapResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            fees: value.fees.map(QuoteFees::try_from).transpose()?,
            expiry: u64::try_from(value.expiry)?,
            warning: value.warning,
            notes: value.notes,
            dust_threshold: value.dust_threshold,
            recommended_min_amount_in: value.recommended_min_amount_in,
            gas_rate_units: value.gas_rate_units,
            memo: value.memo,
            expected_amount_out: Uint128::from_str(value.expected_amount_out.as_str())?,
        })
    }
}

#[derive(Error, Debug)]
pub enum AssetError {
    #[error("Invalid layer 1 string {0}")]
    Invalid(String),

    #[error("Invalid native denom string {0}")]
    InvalidNativeDenom(String),
}

#[derive(Error, Debug)]
pub enum TryFromQuoteSwapResponseError {
    #[error("{0}")]
    Std(#[from] StdError),
    #[error("{0}")]
    TryFromInt(#[from] TryFromIntError),
    #[error("{0}")]
    ParseInt(#[from] ParseIntError),
    #[error("{0}")]
    Asset(#[from] AssetError),
}

// #[cw_serde]
// pub struct QuoteFees {
//     pub asset: String,
//     pub affiliate: String,
//     pub outbound: Uint128,
//     pub liquidity: Uint128,
//     pub total: Uint128,
//     pub slippage_bps: u64,
//     pub total_bps: u64,
// }
