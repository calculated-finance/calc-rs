use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, CosmosMsg, Decimal, Deps, Env, StdError, StdResult,
};
use rujira_rs::fin::{ExecuteMsg, SwapRequest};

use crate::{
    actions::swaps::{
        fin::FinRoute,
        swap::{Adjusted, Swap, SwapAmountAdjustment, SwapQuote, SwapRoute},
    },
    authz,
    manager::Affiliate,
    operation::Operation,
};

#[cw_serde]
pub struct DelegatedSwap {
    pub on_behalf_of: Addr,
    pub pair_address: Addr,
    pub swap_amount: Coin,
    pub minimum_receive_amount: Coin,
    pub maximum_slippage_bps: u64,
    pub adjustment: SwapAmountAdjustment,
    /// Manager-controlled fee recipients. Always overwritten during node initialization.
    pub affiliates: Option<Vec<Affiliate>>,
}

impl DelegatedSwap {
    fn as_swap(&self) -> Swap {
        Swap {
            swap_amount: self.swap_amount.clone(),
            minimum_receive_amount: self.minimum_receive_amount.clone(),
            maximum_slippage_bps: self.maximum_slippage_bps,
            adjustment: self.adjustment.clone(),
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: self.pair_address.clone(),
            })],
        }
    }

    fn execute_unsafe(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, DelegatedSwap)> {
        let gross_quote =
            self.as_swap()
                .best_quote_for(deps, env, &self.on_behalf_of, &self.on_behalf_of)?;

        let affiliates = self.affiliates.clone().unwrap_or_default();
        let mut inner_messages = Vec::with_capacity(affiliates.len() + 1);
        let mut net_swap_amount = gross_quote.swap_amount.amount;

        for affiliate in affiliates {
            let fee = gross_quote
                .swap_amount
                .amount
                .mul_floor(Decimal::bps(affiliate.bps));

            if fee.is_zero() {
                continue;
            }

            net_swap_amount = net_swap_amount.checked_sub(fee)?;
            inner_messages.push(authz::bank_send(
                &self.on_behalf_of,
                &affiliate.address,
                vec![Coin::new(fee, gross_quote.swap_amount.denom.clone())],
            ));
        }

        if net_swap_amount.is_zero() {
            return Err(StdError::generic_err(
                "Swap amount after affiliate fees is zero",
            ));
        }

        let minimum_receive_amount =
            gross_quote
                .minimum_receive_amount
                .amount
                .mul_ceil(Decimal::from_ratio(
                    net_swap_amount,
                    gross_quote.swap_amount.amount,
                ));

        let net_quote = gross_quote.route.clone().validate_adjusted(
            deps,
            env,
            SwapQuote {
                swap_amount: Coin::new(net_swap_amount, gross_quote.swap_amount.denom.clone()),
                minimum_receive_amount: Coin::new(
                    minimum_receive_amount,
                    gross_quote.minimum_receive_amount.denom.clone(),
                ),
                maximum_slippage_bps: gross_quote.maximum_slippage_bps,
                adjustment: gross_quote.adjustment.clone(),
                route: gross_quote.route,
                destination: self.on_behalf_of.clone(),
                state: Adjusted,
            },
        )?;

        inner_messages.push(authz::execute_contract(
            &self.on_behalf_of,
            &self.pair_address,
            to_json_binary(&ExecuteMsg::Swap(SwapRequest::Min {
                min_return: net_quote.minimum_receive_amount.amount,
                to: Some(self.on_behalf_of.to_string()),
                callback: None,
            }))?,
            vec![net_quote.swap_amount],
        ));

        Ok((
            vec![authz::exec(&env.contract.address, inner_messages)],
            self,
        ))
    }
}

impl Operation<DelegatedSwap> for DelegatedSwap {
    fn init(mut self, deps: Deps, env: &Env, affiliates: &[Affiliate]) -> StdResult<DelegatedSwap> {
        deps.api.addr_validate(self.on_behalf_of.as_str())?;
        self.as_swap().validate(deps, env)?;
        self.affiliates = Some(affiliates.to_vec());
        Ok(self)
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, DelegatedSwap)> {
        self.execute_unsafe(deps, env)
    }
}
