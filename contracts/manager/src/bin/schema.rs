use calc_rs::manager::{ManagerConfig, ManagerExecuteMsg, ManagerQueryMsg};
use cosmwasm_schema::write_api;

fn main() {
    write_api! {
        instantiate: ManagerConfig,
        execute: ManagerExecuteMsg,
        query: ManagerQueryMsg,
    }
}
