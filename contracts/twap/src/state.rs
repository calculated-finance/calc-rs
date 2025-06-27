use calc_rs::{
    manager::StrategyExecuteMsg,
    twap::TwapConfig,
    types::{Condition, Schedule, StrategyStatus},
};
use cosmwasm_std::{Coin, Env, StdError, StdResult, Storage, Timestamp};
use cw_storage_plus::Item;

use crate::types::TwapStatistics;

pub struct ConfigStore {
    item: Item<TwapConfig>,
}

impl ConfigStore {
    pub fn save(&self, storage: &mut dyn Storage, env: &Env, update: &TwapConfig) -> StdResult<()> {
        if update.maximum_slippage_bps > 10_000 {
            return Err(StdError::generic_err(
                "Maximum slippage basis points cannot exceed 10,000 (100%)",
            ));
        }

        let config = TwapConfig {
            swap_conditions: vec![
                match update.swap_cadence {
                    Schedule::Blocks { interval, previous } => Condition::BlocksCompleted(
                        previous.unwrap_or(env.block.height.saturating_sub(interval)) + interval,
                    ),
                    Schedule::Time { duration, previous } => Condition::TimestampElapsed(
                        previous
                            .unwrap_or(Timestamp::from_seconds(
                                env.block.time.seconds().saturating_sub(duration.as_secs()),
                            ))
                            .plus_seconds(duration.as_secs()),
                    ),
                },
                Condition::BalanceAvailable {
                    address: env.contract.address.clone(),
                    amount: Coin::new(1u128, update.swap_amount.denom.clone()),
                },
                Condition::ExchangeLiquidityProvided {
                    exchanger_contract: update.exchanger_contract.clone(),
                    swap_amount: update.swap_amount.clone(),
                    minimum_receive_amount: update.minimum_receive_amount.clone(),
                    maximum_slippage_bps: update.maximum_slippage_bps,
                },
            ],
            schedule_conditions: vec![
                Condition::BalanceAvailable {
                    address: env.contract.address.clone(),
                    amount: Coin::new(1u128, update.swap_amount.denom.clone()),
                },
                Condition::StrategyStatus {
                    manager_contract: update.manager_contract.clone(),
                    contract_address: env.contract.address.clone(),
                    status: StrategyStatus::Active,
                },
            ],
            ..update.clone()
        };

        self.item.save(storage, &config)
    }

    pub fn load(&self, storage: &dyn Storage) -> StdResult<TwapConfig> {
        self.item.load(storage)
    }
}

pub const STATE: Item<StrategyExecuteMsg> = Item::new("state");
pub const CONFIG: ConfigStore = ConfigStore {
    item: Item::new("config"),
};
pub const STATS: Item<TwapStatistics> = Item::new("statistics");
