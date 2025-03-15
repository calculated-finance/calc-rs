use calc_rs::msg::{FactoryExecuteMsg, FactoryInstantiateMsg, FactoryQueryMsg};
use cosmwasm_schema::write_api;

fn main() {
    write_api! {
        instantiate: FactoryInstantiateMsg,
        execute: FactoryExecuteMsg,
        query: FactoryQueryMsg,
    }
}
