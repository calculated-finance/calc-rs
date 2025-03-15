use calc_rs::msg::{VaultExecuteMsg, VaultInstantiateMsg, VaultQueryMsg};
use cosmwasm_schema::write_api;

fn main() {
    write_api! {
        instantiate: VaultInstantiateMsg,
        execute: VaultExecuteMsg,
        query: VaultQueryMsg,
    }
}
