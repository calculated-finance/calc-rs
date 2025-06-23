use calc_rs::types::{ExchangeExecuteMsg, ExchangeQueryMsg};
use cosmwasm_schema::write_api;
use exchange::contract::InstantiateMsg;

fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: ExchangeExecuteMsg,
        query: ExchangeQueryMsg,
    }
}
