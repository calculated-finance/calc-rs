use std::vec;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Coin, Deps, Env, Timestamp};
use rujira_rs::fin::{OrderResponse, Price, QueryMsg, Side};

use crate::{
    actions::{
        fin_swap::{get_expected_amount_out as get_expected_amount_out_fin, FinSwap},
        swap::{SwapAmountAdjustment, SwapRoute},
        thor_swap::{get_expected_amount_out as get_expected_amount_out_thorchain, ThorSwap},
    },
    manager::{ManagerQueryMsg, StrategyHandle, StrategyStatus},
};

#[cw_serde]
pub enum Threshold {
    All,
    Any,
}

pub trait Satisfiable {
    fn is_satisfied(&self, deps: Deps, env: &Env) -> bool;
}

#[cw_serde]
pub struct Conditions {
    pub conditions: Vec<Condition>,
    pub threshold: Threshold,
}

impl Satisfiable for Conditions {
    fn is_satisfied(&self, deps: Deps, env: &Env) -> bool {
        match self.threshold {
            Threshold::All => {
                for condition in &self.conditions {
                    if !condition.is_satisfied(deps, env) {
                        return false;
                    }
                }
                true
            }
            Threshold::Any => {
                for condition in &self.conditions {
                    if condition.is_satisfied(deps, env) {
                        return true;
                    }
                }
                false
            }
        }
    }
}

#[cw_serde]
pub enum Condition {
    TimestampElapsed(Timestamp),
    BlocksCompleted(u64),
    CanSwap {
        swap_amount: Coin,
        minimum_receive_amount: Coin,
        route: SwapRoute,
    },
    LimitOrderFilled {
        pair_address: Addr,
        owner: Addr,
        side: Side,
        price: Price,
    },
    BalanceAvailable {
        address: Addr,
        amount: Coin,
    },
    OwnBalanceAvailable {
        amount: Coin,
    },
    StrategyStatus {
        manager_contract: Addr,
        contract_address: Addr,
        status: StrategyStatus,
    },
    Compose(Conditions),
}

impl Satisfiable for Condition {
    fn is_satisfied(&self, deps: Deps, env: &Env) -> bool {
        match self {
            Condition::TimestampElapsed(timestamp) => env.block.time > *timestamp,
            Condition::BlocksCompleted(height) => env.block.height > *height,
            Condition::LimitOrderFilled {
                pair_address,
                owner,
                side,
                price,
            } => {
                let order = deps.querier.query_wasm_smart::<OrderResponse>(
                    pair_address,
                    &QueryMsg::Order((owner.to_string(), side.clone(), price.clone())),
                );

                if let Ok(order) = order {
                    order.remaining.is_zero()
                } else {
                    false
                }
            }
            Condition::CanSwap {
                swap_amount,
                minimum_receive_amount,
                route,
            } => {
                let expected_receive_amount = match route {
                    SwapRoute::Fin(address) => get_expected_amount_out_fin(
                        deps,
                        &FinSwap {
                            swap_amount: swap_amount.clone(),
                            minimum_receive_amount: minimum_receive_amount.clone(),
                            maximum_slippage_bps: 10_000,
                            pair_address: address.clone(),
                            adjustment: SwapAmountAdjustment::Fixed,
                        },
                    ),
                    SwapRoute::Thorchain {
                        streaming_interval,
                        max_streaming_quantity,
                        affiliate_code,
                        affiliate_bps,
                        previous_swap,
                        on_complete,
                        scheduler,
                    } => get_expected_amount_out_thorchain(
                        deps,
                        env,
                        &ThorSwap {
                            swap_amount: swap_amount.clone(),
                            minimum_receive_amount: minimum_receive_amount.clone(),
                            maximum_slippage_bps: 10_000,
                            adjustment: SwapAmountAdjustment::Fixed,
                            streaming_interval: *streaming_interval,
                            max_streaming_quantity: *max_streaming_quantity,
                            affiliate_code: affiliate_code.clone(),
                            affiliate_bps: *affiliate_bps,
                            previous_swap: previous_swap.clone(),
                            on_complete: on_complete.clone(),
                            scheduler: scheduler.clone(),
                        },
                    ),
                };

                if let Ok(expected_receive_amount) = expected_receive_amount {
                    expected_receive_amount.amount >= minimum_receive_amount.amount
                } else {
                    false
                }
            }
            Condition::BalanceAvailable { address, amount } => {
                let balance = deps.querier.query_balance(address, amount.denom.clone());

                if let Ok(balance) = balance {
                    balance.amount >= amount.amount
                } else {
                    false
                }
            }
            Condition::OwnBalanceAvailable { amount } => {
                let balance = deps
                    .querier
                    .query_balance(&env.contract.address, amount.denom.clone());

                if let Ok(balance) = balance {
                    balance.amount >= amount.amount
                } else {
                    false
                }
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
                );

                if let Ok(strategy) = strategy {
                    strategy.status == *status
                } else {
                    false
                }
            }
            Condition::Compose(Conditions {
                conditions,
                threshold,
            }) => match threshold {
                Threshold::All => {
                    for condition in conditions {
                        if !condition.is_satisfied(deps, env) {
                            return false;
                        }
                    }
                    true
                }
                Threshold::Any => {
                    for condition in conditions {
                        if condition.is_satisfied(deps, env) {
                            return true;
                        }
                    }
                    false
                }
            },
        }
    }
}

impl Condition {
    pub fn description(&self, env: &Env) -> String {
        match self {
            Condition::TimestampElapsed(timestamp) => format!("timestamp elapsed: {timestamp}"),
            Condition::BlocksCompleted(height) => format!("blocks completed: {height}"),
            Condition::CanSwap {
                swap_amount,
                minimum_receive_amount,
                ..
            } => format!(
                "can perform swap: swap_amount={swap_amount}, minimum_receive_amount={minimum_receive_amount}"
            ),
            Condition::LimitOrderFilled {
                pair_address,
                owner,
                side,
                price,
            } => format!(
                "limit order filled: pair_address={pair_address}, owner={owner}, side={side:?}, price={price}"
            ),
            Condition::BalanceAvailable { address, amount } => format!(
                "balance available: address={address}, amount={amount}"
            ),
            Condition::OwnBalanceAvailable { amount } => {
                format!("balance available: address={}, amount={}", env.contract.address, amount)
            }
            Condition::StrategyStatus {
                contract_address,
                status,
                ..
            } => format!(
                "strategy ({contract_address}) is in status: {status:?}"
            ),
            Condition::Compose (Conditions { conditions, threshold: operator }) => {
                match operator {
                    Threshold::All => format!(
                        "All the following conditions are met: [\n\t{}\n]",
                        conditions
                            .iter()
                            .map(|c| c.description(env))
                            .collect::<Vec<_>>()
                            .join(",\n\t")
                    ),
                    Threshold::Any => format!(
                        "Any of the following conditions are met: [\n\t{}\n]",
                        conditions
                            .iter()
                            .map(|c| c.description(env))
                            .collect::<Vec<_>>()
                            .join(",\n\t")
                    ),
                }
            }
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

    use crate::{manager::StrategyHandle, manager::StrategyStatus};

    #[test]
    fn timestamp_elapsed_check() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::TimestampElapsed(env.block.time.minus_seconds(1))
            .is_satisfied(deps.as_ref(), &env));

        assert!(!Condition::TimestampElapsed(env.block.time).is_satisfied(deps.as_ref(), &env));

        assert!(!Condition::TimestampElapsed(env.block.time.plus_seconds(1))
            .is_satisfied(deps.as_ref(), &env));
    }

    #[test]
    fn blocks_completed_check() {
        let deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::BlocksCompleted(0).is_satisfied(deps.as_ref(), &env));

        assert!(Condition::BlocksCompleted(env.block.height - 1).is_satisfied(deps.as_ref(), &env));

        assert!(!Condition::BlocksCompleted(env.block.height).is_satisfied(deps.as_ref(), &env));

        assert!(!Condition::BlocksCompleted(env.block.height + 1).is_satisfied(deps.as_ref(), &env));
    }

    #[test]
    fn balance_available_check() {
        let mut deps = mock_dependencies();
        let env = mock_env();

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(0u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env));

        assert!(!Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(1u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env));

        deps.querier.bank.update_balance(
            env.contract.address.clone(),
            vec![Coin::new(100u128, "rune")],
        );

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(99u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env));

        assert!(Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(100u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env));

        assert!(!Condition::BalanceAvailable {
            address: env.contract.address.clone(),
            amount: Coin::new(101u128, "rune"),
        }
        .is_satisfied(deps.as_ref(), &env));
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

        assert!(!Condition::CanSwap {
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(101u128, "rune"),
            route: SwapRoute::Fin(Addr::unchecked("fin_pair")),
        }
        .is_satisfied(deps.as_ref(), &env));

        assert!(Condition::CanSwap {
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(100u128, "rune"),
            route: SwapRoute::Fin(Addr::unchecked("fin_pair")),
        }
        .is_satisfied(deps.as_ref(), &env));

        assert!(Condition::CanSwap {
            swap_amount: Coin::new(100u128, "rune"),
            minimum_receive_amount: Coin::new(99u128, "rune"),
            route: SwapRoute::Fin(Addr::unchecked("fin_pair")),
        }
        .is_satisfied(deps.as_ref(), &env));
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
            price: Price::Fixed(Decimal::from_str("1.0").unwrap()),
        }
        .is_satisfied(deps.as_ref(), &env));

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
            price: Price::Fixed(Decimal::from_str("1.0").unwrap()),
        }
        .is_satisfied(deps.as_ref(), &env));
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
                    affiliates: vec![],
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
        .is_satisfied(deps.as_ref(), &env));

        assert!(!Condition::StrategyStatus {
            manager_contract: Addr::unchecked("manager"),
            contract_address: strategy_address.clone(),
            status: StrategyStatus::Paused,
        }
        .is_satisfied(deps.as_ref(), &env));
    }
}
