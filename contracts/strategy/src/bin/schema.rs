use calc_rs::strategy::{StrategyExecuteMsg, StrategyInstantiateMsg, StrategyQueryMsg};
use cosmwasm_schema::write_api;

fn main() {
    write_api! {
        instantiate: StrategyInstantiateMsg,
        execute: StrategyExecuteMsg,
        query: StrategyQueryMsg,
    }
}
