use calc_rs::types::ContractResult;
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Decimal, Deps, MessageInfo, StdResult};

pub trait Exchange {
    fn can_swap(&self, deps: Deps, swap_denom: &str, target_denom: &str) -> StdResult<bool>;
    fn get_expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: Coin,
        target_denom: &str,
    ) -> StdResult<Coin>;
    fn get_spot_price(
        &self,
        deps: Deps,
        swap_denom: &str,
        target_denom: &str,
    ) -> StdResult<Decimal>;
    fn swap(
        &self,
        deps: Deps,
        info: MessageInfo,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
    ) -> ContractResult;
}

#[cw_serde]
#[derive(Hash)]
pub enum PositionType {
    Enter,
    Exit,
}

#[cw_serde]
pub struct Pair {
    pub base_denom: String,
    pub quote_denom: String,
    pub address: Addr,
    pub decimal_delta: i8,
    pub price_precision: u8,
}

impl Pair {
    pub fn position_type(&self, swap_denom: &str) -> PositionType {
        if self.quote_denom == swap_denom {
            PositionType::Enter
        } else {
            PositionType::Exit
        }
    }

    pub fn denoms(&self) -> [String; 2] {
        [self.base_denom.clone(), self.quote_denom.clone()]
    }

    pub fn other_denom(&self, swap_denom: String) -> String {
        if self.quote_denom == swap_denom {
            self.base_denom.clone()
        } else {
            self.quote_denom.clone()
        }
    }
}
