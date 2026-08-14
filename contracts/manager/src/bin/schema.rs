use calc_manager::contract::MigrateMsg;
use calc_rs::manager::{ManagerConfig, ManagerExecuteMsg, ManagerQueryMsg, ManagerSudoMsg};
use cosmwasm_schema::write_api;

fn main() {
    write_api! {
        instantiate: ManagerConfig,
        execute: ManagerExecuteMsg,
        query: ManagerQueryMsg,
        migrate: MigrateMsg,
        sudo: ManagerSudoMsg,
    }
}
