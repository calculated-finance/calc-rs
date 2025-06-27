use calc_rs::scheduler::{SchedulerExecuteMsg, SchedulerQueryMsg};
use cosmwasm_schema::write_api;
use scheduler::contract::InstantiateMsg;

fn main() {
    write_api! {
        instantiate: InstantiateMsg,
        execute: SchedulerExecuteMsg,
        query: SchedulerQueryMsg,
    }
}
