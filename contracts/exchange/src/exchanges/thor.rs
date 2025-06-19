use anybuf::Anybuf;
use calc_rs::{
    math::checked_mul,
    types::{ContractResult, ExpectedReturnAmount},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, AnyMsg, Binary, CanonicalAddr, Coin, CosmosMsg, Decimal, Deps, Env, QuerierWrapper,
    Response, StdError, StdResult, Uint128,
};
use prost::{DecodeError, EncodeError, Message};
use rujira_rs::{
    proto::types::{QueryQuoteSwapRequest, QueryQuoteSwapResponse},
    query::Pool,
    Asset, Layer1Asset, NativeAsset, SecuredAsset,
};
use thiserror::Error;

use crate::types::Exchange;

#[cw_serde]
pub struct ThorExchange {}

impl ThorExchange {
    pub fn new() -> Self {
        ThorExchange {}
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

impl QueryablePair for QueryQuoteSwapResponse {
    type Request = QueryQuoteSwapRequest;
    type Response = QueryQuoteSwapResponse;

    fn grpc_path() -> &'static str {
        "/types.Query/QuoteSwap"
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

struct MsgDeposit {
    pub memo: String,
    pub coins: Vec<Coin>,
    pub signer: CanonicalAddr,
}

impl From<MsgDeposit> for CosmosMsg {
    fn from(value: MsgDeposit) -> Self {
        let coins: Vec<Anybuf> = value
            .coins
            .iter()
            .map(|c| {
                let asset = layer_1_asset(&NativeAsset::new(&c.denom))
                    .unwrap()
                    .denom_string()
                    .to_ascii_uppercase();
                let (chain, symbol) = asset.split_once('.').unwrap();

                Anybuf::new()
                    .append_message(
                        1,
                        &Anybuf::new()
                            .append_string(1, chain)
                            .append_string(2, symbol)
                            .append_string(3, symbol)
                            .append_bool(4, false)
                            .append_bool(5, false)
                            .append_bool(6, c.denom.to_lowercase() != "rune"),
                    )
                    .append_string(2, c.amount.to_string())
            })
            .collect();

        let value = Anybuf::new()
            .append_repeated_message(1, &coins)
            .append_string(2, value.memo)
            .append_bytes(3, value.signer.to_vec());

        CosmosMsg::Any(AnyMsg {
            type_url: "/types.MsgDeposit".to_string(),
            value: value.as_bytes().into(),
        })
    }
}

pub fn layer_1_asset(denom: &NativeAsset) -> StdResult<Layer1Asset> {
    let denom_string = denom.denom_string();

    if denom_string.contains("rune") {
        return Ok(Layer1Asset::new("THOR", "RUNE"));
    }

    let (chain, symbol) = denom_string
        .split_once('-')
        .ok_or_else(|| StdError::generic_err(format!("Invalid layer 1 asset: {}", denom)))?;

    Ok(Layer1Asset::new(
        &chain.to_ascii_uppercase(),
        &symbol.to_ascii_uppercase(),
    ))
}

fn secured_asset(asset: &Layer1Asset) -> StdResult<SecuredAsset> {
    match asset.denom_string().to_uppercase().split_once(".") {
        Some((chain, symbol)) => Ok(SecuredAsset::new(chain, symbol)),
        None => Err(StdError::generic_err(format!(
            "Invalid layer 1 asset: {}",
            asset.denom_string()
        ))),
    }
}

fn load_pool(deps: Deps, asset: &Layer1Asset) -> StdResult<Pool> {
    Ok(Pool::load(deps.querier, asset).map_err(|e| {
        StdError::generic_err(format!(
            "Failed to load pool for asset {}: {}",
            asset.denom_string(),
            e
        ))
    })?)
}

fn get_pools(
    deps: Deps,
    swap_denom: &NativeAsset,
    target_denom: &NativeAsset,
) -> Result<Vec<Pool>, StdError> {
    Ok([swap_denom, target_denom]
        .iter()
        .filter(|&&denom| !denom.denom_string().contains("rune"))
        .map(|&denom| load_pool(deps, &layer_1_asset(denom)?))
        .collect::<StdResult<Vec<Pool>>>()?)
}

fn get_expected_receive_amount(
    pool: &Pool,
    swap_asset: &Layer1Asset,
    swap_amount: &Uint128,
) -> StdResult<(Layer1Asset, Uint128)> {
    let receive_asset = match swap_asset.denom_string().as_str() {
        "thor.rune" => match pool.asset.clone() {
            Asset::Layer1(asset) => asset,
            _ => return Err(StdError::generic_err("Pool asset is not a Layer 1 asset")),
        },
        _ => Layer1Asset::new("THOR", "RUNE"),
    };

    let receive_amount = swap_amount
        .checked_mul(pool.balance_asset)?
        .checked_mul(pool.balance_rune)?
        .checked_div(
            swap_amount
                .checked_add(match swap_asset.denom_string().as_str() {
                    "thor.rune" => pool.balance_rune,
                    _ => pool.balance_asset,
                })?
                .pow(2),
        )?;

    Ok((receive_asset, receive_amount))
}

fn get_spot_price(pool: &Pool, swap_asset: &Layer1Asset) -> StdResult<(Layer1Asset, Decimal)> {
    let pool_asset = match pool.asset.clone() {
        Asset::Layer1(asset) => asset,
        _ => return Err(StdError::generic_err("Pool asset is not a Layer 1 asset")),
    };

    let pool_asset_price = Decimal::from_ratio(pool.balance_rune, pool.balance_asset);

    match swap_asset.denom_string().as_str() {
        "thor.rune" => Ok((pool_asset, pool_asset_price)),
        _ => Ok((
            Layer1Asset::new("THOR", "RUNE"),
            Decimal::one() / (pool_asset_price),
        )),
    }
}

impl Exchange for ThorExchange {
    fn can_swap(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
    ) -> StdResult<bool> {
        let expected_return_amount = self
            .expected_receive_amount(
                deps,
                swap_amount,
                &NativeAsset::new(&minimum_receive_amount.denom),
            )
            .unwrap_or(ExpectedReturnAmount {
                return_amount: Coin {
                    denom: minimum_receive_amount.denom.clone(),
                    amount: Uint128::zero(),
                },
                slippage: Decimal::zero(),
            });

        Ok(expected_return_amount.return_amount.amount >= minimum_receive_amount.amount)
    }

    fn route(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
    ) -> StdResult<Vec<Coin>> {
        let pools = get_pools(deps, &NativeAsset::new(&swap_amount.denom), target_denom)?;

        if pools.is_empty() {
            return Err(StdError::generic_err("No valid route found"));
        }

        let mut route = vec![swap_amount.clone()];

        for (i, pool) in pools.iter().enumerate() {
            let (out_asset, out_amount) = get_expected_receive_amount(
                pool,
                &layer_1_asset(&NativeAsset::new(&route[i].denom))?,
                &route[i].amount,
            )?;

            if out_amount.is_zero() {
                return Err(StdError::generic_err("Received zero amount from pool"));
            }

            route.push(Coin {
                denom: if out_asset.is_rune() {
                    "rune".to_string()
                } else {
                    secured_asset(&out_asset)?.denom_string()
                },
                amount: out_amount,
            });
        }

        Ok(route)
    }

    fn expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &NativeAsset,
    ) -> StdResult<ExpectedReturnAmount> {
        let swap_asset = NativeAsset::new(&swap_amount.denom);

        let pools = get_pools(deps, &swap_asset, target_denom)?;

        if pools.is_empty() {
            return Err(StdError::generic_err("No valid route found"));
        }

        let (_, out_amount) = pools.iter().fold(
            (layer_1_asset(&swap_asset)?, swap_amount.amount),
            |(in_asset, in_amount), pool| {
                get_expected_receive_amount(pool, &in_asset, &in_amount).expect(
                    format!(
                        "Failed to get expected receive amount for swapping {} {} in {} pool",
                        in_amount,
                        in_asset.denom_string(),
                        pool.asset
                    )
                    .as_str(),
                )
            },
        );

        let spot_price = self.spot_price(deps, &swap_asset, target_denom)?;

        let optimal_return_amount = checked_mul(swap_amount.amount, Decimal::one() / spot_price)
            .map_err(|e| {
                StdError::generic_err(format!("Failed to calculate optimal return amount: {}", e))
            })?;

        let slippage =
            Decimal::one().checked_sub(Decimal::from_ratio(out_amount, optimal_return_amount))?;

        Ok(ExpectedReturnAmount {
            return_amount: Coin {
                denom: target_denom.denom_string(),
                amount: out_amount,
            },
            slippage,
        })
    }

    fn spot_price(
        &self,
        deps: Deps,
        swap_denom: &NativeAsset,
        target_denom: &NativeAsset,
    ) -> StdResult<Decimal> {
        let pools = get_pools(deps, swap_denom, target_denom)?;

        if pools.is_empty() {
            return Err(StdError::generic_err("No valid route found"));
        }

        let (_, price) = pools.iter().fold(
            (layer_1_asset(swap_denom)?, Decimal::one()),
            |(asset, out_price), pool| {
                get_spot_price(pool, &asset)
                    .map(|(asset, price)| (asset, out_price * price))
                    .expect(&format!(
                        "Failed to get spot price for swapping {} in {} pool",
                        asset.denom_string(),
                        pool.asset
                    ))
            },
        );

        Ok(price)
    }

    fn swap(
        &self,
        deps: Deps,
        env: Env,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        recipient: Addr,
    ) -> ContractResult {
        let swap_asset = secured_asset(&layer_1_asset(&NativeAsset::new(&swap_amount.denom))?)?;
        let receive_asset = secured_asset(&layer_1_asset(&NativeAsset::new(
            &minimum_receive_amount.denom,
        ))?)?;

        let memo = format!(
            "=:{}:{}:{}",
            receive_asset.denom_string().to_ascii_uppercase(),
            recipient,
            minimum_receive_amount.amount
        );

        let swap_msg = MsgDeposit {
            memo,
            coins: vec![Coin {
                denom: swap_asset.denom_string(),
                amount: swap_amount.amount,
            }],
            signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
        };

        Ok(Response::new().add_message(swap_msg))
    }
}

#[cfg(test)]
mod tests {
    use cosmwasm_std::testing::mock_dependencies;

    use super::*;

    #[test]
    fn test_layer_1_asset() {
        let denom_string = layer_1_asset(&NativeAsset::new("rune"))
            .unwrap()
            .denom_string();
        let (chain, symbol) = denom_string.split_once('.').unwrap();

        assert_eq!(chain, "thor");
        assert_eq!(symbol, "rune");

        let denom_string = layer_1_asset(&NativeAsset::new("eth-usd"))
            .unwrap()
            .denom_string();
        let (chain, symbol) = denom_string.split_once('.').unwrap();

        assert_eq!(chain, "eth");
        assert_eq!(symbol, "usd");

        let denom_string = layer_1_asset(&NativeAsset::new(
            "eth-usd-0xdac17f958d2ee523a2206206994597c13d831ec7",
        ))
        .unwrap()
        .denom_string();
        let (chain, symbol) = denom_string.split_once('.').unwrap();

        assert_eq!(chain, "eth");
        assert_eq!(symbol, "usd-0xdac17f958d2ee523a2206206994597c13d831ec7");

        let err = layer_1_asset(&NativeAsset::new("uruji")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Generic error: Invalid layer 1 asset: uruji"
        );
    }

    #[test]
    fn test_secured_asset() {
        let secured_denom = secured_asset(&layer_1_asset(&NativeAsset::new("ETH.ETH")).unwrap())
            .unwrap()
            .denom_string();
        let (chain, symbol) = secured_denom.split_once('-').unwrap();

        assert_eq!(chain, "eth");
        assert_eq!(symbol, "eth");

        let secured_denom = secured_asset(&layer_1_asset(&NativeAsset::new("rune")).unwrap())
            .unwrap()
            .denom_string();
        let (chain, symbol) = secured_denom.split_once('-').unwrap();

        assert_eq!(chain, "thor");
        assert_eq!(symbol, "rune");
    }

    #[test]
    fn test_get_pools() {
        let deps = mock_dependencies();

        let pools = get_pools(
            deps.as_ref(),
            &NativeAsset::new("eth-eth"),
            &NativeAsset::new("eth-usdc"),
        )
        .unwrap();
        assert!(pools.is_empty());
    }
}
