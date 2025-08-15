use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Deps, Env, StdError, StdResult};
use rujira_rs::{
    fin::{BookResponse, ConfigResponse, QueryMsg},
    query::Pool,
    Layer1Asset,
};

#[cw_serde]
pub enum PriceSource {
    Fin { address: Addr },
    Thorchain,
}

#[cw_serde]
pub struct AssetValueRatio {
    pub numerator: String,
    pub denominator: String,
    pub ratio: Decimal,
    pub tolerance: Decimal,
    pub oracle: PriceSource,
}

impl AssetValueRatio {
    pub fn validate(&self, deps: Deps) -> StdResult<()> {
        match self.oracle {
            PriceSource::Fin { ref address } => {
                let pair = deps
                    .querier
                    .query_wasm_smart::<ConfigResponse>(address.clone(), &QueryMsg::Config {})?;

                let denoms = [pair.denoms.base(), pair.denoms.quote()];

                if !denoms.contains(&self.numerator.as_str()) {
                    return Err(StdError::generic_err(format!(
                        "Pair at {} does not include asset {}",
                        address, self.numerator
                    )));
                }

                if !denoms.contains(&self.denominator.as_str()) {
                    return Err(StdError::generic_err(format!(
                        "Pair at {} does not include asset {}",
                        address, self.denominator
                    )));
                }
            }
            PriceSource::Thorchain => {
                fetch_l1_asset_price(deps, &self.numerator)?;
                fetch_l1_asset_price(deps, &self.denominator)?;
            }
        }

        Ok(())
    }

    pub fn is_satisfied(&self, deps: Deps, env: &Env) -> StdResult<bool> {
        let price = match self.oracle.clone() {
            PriceSource::Fin { address } => {
                let book_response = deps.querier.query_wasm_smart::<BookResponse>(
                    &address,
                    &QueryMsg::Book {
                        limit: Some(1),
                        offset: None,
                    },
                )?;

                let pair = deps
                    .querier
                    .query_wasm_smart::<ConfigResponse>(address, &QueryMsg::Config {})?;

                let mid_price = (book_response.base[0].price + book_response.quote[0].price)
                    / Decimal::from_ratio(2u128, 1u128);

                if pair.denoms.base() == self.numerator {
                    Decimal::one() / mid_price
                } else {
                    mid_price
                }
            }
            PriceSource::Thorchain => {
                let numerator_price = fetch_l1_asset_price(deps, &self.numerator)?;
                let denominator_price = fetch_l1_asset_price(deps, &self.denominator)?;

                numerator_price
                    .checked_div(denominator_price)
                    .map_err(|_| {
                        StdError::generic_err(format!(
                        "Failed to calculate asset value ratio: L1 oracle price for '{}' is zero",
                        self.denominator
                    ))
                    })?
            }
        };

        let numerator_balance = deps
            .querier
            .query_balance(&env.contract.address, &self.numerator)?;

        let denominator_balance = deps
            .querier
            .query_balance(&env.contract.address, &self.denominator)?;

        if denominator_balance.amount.is_zero() {
            return Err(StdError::generic_err("Denominator balance is zero"));
        }

        let balance_ratio =
            Decimal::from_ratio(numerator_balance.amount, denominator_balance.amount);

        let value_ratio = balance_ratio * price;

        Ok(value_ratio.abs_diff(self.ratio) < self.tolerance)
    }
}

fn fetch_l1_asset_price(deps: Deps, asset: &str) -> StdResult<Decimal> {
    let layer_1_asset = Layer1Asset::from_native(asset.to_string())
        .map_err(|e| StdError::generic_err(format!("'{}' is not a secured asset: {e}", asset)))?;

    Pool::load(deps.querier, &layer_1_asset)
        .map_err(|e| {
            StdError::generic_err(format!(
                "Failed to load oracle price for {layer_1_asset}, error: {e}"
            ))
        })
        .map(|pool| pool.asset_tor_price)
}
