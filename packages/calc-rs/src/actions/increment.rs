use cosmwasm_schema::cw_serde;
use cosmwasm_std::{CosmosMsg, Deps, Env, StdResult};

use crate::operation::Operation;

#[cw_serde]
pub struct Increment {
    pub count: u32,
    pub label: String,
}

impl Operation<Increment> for Increment {
    fn init(
        self,
        _deps: Deps,
        _env: &Env,
        _affiliates: &[crate::manager::Affiliate],
    ) -> StdResult<Increment> {
        Ok(Increment {
            count: 0,
            label: self.label,
        })
    }

    fn execute(self, _deps: Deps, _env: &Env) -> StdResult<(Vec<CosmosMsg>, Increment)> {
        Ok((
            vec![],
            Increment {
                count: self.count + 1,
                label: self.label,
            },
        ))
    }
}
