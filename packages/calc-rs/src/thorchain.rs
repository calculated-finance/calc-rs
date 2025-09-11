use std::{
    num::{ParseIntError, TryFromIntError},
    ops::Div,
    str::FromStr,
};

use anybuf::Anybuf;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    AnyMsg, Binary, CanonicalAddr, Coin, CosmosMsg, Decimal, QuerierWrapper, StdError, StdResult,
    Uint128,
};
use prost::{DecodeError, EncodeError, Message};
use rujira_rs::proto::types::{
    QueryNetworkRequest, QueryNetworkResponse, QueryQuoteSwapRequest, QueryQuoteSwapResponse,
    QuoteFees as QuerySwapQuoteResponseFees,
};
use thiserror::Error;

#[cw_serde]
pub struct MsgDeposit {
    pub memo: String,
    pub coins: Vec<Coin>,
    pub signer: CanonicalAddr,
}

pub fn denom_to_asset_str(denom: &str) -> String {
    match denom.split_once("-") {
        Some(_) => denom.to_string(),
        None => match denom.split_once("/") {
            Some((_, symbol)) => format!("THOR.{}", symbol.to_ascii_uppercase()),
            None => format!("THOR.{}", denom.to_ascii_uppercase()),
        },
    }
}

fn secured_denom_to_buf(chain: &str, symbol: &str) -> Anybuf {
    Anybuf::new()
        .append_string(1, chain) // chain
        .append_string(2, symbol) // symbol
        .append_string(3, symbol.split("-").next().unwrap_or(symbol)) // ticker
        .append_bool(4, false) // synth
        .append_bool(5, false) // trade
        .append_bool(6, true) // secured
}

fn native_denom_to_buf(symbol: &str) -> Anybuf {
    Anybuf::new()
        .append_string(1, "THOR") // chain
        .append_string(2, symbol) // symbol
        .append_string(3, symbol.split("-").next().unwrap_or(symbol)) // ticker
}

pub fn denom_to_buf(denom: &str) -> Anybuf {
    match denom.split_once("-") {
        Some((chain, symbol)) => secured_denom_to_buf(chain, symbol),
        None => match denom {
            "x/ruji" => native_denom_to_buf("RUJI"),
            _ => native_denom_to_buf(&denom.to_uppercase()),
        },
    }
}

impl MsgDeposit {
    pub fn into_cosmos_msg(self) -> StdResult<CosmosMsg> {
        let mut coins = Vec::with_capacity(self.coins.len());

        for coin in self.coins {
            coins.push(
                Anybuf::new()
                    .append_message(1, &denom_to_buf(&coin.denom))
                    .append_string(2, coin.amount.to_string()),
            );
        }

        let value = Anybuf::new()
            .append_repeated_message(1, &coins)
            .append_string(2, self.memo)
            .append_bytes(3, self.signer.to_vec());

        Ok(CosmosMsg::Any(AnyMsg {
            type_url: "/types.MsgDeposit".to_string(),
            value: value.as_bytes().into(),
        }))
    }
}

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
    pub recommended_min_amount_in: Uint128,
    pub gas_rate_units: String,
    pub memo: String,
    pub expected_amount_out: Uint128,
    pub max_streaming_quantity: u64,
    pub streaming_swap_blocks: u64,
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
            recommended_min_amount_in: Uint128::from_str(value.recommended_min_amount_in.as_str())?,
            gas_rate_units: value.gas_rate_units,
            memo: value.memo,
            expected_amount_out: Uint128::from_str(value.expected_amount_out.as_str())?,
            max_streaming_quantity: u64::try_from(value.streaming_swap_blocks).unwrap_or(1),
            streaming_swap_blocks: u64::try_from(value.streaming_swap_blocks).unwrap_or(1),
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

impl QueryablePair for QueryNetworkResponse {
    type Request = QueryNetworkRequest;
    type Response = QueryNetworkResponse;

    fn grpc_path() -> &'static str {
        "/types.Query/Network"
    }
}

#[cw_serde]
pub struct Network {
    pub bond_reward_rune: Uint128,
    pub total_bond_units: Uint128,
    pub effective_security_bond: Uint128,
    pub total_reserve: Uint128,
    pub vaults_migrating: bool,
    pub gas_spent_rune: Uint128,
    pub gas_withheld_rune: Uint128,
    pub outbound_fee_multiplier: u16,
    pub native_outbound_fee_rune: Uint128,
    pub native_tx_fee_rune: Uint128,
    pub tns_register_fee_rune: Uint128,
    pub tns_fee_per_block_rune: Uint128,
    pub rune_price_in_tor: Decimal,
    pub tor_price_in_rune: Decimal,
}

impl TryFrom<QueryNetworkResponse> for Network {
    type Error = TryFromNetworkError;

    fn try_from(value: QueryNetworkResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            bond_reward_rune: Uint128::from_str(value.bond_reward_rune.as_str())?,
            total_bond_units: Uint128::from_str(value.total_bond_units.as_str())?,
            effective_security_bond: Uint128::from_str(value.effective_security_bond.as_str())?,
            total_reserve: Uint128::from_str(value.total_reserve.as_str())?,
            vaults_migrating: value.vaults_migrating,
            gas_spent_rune: Uint128::from_str(value.gas_spent_rune.as_str())?,
            gas_withheld_rune: Uint128::from_str(value.gas_withheld_rune.as_str())?,
            outbound_fee_multiplier: u16::from_str(value.outbound_fee_multiplier.as_str())?,
            native_outbound_fee_rune: Uint128::from_str(value.native_outbound_fee_rune.as_str())?,
            native_tx_fee_rune: Uint128::from_str(value.native_tx_fee_rune.as_str())?,
            tns_register_fee_rune: Uint128::from_str(value.tns_register_fee_rune.as_str())?,
            tns_fee_per_block_rune: Uint128::from_str(value.tns_fee_per_block_rune.as_str())?,
            rune_price_in_tor: Decimal::from_str(value.rune_price_in_tor.as_str())?
                .div(Uint128::from(10u128).pow(8)),
            tor_price_in_rune: Decimal::from_str(value.tor_price_in_rune.as_str())?
                .div(Uint128::from(10u128).pow(8)),
        })
    }
}

impl Network {
    pub fn load(q: QuerierWrapper) -> Result<Self, TryFromNetworkError> {
        let req = QueryNetworkRequest {
            height: "0".to_string(),
        };
        let res = QueryNetworkResponse::get(q, req)?;
        Network::try_from(res)
    }
}

#[derive(Error, Debug)]
pub enum TryFromNetworkError {
    #[error("{0}")]
    Std(#[from] StdError),
    #[error("{0}")]
    ParseInt(#[from] ParseIntError),
    #[error("{0}")]
    Query(#[from] QueryError),
}

#[cfg(test)]
mod msg_deposit_tests {
    use anybuf::Anybuf;
    use cosmwasm_std::{testing::mock_dependencies, AnyMsg, Api, Coin, CosmosMsg};

    use crate::thorchain::MsgDeposit;

    #[test]
    fn encodes_native_deposit() {
        let deps = mock_dependencies();

        let deposit_msg = MsgDeposit {
            memo: "test".to_string(),
            coins: vec![Coin::new(1000u128, "rune")],
            signer: deps
                .api
                .addr_canonicalize(deps.api.addr_make("test").as_str())
                .unwrap(),
        };

        assert_eq!(
            deposit_msg.clone().into_cosmos_msg().unwrap(),
            CosmosMsg::Any(AnyMsg {
                type_url: "/types.MsgDeposit".to_string(),
                value: Anybuf::new()
                    .append_repeated_message(
                        1,
                        &deposit_msg
                            .coins
                            .iter()
                            .map(|c| Anybuf::new()
                                .append_message(
                                    1,
                                    &Anybuf::new()
                                        .append_string(1, "THOR")
                                        .append_string(2, "RUNE")
                                        .append_string(3, "RUNE") // .append_bool(4, false)
                                                                  // .append_bool(5, false)
                                                                  // .append_bool(6, false),
                                )
                                .append_string(2, c.amount.to_string()))
                            .collect::<Vec<_>>()
                    )
                    .append_string(2, deposit_msg.memo)
                    .append_bytes(3, deposit_msg.signer.clone().to_vec())
                    .as_bytes()
                    .into()
            })
        );
    }

    #[test]
    fn encode_secured_asset() {
        let deps = mock_dependencies();

        let deposit_msg = MsgDeposit {
            memo: "test".to_string(),
            coins: vec![Coin::new(1000u128, "gaia-atom")],
            signer: deps
                .api
                .addr_canonicalize(deps.api.addr_make("test").as_str())
                .unwrap(),
        };

        assert_eq!(
            deposit_msg.clone().into_cosmos_msg().unwrap(),
            CosmosMsg::Any(AnyMsg {
                type_url: "/types.MsgDeposit".to_string(),
                value: Anybuf::new()
                    .append_repeated_message(
                        1,
                        &deposit_msg
                            .coins
                            .iter()
                            .map(|c| Anybuf::new()
                                .append_message(
                                    1,
                                    &Anybuf::new()
                                        .append_string(1, "gaia")
                                        .append_string(2, "atom")
                                        .append_string(3, "atom")
                                        .append_bool(4, false)
                                        .append_bool(5, false)
                                        .append_bool(6, true),
                                )
                                .append_string(2, c.amount.to_string()))
                            .collect::<Vec<_>>()
                    )
                    .append_string(2, deposit_msg.memo)
                    .append_bytes(3, deposit_msg.signer.clone().to_vec())
                    .as_bytes()
                    .into()
            })
        );
    }
}
