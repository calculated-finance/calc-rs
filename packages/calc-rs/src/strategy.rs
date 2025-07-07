use std::{collections::HashSet, hash::Hasher, vec};

use ahash::AHasher;
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    instantiate2_address, to_json_binary, Addr, Binary, Coin, Coins, Decimal, Deps, DepsMut, Env,
    Event, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg,
};

use crate::{
    actions::{
        action::Action,
        distribution::{Destination, Distribution, Recipient},
        operation::Operation,
        schedule::Schedule,
        swap::{OptimalSwap, SwapRoute},
        thor_swap::ThorSwap,
    },
    manager::{Affiliate, StrategyStatus},
    statistics::Statistics,
};

#[cw_serde]
pub struct StrategyConfig {
    pub manager: Addr,
    pub strategy: Strategy<Idle>,
    pub escrowed: HashSet<String>,
}

#[cw_serde]
pub struct StrategyInstantiateMsg(pub Strategy<Instantiable>);

#[cw_serde]
pub enum StrategyExecuteMsg {
    Execute {},
    Withdraw(HashSet<String>),
    Update(Strategy<Instantiable>),
    UpdateStatus(StrategyStatus),
    Clear {},
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum StrategyQueryMsg {
    #[returns(StrategyConfig)]
    Config {},
    #[returns(Statistics)]
    Statistics {},
    #[returns(Vec<Coin>)]
    Balances(HashSet<String>),
}

#[cw_serde]
pub struct Strategy<S> {
    pub owner: Addr,
    pub action: Action,
    pub state: S,
}

impl<S> Strategy<S> {
    pub fn size(&self) -> usize {
        self.action.size()
    }

    pub fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        self.action.escrowed(deps, env)
    }

    pub fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        self.action.balances(deps, env, denoms)
    }
}

#[cw_serde]
pub struct Json;

#[cw_serde]
pub struct New {
    code_id: u64,
    creator: Addr,
    label: String,
}

#[cw_serde]
pub struct Instantiable {
    pub contract_address: Addr,
    salt: Binary,
    code_id: u64,
    label: String,
}

#[cw_serde]
pub struct Idle {
    pub contract_address: Addr,
}

#[cw_serde]
pub struct Executable {
    pub contract_address: Addr,
    messages: Vec<SubMsg>,
    events: Vec<Event>,
}

impl Strategy<Json> {
    pub fn to_new(self, code_id: u64, creator: Addr, label: String) -> Strategy<New> {
        Strategy {
            owner: self.owner,
            action: self.action,
            state: New {
                code_id,
                creator,
                label,
            },
        }
    }
}

impl Strategy<New> {
    // pub fn new(code_id: u64, creator: Addr, owner: Addr, action: Action) -> Self {
    //     Self {
    //         owner,
    //         action,
    //         state: New { code_id, creator, label },
    //     }
    // }

    pub fn with_affiliates(
        self,
        deps: Deps,
        affiliates: &Vec<Affiliate>,
    ) -> StdResult<Strategy<Instantiable>> {
        let action_with_affiliates = Self::add_affiliates(deps, self.action.clone(), affiliates)?;
        let salt_data = to_json_binary(&(self.owner.to_string(), action_with_affiliates.clone()))?;

        let mut hash = AHasher::default();
        hash.write(salt_data.as_slice());
        let salt = hash.finish().to_le_bytes();

        let contract_address = deps.api.addr_humanize(
            &instantiate2_address(
                deps.querier
                    .query_wasm_code_info(self.state.code_id)?
                    .checksum
                    .as_slice(),
                &deps.api.addr_canonicalize(self.state.creator.as_str())?,
                &salt,
            )
            .map_err(|e| {
                StdError::generic_err(format!("Failed to instantiate contract address: {}", e))
            })?,
        )?;

        Ok(Strategy {
            owner: self.owner,
            action: action_with_affiliates,
            state: Instantiable {
                code_id: self.state.code_id,
                salt: Binary::from(salt),
                label: self.state.label,
                contract_address,
            },
        })
    }

    fn add_affiliates(
        deps: Deps,
        action: Action,
        affiliates: &Vec<Affiliate>,
    ) -> StdResult<Action> {
        Ok(match action {
            Action::Distribute(Distribution {
                denoms,
                destinations,
            }) => {
                let total_affiliate_bps = affiliates
                    .iter()
                    .fold(0, |acc, affiliate| acc + affiliate.bps);

                let total_shares = destinations
                    .iter()
                    .filter(|d| match d.recipient {
                        Recipient::Bank { .. }
                        | Recipient::Wasm { .. }
                        | Recipient::Deposit { .. } => true,
                        // We don't take fees on transfers between strategies
                        Recipient::Strategy { .. } => false,
                    })
                    .fold(Uint128::zero(), |acc, d| acc + d.shares);

                let total_shares_with_fees =
                    total_shares.mul_ceil(Decimal::bps(10_000 + total_affiliate_bps));

                Action::Distribute(Distribution {
                    denoms: denoms.clone(),
                    destinations: [
                        destinations.clone(),
                        affiliates
                            .iter()
                            .map(|affiliate| Destination {
                                recipient: Recipient::Bank {
                                    address: affiliate.address.clone(),
                                },
                                shares: total_shares_with_fees
                                    .mul_floor(Decimal::bps(affiliate.bps)),
                                label: Some(affiliate.label.clone()),
                            })
                            .collect::<Vec<_>>(),
                    ]
                    .concat(),
                })
            }
            Action::ThorSwap(thor_swap) => Action::ThorSwap(ThorSwap {
                // As per agreement with Rujira
                affiliate_code: Some("rj".to_string()),
                affiliate_bps: Some(10),
                ..thor_swap
            }),
            Action::OptimalSwap(swap) => {
                let routes_with_affiliates = swap
                    .routes
                    .into_iter()
                    .map(|route| match route {
                        SwapRoute::Thorchain {
                            streaming_interval,
                            max_streaming_quantity,
                            previous_swap,
                            on_complete,
                            scheduler,
                            affiliate_code: _,
                            affiliate_bps: _,
                        } => SwapRoute::Thorchain {
                            streaming_interval,
                            max_streaming_quantity,
                            previous_swap,
                            on_complete,
                            scheduler,
                            // As per agreement with Rujira
                            affiliate_code: Some("rj".to_string()),
                            affiliate_bps: Some(10),
                        },
                        _ => route,
                    })
                    .collect::<Vec<_>>();

                Action::OptimalSwap(OptimalSwap {
                    routes: routes_with_affiliates,
                    ..swap
                })
            }
            Action::Schedule(schedule) => Action::Schedule(Schedule {
                action: Box::new(Self::add_affiliates(deps, *schedule.action, affiliates)?),
                ..schedule
            }),
            Action::Conditional((conditions, action)) => Action::Conditional((
                conditions,
                Box::new(Self::add_affiliates(deps, *action, affiliates)?),
            )),
            Action::Many(actions) => {
                let mut initialised_actions = vec![];

                for action in actions {
                    initialised_actions.push(Self::add_affiliates(deps, action, affiliates)?);
                }

                Action::Many(initialised_actions)
            }
            _ => action.clone(),
        })
    }
}

impl Strategy<Instantiable> {
    pub fn instantiate_msg(&self) -> StdResult<WasmMsg> {
        Ok(WasmMsg::Instantiate2 {
            admin: Some(self.owner.to_string()),
            code_id: self.state.code_id,
            label: self.state.label.clone(),
            salt: self.state.salt.clone(),
            msg: to_json_binary(&StrategyInstantiateMsg(self.clone()))?,
            funds: vec![],
        })
    }

    pub fn init<F>(self, deps: &mut DepsMut, env: &Env, save: F) -> StdResult<Response>
    where
        F: FnOnce(&mut dyn Storage, Strategy<Idle>) -> StdResult<()>,
    {
        let (action, messages, events) = self.action.init(deps.as_ref(), env)?;

        save(
            deps.storage,
            Strategy {
                owner: self.owner,
                action,
                state: Idle {
                    contract_address: self.state.contract_address.clone(),
                },
            },
        )?;

        Ok(Response::default()
            .add_submessages(messages)
            .add_events(events))
    }
}

impl Strategy<Idle> {
    pub fn prepare_to_execute(self, deps: Deps, env: &Env) -> StdResult<Strategy<Executable>> {
        let (action, messages, events) = self.action.execute(deps, env)?;

        Ok(Strategy {
            owner: self.owner,
            action,
            state: Executable {
                contract_address: self.state.contract_address.clone(),
                messages,
                events,
            },
        })
    }

    pub fn prepare_to_withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &HashSet<String>,
    ) -> StdResult<Strategy<Executable>> {
        let (action, messages, events) = self.action.withdraw(deps, env, desired)?;

        Ok(Strategy {
            owner: self.owner,
            action,
            state: Executable {
                contract_address: self.state.contract_address.clone(),
                messages,
                events,
            },
        })
    }

    pub fn prepare_to_cancel(self, deps: Deps, env: &Env) -> StdResult<Strategy<Executable>> {
        let (action, messages, events) = self.action.cancel(deps, env)?;

        Ok(Strategy {
            owner: self.owner,
            action,
            state: Executable {
                contract_address: self.state.contract_address.clone(),
                messages,
                events,
            },
        })
    }
}

impl Strategy<Executable> {
    pub fn execute<F>(self, deps: &mut DepsMut, save: F) -> StdResult<Response>
    where
        F: FnOnce(&mut dyn Storage, Strategy<Idle>) -> StdResult<()>,
    {
        save(
            deps.storage,
            Strategy {
                owner: self.owner,
                action: self.action,
                state: Idle {
                    contract_address: self.state.contract_address.clone(),
                },
            },
        )?;

        Ok(Response::default()
            .add_submessages(self.state.messages)
            .add_events(self.state.events))
    }
}
