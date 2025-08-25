use calc_rs::tokenizer::{TokenizerExecuteMsg, TokenizerInstantiateMsg, TokenizerQueryMsg};
use cosmwasm_schema::write_api;

fn main() {
    write_api! {
        instantiate: TokenizerInstantiateMsg,
        execute: TokenizerExecuteMsg,
        query: TokenizerQueryMsg,
    }
}
