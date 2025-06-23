use calc_rs::{
    types::Trigger,
    types::{Condition, Contract, ContractError, ContractResult},
};
use cosmwasm_std::{CosmosMsg, Env, Response, StdError};

pub trait Executable {
    fn can_execute(&self, env: &Env) -> bool;
    fn execute(&self, env: &Env) -> ContractResult;
}

impl Executable for Trigger {
    fn can_execute(&self, env: &Env) -> bool {
        match self.condition {
            Condition::BlockHeight { height } => height <= env.block.height,
            Condition::Timestamp { timestamp } => timestamp <= env.block.time,
        }
    }

    fn execute(&self, env: &Env) -> ContractResult {
        if !self.can_execute(env) {
            return Err(ContractError::Std(StdError::generic_err(format!(
                "Condition not met: {:?}",
                self.condition
            ))));
        }

        let mut messages: Vec<CosmosMsg> = vec![];

        match self.condition {
            Condition::Timestamp { .. } | Condition::BlockHeight { .. } => {
                let execute_message = Contract(self.to.clone()).call(self.msg.clone(), vec![]);
                messages.push(execute_message);
            }
        }

        Ok(Response::default().add_messages(messages))
    }
}

#[cfg(test)]
mod can_execute_tests {
    use super::*;
    use cosmwasm_std::{testing::mock_env, Addr, Timestamp};
    use rstest::rstest;

    #[rstest]
    #[case(Condition::BlockHeight { height: 0 }, 0, true)]
    #[case(Condition::BlockHeight { height: 0 }, 1, true)]
    #[case(Condition::BlockHeight { height: 10 }, 0, false)]
    #[case(Condition::BlockHeight { height: 10 }, 9, false)]
    #[case(Condition::BlockHeight { height: 10 }, 10, true)]
    #[case(Condition::BlockHeight { height: 10 }, 11, true)]
    fn test_can_execute(
        #[case] condition: Condition,
        #[case] block_height: u64,
        #[case] expected: bool,
    ) {
        use cosmwasm_std::Binary;

        let mut env = mock_env();
        env.block.height = block_height;
        assert_eq!(
            Trigger {
                condition,
                to: Addr::unchecked("recipient"),
                msg: Binary::default(),
                execution_rebate: vec![],
                id: 1,
                owner: Addr::unchecked("owner"),
            }
            .can_execute(&env),
            expected
        );
    }

    #[rstest]
    #[case(Condition::Timestamp { timestamp: Timestamp::from_seconds(0) }, 0, true)]
    #[case(Condition::Timestamp { timestamp: Timestamp::from_seconds(0) }, 1, true)]
    #[case(Condition::Timestamp { timestamp: Timestamp::from_seconds(10) }, 0, false)]
    #[case(Condition::Timestamp { timestamp: Timestamp::from_seconds(10) }, 9, false)]
    #[case(Condition::Timestamp { timestamp: Timestamp::from_seconds(10) }, 10, true)]
    #[case(Condition::Timestamp { timestamp: Timestamp::from_seconds(10) }, 11, true)]
    fn test_can_execute_timestamp(
        #[case] condition: Condition,
        #[case] block_time: u64,
        #[case] expected: bool,
    ) {
        use cosmwasm_std::Binary;
        let mut env = mock_env();
        env.block.time = Timestamp::from_seconds(block_time);
        assert_eq!(
            Trigger {
                condition,
                to: Addr::unchecked("recipient"),
                msg: Binary::default(),
                execution_rebate: vec![],
                id: 1,
                owner: Addr::unchecked("owner"),
            }
            .can_execute(&env),
            expected
        );
    }
}
