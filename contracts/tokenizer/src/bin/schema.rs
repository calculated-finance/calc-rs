use calc_rs::scheduler::{SchedulerExecuteMsg, SchedulerInstantiateMsg, SchedulerQueryMsg};
use cosmwasm_schema::write_api;

fn main() {
    write_api! {
        instantiate: SchedulerInstantiateMsg,
        execute: SchedulerExecuteMsg,
        query: SchedulerQueryMsg,
    }
}
