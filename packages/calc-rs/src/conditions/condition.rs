use std::{collections::HashSet, vec};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    Addr, Coin, Coins, CosmosMsg, Decimal, Deps, Env, StdError, StdResult, Timestamp,
};
use rujira_rs::{
    fin::{ConfigResponse, OrderResponse, Price, QueryMsg, Side},
    query::Pool,
    Layer1Asset,
};

use crate::{
    actions::{
        limit_orders::fin_limit_order::{Direction, FinLimitOrder, PriceStrategy},
        swaps::swap::Swap,
    },
    cadence::Cadence,
    conditions::schedule::Schedule,
    manager::{Affiliate, ManagerQueryMsg, Strategy, StrategyStatus},
    operation::{Operation, StatefulOperation},
};

#[cw_serde]
pub enum Condition {
    TimestampElapsed(Timestamp),
    BlocksCompleted(u64),
    Schedule(Schedule),
    CanSwap(Swap),
    FinLimitOrderFilled {
        owner: Option<Addr>,
        pair_address: Addr,
        side: Side,
        price: Decimal,
    },
    BalanceAvailable {
        address: Option<Addr>,
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
        price: Decimal,
    },
}

impl Condition {
    pub fn size(&self) -> usize {
        match self {
            Condition::TimestampElapsed(_) => 1,
            Condition::BlocksCompleted(_) => 1,
            Condition::Schedule(schedule) => match schedule.cadence {
                Cadence::LimitOrder { .. } => 4,
                _ => 2,
            },
            Condition::CanSwap { .. } => 2,
            Condition::FinLimitOrderFilled { .. } => 2,
            Condition::BalanceAvailable { .. } => 1,
            Condition::StrategyStatus { .. } => 2,
            Condition::OraclePrice { .. } => 2,
        }
    }

    pub fn is_satisfied(&self, deps: Deps, env: &Env) -> StdResult<bool> {
        Ok(match self {
            Condition::TimestampElapsed(timestamp) => env.block.time >= *timestamp,
            Condition::BlocksCompleted(height) => env.block.height >= *height,
            Condition::Schedule(schedule) => {
                schedule
                    .cadence
                    .is_due(deps, env, &schedule.scheduler_address)?
            }
            Condition::FinLimitOrderFilled {
                owner,
                pair_address,
                side,
                price,
            } => {
                let order = deps.querier.query_wasm_smart::<OrderResponse>(
                    pair_address,
                    &QueryMsg::Order((
                        owner.as_ref().unwrap_or(&env.contract.address).to_string(),
                        side.clone(),
                        Price::Fixed(*price),
                    )),
                )?;

                order.remaining.is_zero()
            }
            Condition::CanSwap(swap) => swap.best_quote(deps, env)?.is_some(),
            Condition::BalanceAvailable { address, amount } => {
                let balance = deps.querier.query_balance(
                    address.as_ref().unwrap_or(&env.contract.address),
                    amount.denom.clone(),
                )?;
                balance.amount >= amount.amount
            }
            Condition::StrategyStatus {
                manager_contract,
                contract_address,
                status,
            } => {
                let strategy = deps.querier.query_wasm_smart::<Strategy>(
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
                price,
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
                    Direction::Above => oracle_price > *price,
                    Direction::Below => oracle_price < *price,
                }
            }
        })
    }
}

impl Operation<Condition> for Condition {
    fn init(self, deps: Deps, env: &Env, affiliates: &[Affiliate]) -> StdResult<Condition> {
        match self {
            Condition::Schedule(schedule) => schedule.init(deps, env, affiliates),
            Condition::BalanceAvailable { ref address, .. } => {
                if let Some(address) = address {
                    deps.api.addr_validate(address.as_str()).map_err(|_| {
                        StdError::generic_err(format!(
                            "Invalid address to check for balance: {}",
                            address
                        ))
                    })?;
                }

                Ok(self)
            }
            Condition::CanSwap(ref swap) => {
                swap.clone().init(deps, env, affiliates)?;
                Ok(self)
            }
            Condition::StrategyStatus {
                ref manager_contract,
                ref contract_address,
                ..
            } => {
                deps.querier
                    .query_wasm_smart::<Strategy>(
                        manager_contract,
                        &ManagerQueryMsg::Strategy {
                            address: contract_address.clone(),
                        },
                    )
                    .map_err(|e| {
                        StdError::generic_err(format!(
                            "Failed to query strategy status for {}: {}",
                            contract_address, e
                        ))
                    })?;

                Ok(self)
            }
            Condition::FinLimitOrderFilled {
                ref pair_address,
                ref side,
                price,
                ..
            } => {
                let pair = deps
                    .querier
                    .query_wasm_smart::<ConfigResponse>(pair_address, &QueryMsg::Config {})?;

                let limit_order = FinLimitOrder {
                    pair_address: pair_address.clone(),
                    side: side.clone(),
                    bid_amount: None,
                    bid_denom: if side == &Side::Base {
                        pair.denoms.base()
                    } else {
                        pair.denoms.quote()
                    }
                    .to_string(),
                    strategy: PriceStrategy::Fixed(price),
                    current_order: None,
                };

                limit_order.init(deps, env, &[])?;

                Ok(self)
            }
            Condition::OraclePrice { ref asset, .. } => {
                Layer1Asset::from_native(asset.clone()).map_err(|e| {
                    StdError::generic_err(format!(
                        "Denom ({asset}) not a secured asset, error: {e}"
                    ))
                })?;

                Ok(self)
            }
            Condition::BlocksCompleted(_) | Condition::TimestampElapsed(_) => Ok(self),
        }
    }

    fn execute(self, deps: Deps, env: &Env) -> (Vec<CosmosMsg>, Condition) {
        match self {
            Condition::Schedule(schedule) => schedule.execute(deps, env),
            _ => (vec![], self),
        }
    }

    fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        match self {
            Condition::Schedule(schedule) => schedule.denoms(deps, env),
            _ => Ok(HashSet::new()),
        }
    }
}

// 479
// 541

impl StatefulOperation<Condition> for Condition {
    fn commit(self, deps: Deps, env: &Env) -> StdResult<Condition> {
        match self {
            Condition::Schedule(schedule) => schedule.commit(deps, env),
            _ => Ok(self),
        }
    }

    fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        match self {
            Condition::Schedule(schedule) => schedule.balances(deps, env, denoms),
            _ => Ok(Coins::default()),
        }
    }

    fn withdraw(
        self,
        deps: Deps,
        env: &Env,
        denoms: &HashSet<String>,
    ) -> StdResult<(Vec<CosmosMsg>, Condition)> {
        match self {
            Condition::Schedule(schedule) => schedule.withdraw(deps, env, denoms),
            _ => Ok((vec![], self)),
        }
    }

    fn cancel(self, deps: Deps, env: &Env) -> StdResult<(Vec<CosmosMsg>, Condition)> {
        match self {
            Condition::Schedule(schedule) => schedule.cancel(deps, env),
            _ => Ok((vec![], self)),
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
        manager::{Strategy, StrategyStatus},
    };

    #[test]
    fn timestamp_elapsed_check() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::TimestampElapsed(env.block.time.minus_seconds(1))
            .is_satisfied(deps.as_ref(), &env)
            .unwrap());

        assert!(Condition::TimestampElapsed(env.block.time)
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
        assert!(Condition::BlocksCompleted(env.block.height)
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
            address: None,
            amount: Coin::new(0u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(!Condition::BalanceAvailable {
            address: None,
            amount: Coin::new(1u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![Coin::new(100u128, "rune")],
        );

        assert!(Condition::BalanceAvailable {
            address: None,
            amount: Coin::new(99u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(Condition::BalanceAvailable {
            address: None,
            amount: Coin::new(100u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env)
        .unwrap());

        assert!(!Condition::BalanceAvailable {
            address: None,
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

        assert!(!Condition::FinLimitOrderFilled {
            owner: None,
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

        assert!(Condition::FinLimitOrderFilled {
            owner: None,
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
                to_json_binary(&Strategy {
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
}
