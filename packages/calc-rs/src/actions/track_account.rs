use std::vec;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, CosmosMsg, Deps, Env, StdResult, Uint128};

use crate::{manager::Affiliate, operation::Operation, strategy::StrategyQueryMsg};

#[cw_serde]
pub struct TrackAccount {
    pub denom: String,
    pub debit: Uint128,
    pub credit: Uint128,
}

impl Operation<TrackAccount> for TrackAccount {
    fn init(self, deps: Deps, env: &Env, _affiliates: &[Affiliate]) -> StdResult<TrackAccount> {
        let balances = deps
            .querier
            .query_wasm_smart::<Vec<Coin>>(&env.contract.address, &StrategyQueryMsg::Balances {})?;

        let balance = balances
            .into_iter()
            .find(|coin| coin.denom == self.denom)
            .map(|coin| coin.amount)
            .unwrap_or_default();

        Ok(TrackAccount {
            denom: self.denom,
            debit: Uint128::zero(),
            credit: balance,
        })
    }

    fn execute(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, TrackAccount)> {
        let balance = deps
            .querier
            .query_wasm_smart::<Vec<Coin>>(&env.contract.address, &StrategyQueryMsg::Balances {})?
            .into_iter()
            .find(|coin| coin.denom == self.denom)
            .map(|coin| coin.amount)
            .unwrap_or_else(|| Uint128::zero());

        let previous_balance = self.credit.saturating_sub(self.debit);

        let (debit, credit) = if balance < previous_balance {
            (previous_balance - balance, Uint128::zero())
        } else {
            (Uint128::zero(), balance - previous_balance)
        };

        Ok((
            vec![],
            TrackAccount {
                denom: self.denom,
                debit: self.debit.checked_add(debit)?,
                credit: self.credit.checked_add(credit)?,
            },
        ))
    }
}
