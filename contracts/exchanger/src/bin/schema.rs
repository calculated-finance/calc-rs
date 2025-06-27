use calc_rs::exchanger::{ExchangeExecuteMsg, ExchangeQueryMsg};
use cosmwasm_schema::write_api;
use exchanger::contract::InstantiateMsg;

fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: ExchangeExecuteMsg,
        query: ExchangeQueryMsg,
    }
}
