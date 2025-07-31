use calc_rs::strategy::{Indexed, Strategy, StrategyExecuteMsg, StrategyQueryMsg};
use cosmwasm_schema::write_api;

fn main() {
    write_api! {
        instantiate: Strategy<Indexed>,
        execute: StrategyExecuteMsg,
        query: StrategyQueryMsg,
    }
}
