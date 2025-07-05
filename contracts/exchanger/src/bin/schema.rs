use calc_rs::exchanger::{ExchangerExecuteMsg, ExchangerInstantiateMsg, ExchangerQueryMsg};
use cosmwasm_schema::write_api;

fn main() {
    write_api! {
        instantiate: ExchangerInstantiateMsg,
        execute: ExchangerExecuteMsg,
        query: ExchangerQueryMsg,
    }
}
