use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hasher},
    vec,
};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, Coins, Decimal, Deps, Env, Event, StdError, StdResult, Timestamp,
};
use rujira_rs::{
    fin::{OrderResponse, Price, QueryMsg, Side},
    query::Pool,
    Layer1Asset,
};

use crate::{
    actions::{
        limit_order::Direction, operation::Operation, schedule::Schedule, swaps::swap::Swap,
    },
    cadence::Cadence,
    manager::{Affiliate, ManagerQueryMsg, StrategyHandle, StrategyStatus},
    strategy::StrategyMsg,
};

#[cw_serde]
pub enum Condition {
    TimestampElapsed(Timestamp),
    BlocksCompleted(u64),
    Schedule(Schedule),
    CanSwap(Swap),
    LimitOrderFilled {
        owner: Addr,
        pair_address: Addr,
        side: Side,
        price: Decimal,
    },
    BalanceAvailable {
        address: Addr,
        amount: Coin,
    },
    StrategyBalanceAvailable {
        amount: Coin,
    },
    StrategyStatus {
        manager_contract: Addr,
        contract_address: Addr,
        status: StrategyStatus,
    },
    OraclePrice {
        asset: String,
        direction: Direction,
        rate: Decimal,
    },
    Not(Box<Condition>),
}

impl Condition {
    pub fn size(&self) -> usize {
        match self {
            Condition::TimestampElapsed(_) => 1,
            Condition::BlocksCompleted(_) => 1,
            Condition::Schedule(schedule) => match schedule.cadence {
                Cadence::LimitOrder { .. } => 4,
                _ => 1,
            },
            Condition::CanSwap { .. } => 2,
            Condition::LimitOrderFilled { .. } => 2,
            Condition::BalanceAvailable { .. } => 1,
            Condition::StrategyBalanceAvailable { .. } => 1,
            Condition::StrategyStatus { .. } => 2,
            Condition::OraclePrice { .. } => 2,
            Condition::Not(condition) => condition.size(),
        }
    }

    pub fn id(&self, owner: Addr) -> StdResult<u64> {
        let salt_data = to_json_binary(&(owner, self.clone()))?;
        let mut hash = DefaultHasher::new();
        hash.write(salt_data.as_slice());
        Ok(hash.finish())
    }

    pub fn is_satisfied(&self, deps: Deps, env: &Env) -> StdResult<bool> {
        Ok(match self {
            Condition::TimestampElapsed(timestamp) => env.block.time > *timestamp,
            Condition::BlocksCompleted(height) => env.block.height > *height,
            Condition::Schedule(schedule) => {
                schedule.cadence.is_due(deps, env, &schedule.scheduler)?
            }
            Condition::LimitOrderFilled {
                owner,
                pair_address,
                side,
                price,
            } => {
                let order = deps.querier.query_wasm_smart::<OrderResponse>(
                    pair_address,
                    &QueryMsg::Order((
                        owner.to_string(),
                        side.clone(),
                        Price::Fixed(price.clone()),
                    )),
                )?;

                order.remaining.is_zero()
            }
            Condition::CanSwap(swap) => swap.best_route(deps, env)?.is_some(),
            Condition::BalanceAvailable { address, amount } => {
                let balance = deps.querier.query_balance(address, amount.denom.clone())?;
                balance.amount >= amount.amount
            }
            Condition::StrategyBalanceAvailable { amount } => {
                let balance = deps
                    .querier
                    .query_balance(&env.contract.address, amount.denom.clone())?;
                balance.amount >= amount.amount
            }
            Condition::StrategyStatus {
                manager_contract,
                contract_address,
                status,
            } => {
                let strategy = deps.querier.query_wasm_smart::<StrategyHandle>(
                    manager_contract,
                    &ManagerQueryMsg::Strategy {
                        address: contract_address.clone(),
                    },
                )?;
                strategy.status == *status
            }
            Condition::OraclePrice {
                asset,
                direction,
                rate,
            } => {
                let layer_1_asset = Layer1Asset::from_native(asset.clone()).map_err(|e| {
                    StdError::generic_err(format!(
                        "Denom ({asset}) not a secured asset, error: {e}"
                    ))
                })?;

                let oracle_price = Pool::load(deps.querier, &layer_1_asset)
                    .map_err(|e| {
                        StdError::generic_err(format!(
                            "Failed to load oracle price for {asset}, error: {e}"
                        ))
                    })?
                    .asset_tor_price;

                match direction {
                    Direction::Above => oracle_price > *rate,
                    Direction::Below => oracle_price < *rate,
                }
            }
            Condition::Not(condition) => !condition.is_satisfied(deps, env)?,
        })
    }
}

impl Operation<Condition> for Condition {
    fn init(self, deps: Deps, env: &Env, affiliates: &[Affiliate]) -> StdResult<Condition> {
        match self {
            Condition::Schedule(schedule) => schedule.init(deps, env, affiliates),
            _ => Ok(self),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<StrategyMsg>, Vec<Event>, Condition) {
        match self {
            Condition::Schedule(schedule) => schedule.execute(deps, env),
            _ => (vec![], vec![], self),
        }
    }

    fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Condition::Schedule(schedule) => schedule.denoms(deps, env),
            _ => Ok(HashSet::new()),
        }
    }

    fn escrowed(&self, _deps: Deps, _env: &Env) -> StdResult<HashSet<String>> {
        Ok(HashSet::new())
    }

    fn commit(self, deps: Deps, env: &Env) -> StdResult<Condition> {
        match self {
            Condition::Schedule(schedule) => schedule.commit(deps, env),
            _ => Ok(self),
        }
    }

    fn balances(
        &self,
        deps: Deps,
        env: &Env,
        denoms: &HashSet<String>,
    ) -> StdResult<cosmwasm_std::Coins> {
        match self {
            Condition::Schedule(schedule) => schedule.balances(deps, env, denoms),
            _ => Ok(Coins::default()),
        }
    }

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Condition)> {
        if let Condition::Schedule(schedule) = self {
            schedule.withdraw(deps, env, desired)
        } else {
            Ok((vec![], vec![], self))
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<StrategyMsg>, Vec<Event>, Condition)> {
        if let Condition::Schedule(schedule) = self {
            schedule.cancel(deps, env)
        } else {
            Ok((vec![], vec![], self))
        }
    }
}

#[cfg(test)]
mod conditions_tests {
    use super::*;
    use std::str::FromStr;

    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env},
        to_json_binary, Addr, Coin, ContractResult, Decimal, SystemResult, Timestamp, Uint128,
    };
    use rujira_rs::fin::{OrderResponse, Price, Side, SimulationResponse};

    use crate::{
        actions::{
            swaps::fin::FinRoute,
            swaps::swap::{SwapAmountAdjustment, SwapRoute},
        },
        manager::{StrategyHandle, StrategyStatus},
    };

    #[test]
    fn timestamp_elapsed_check() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::TimestampElapsed(env.block.time.minus_seconds(1))
            .is_satisfied(deps.as_ref(), &env)
            .unwrap());

        assert!(!Condition::TimestampElapsed(env.block.time)
            .is_satisfied(deps.as_ref(), &env)
            .unwrap());

        assert!(!Condition::TimestampElapsed(env.block.time.plus_seconds(1))
            .is_satisfied(deps.as_ref(), &env)
            .unwrap());
    }

    #[test]
    fn blocks_completed_check() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::BlocksCompleted(0)
            .is_satisfied(deps.as_ref(), &env)
            .unwrap());
        assert!(Condition::BlocksCompleted(env.block.height - 1)
            .is_satisfied(deps.as_ref(), &env)
            .unwrap());
        assert!(!Condition::BlocksCompleted(env.block.height)
            .is_satisfied(deps.as_ref(), &env)
            .unwrap());
        assert!(!Condition::BlocksCompleted(env.block.height + 1)
            .is_satisfied(deps.as_ref(), &env)
            .unwrap());
    }

    #[test]
    fn balance_available_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(0u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(!Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(1u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![Coin::new(100u128, "rune")],
        );

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(99u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(100u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(!Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(101u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());
    }

    #[test]
    fn can_swap_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier.update_wasm(|_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&SimulationResponse {
                    returned: Uint128::new(100),
                    fee: Uint128::new(1),
                })
                .unwrap(),
            ))
        });

        assert!(!Condition::CanSwap(Swap {
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(101u128, "rune"),
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: Addr::unchecked("fin_pair")
            })],
            maximum_slippage_bps: 100,
            adjustment: SwapAmountAdjustment::Fixed
        })
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(!Condition::CanSwap(Swap {
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(100u128, "rune"),
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: Addr::unchecked("fin_pair")
            })],
            maximum_slippage_bps: 100,
            adjustment: SwapAmountAdjustment::Fixed
        })
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(!Condition::CanSwap(Swap {
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(99u128, "rune"),
            routes: vec![SwapRoute::Fin(FinRoute {
                pair_address: Addr::unchecked("fin_pair")
            })],
            maximum_slippage_bps: 100,
            adjustment: SwapAmountAdjustment::Fixed
        })
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());
    }

    #[test]
    fn limit_order_filled_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&OrderResponse {
                    remaining: Uint128::new(100),
                    filled: Uint128::new(100),
                    owner: "owner".to_string(),
                    side: Side::Base,
                    price: Price::Fixed(Decimal::from_str("1.0").unwrap()),
                    rate: Decimal::from_str("1.0").unwrap(),
                    updated_at: Timestamp::from_seconds(env.block.time.seconds()),
                    offer: Uint128::new(21029),
                })
                .unwrap(),
            ))
        });

        assert!(!Condition::LimitOrderFilled {
            owner: Addr::unchecked("owner"),
            pair_address: Addr::unchecked("pair"),
            side: Side::Base,
            price: Decimal::from_str("1.0").unwrap(),
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&OrderResponse {
                    remaining: Uint128::new(0),
                    filled: Uint128::new(100),
                    owner: "owner".to_string(),
                    side: Side::Base,
                    price: Price::Fixed(Decimal::from_str("1.0").unwrap()),
                    rate: Decimal::from_str("1.0").unwrap(),
                    updated_at: Timestamp::from_seconds(env.block.time.seconds()),
                    offer: Uint128::new(21029),
                })
                .unwrap(),
            ))
        });

        assert!(Condition::LimitOrderFilled {
            owner: Addr::unchecked("owner"),
            pair_address: Addr::unchecked("pair"),
            side: Side::Base,
            price: Decimal::from_str("1.0").unwrap(),
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());
    }

    #[test]
    fn strategy_status_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        deps.querier.update_wasm(move |_| {
            SystemResult::Ok(ContractResult::Ok(
                to_json_binary(&StrategyHandle {
                    id: 1,
                    contract_address: Addr::unchecked("strategy"),
                    status: StrategyStatus::Active,
                    owner: Addr::unchecked("owner"),
                    created_at: 0,
                    updated_at: 0,
                    label: "label".to_string(),
                })
                .unwrap(),
            ))
        });

        let strategy_address = Addr::unchecked("strategy");

        assert!(Condition::StrategyStatus {
            manager_contract: Addr::unchecked("manager"),
            contract_address: strategy_address.clone(),
            status: StrategyStatus::Active,
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(!Condition::StrategyStatus {
            manager_contract: Addr::unchecked("manager"),
            contract_address: strategy_address.clone(),
            status: StrategyStatus::Paused,
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());
    }

    #[test]
    fn not_satisfied_check() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert!(
            !Condition::Not(Box::new(Condition::BlocksCompleted(env.block.height - 1)))
                .is_satisfied(deps.as_ref(), &env)
                .unwrap()
        );
        assert!(
            Condition::Not(Box::new(Condition::BlocksCompleted(env.block.height)))
                .is_satisfied(deps.as_ref(), &env)
                .unwrap()
        );
    }
}
