use calc_rs::types::{
    Condition, Schedule, StrategyStatus, TwapConfig, TwapExecuteMsg, TwapStatistics,
};
use cosmwasm_std::{Coin, Env, StdError, StdResult, Storage};
use cw_storage_plus::Item;

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
                    Schedule::Blocks { interval, previous } => {
                        Condition::BlocksCompleted(previous.unwrap_or(env.block.height) + interval)
                    }
                    Schedule::Time { duration, previous } => Condition::TimestampElapsed(
                        previous
                            .unwrap_or(env.block.time)
                            .plus_seconds(duration.as_secs()),
                    ),
                },
                Condition::BalanceAvailable {
                    address: env.contract.address.clone(),
                    amount: Coin::new(1u128, update.swap_amount.denom.clone()),
                },
                Condition::ExchangeLiquidityProvided {
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

pub const STATE: Item<TwapExecuteMsg> = Item::new("state");
pub const CONFIG: ConfigStore = ConfigStore {
    item: Item::new("config"),
};
pub const STATS: Item<TwapStatistics> = Item::new("statistics");
