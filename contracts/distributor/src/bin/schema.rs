use calc_rs::distributor::{DistributorExecuteMsg, DistributorInstantiateMsg, DistributorQueryMsg};
use cosmwasm_schema::write_api;

fn main() {
    write_api! {
        instantiate: DistributorInstantiateMsg,
        execute: DistributorExecuteMsg,
        query: DistributorQueryMsg,
    }
}
