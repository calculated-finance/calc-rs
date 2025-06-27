use calc_rs::manager::{ManagerExecuteMsg, ManagerInstantiateMsg, ManagerQueryMsg};
use cosmwasm_schema::write_api;

fn main() {
    write_api! {
        instantiate: ManagerInstantiateMsg,
        execute: ManagerExecuteMsg,
        query: ManagerQueryMsg,
    }
}
