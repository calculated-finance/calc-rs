use std::{collections::HashSet, hash::Hasher, vec};

use ahash::AHasher;
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    instantiate2_address, to_json_binary, Addr, Binary, Coin, Coins, CosmosMsg, Decimal, Deps,
    DepsMut, Env, Event, MessageInfo, Response, StdError, StdResult, Storage, SubMsg, Uint128,
    WasmMsg,
};

use crate::{
    actions::{
        action::Action,
        conditional::Conditional,
        distribution::{Destination, Distribution, Recipient},
        operation::{StatefulOperation, StatelessOperation},
        schedule::Schedule,
        swap::{Swap, SwapRoute, ThorchainRoute},
    },
    constants::{LOG_ERRORS_REPLY_ID, MAX_STRATEGY_SIZE, PROCESS_PAYLOAD_REPLY_ID},
    core::Contract,
    manager::{Affiliate, StrategyStatus},
    statistics::Statistics,
};

#[cw_serde]
pub struct StrategyConfig {
    pub manager: Addr,
    pub strategy: Strategy<Committed>,
    pub escrowed: HashSet<String>,
}

pub type StrategyInstantiateMsg = Strategy<Indexed>;

#[cw_serde]
pub enum StrategyExecuteMsg {
    Execute,
    Withdraw(HashSet<String>),
    Update(Strategy<Indexed>),
    UpdateStatus(StrategyStatus),
    Commit,
    Clear,
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
#[derive(Default)]
pub struct StrategyMsgPayload {
    pub statistics: Statistics,
    pub events: Vec<Event>,
}

impl StrategyMsgPayload {
    pub fn decorated_events(&self, decorator: &str) -> Vec<Event> {
        self.events
            .clone()
            .into_iter()
            .map(|mut event| {
                event.ty = format!("calc_event:{}_{}", event.ty, decorator);
                event
            })
            .collect()
    }
}

#[cw_serde]
pub struct StrategyMsg {
    msg: CosmosMsg,
    payload: StrategyMsgPayload,
}

impl StrategyMsg {
    pub fn with_payload(msg: CosmosMsg, payload: StrategyMsgPayload) -> Self {
        Self { msg, payload }
    }

    pub fn without_payload(msg: CosmosMsg) -> Self {
        Self {
            msg,
            payload: StrategyMsgPayload::default(),
        }
    }
}

impl From<StrategyMsg> for SubMsg {
    fn from(msg: StrategyMsg) -> Self {
        SubMsg::reply_always(msg.msg, PROCESS_PAYLOAD_REPLY_ID)
            .with_payload(to_json_binary(&msg.payload).unwrap())
    }
}

#[cw_serde]
pub struct Json;

#[cw_serde]
pub struct New;

#[cw_serde]
pub struct Indexable;

pub struct Instantiable {
    pub contract_address: Addr,
    label: String,
    salt: Binary,
    code_id: u64,
}

pub struct Updatable {
    pub contract_address: Addr,
}

#[cw_serde]
pub struct Indexed {
    pub contract_address: Addr,
}

#[cw_serde]
pub struct Active {
    pub contract_address: Addr,
}

#[cw_serde]
pub struct Executable {
    pub contract_address: Addr,
    messages: Vec<StrategyMsg>,
    events: Vec<Event>,
}

#[cw_serde]
pub struct Committable {
    pub contract_address: Addr,
    messages: Vec<StrategyMsg>,
    events: Vec<Event>,
}

#[cw_serde]
pub struct Committed {
    pub contract_address: Addr,
}

impl Strategy<Json> {
    pub fn with_affiliates(
        self,
        deps: Deps,
        affiliates: &Vec<Affiliate>,
    ) -> StdResult<Strategy<Indexable>> {
        let action_with_affiliates = Self::add_affiliates(deps, self.action.clone(), affiliates)?;

        Ok(Strategy {
            owner: self.owner,
            action: action_with_affiliates,
            state: Indexable,
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

                let mut total_fee_applied_shares = Uint128::zero();
                let mut total_fee_exempt_shares = Uint128::zero();

                for destination in destinations.iter() {
                    match &destination.recipient {
                        Recipient::Bank { .. }
                        | Recipient::Contract { .. }
                        | Recipient::Deposit { .. } => {
                            total_fee_applied_shares += destination.shares;
                        }
                        Recipient::Strategy { .. } => {
                            total_fee_exempt_shares += destination.shares;
                        }
                    }
                }

                let total_fee_shares =
                    total_fee_applied_shares.mul_ceil(Decimal::bps(total_affiliate_bps));

                if total_fee_shares.is_zero() {
                    Action::Distribute(Distribution {
                        denoms: denoms.clone(),
                        destinations: destinations,
                    })
                } else {
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
                                    shares: total_fee_applied_shares
                                        .mul_ceil(Decimal::bps(affiliate.bps)),
                                    label: Some(affiliate.label.clone()),
                                })
                                .collect(),
                        ]
                        .concat(),
                    })
                }
            }
            Action::Swap(swap) => Action::Swap(Swap {
                routes: swap
                    .routes
                    .into_iter()
                    .map(|route| match route {
                        SwapRoute::Thorchain(thor_route) => SwapRoute::Thorchain(ThorchainRoute {
                            affiliate_code: Some("rj".to_string()),
                            affiliate_bps: Some(10),
                            ..thor_route
                        }),
                        _ => route,
                    })
                    .collect(),
                ..swap
            }),
            Action::Schedule(schedule) => Action::Schedule(Schedule {
                action: Box::new(Self::add_affiliates(deps, *schedule.action, affiliates)?),
                ..schedule
            }),
            Action::Conditional(conditional) => Action::Conditional(Conditional {
                action: Box::new(Self::add_affiliates(deps, *conditional.action, affiliates)?),
                ..conditional
            }),
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

impl Strategy<Indexable> {
    pub fn add_to_index<F>(
        self,
        deps: &mut DepsMut,
        env: &Env,
        code_id: u64,
        label: String,
        save: F,
    ) -> StdResult<Strategy<Instantiable>>
    where
        F: FnOnce(&mut dyn Storage, &Strategy<Instantiable>) -> StdResult<()>,
    {
        let salt_data = to_json_binary(&(self.owner.to_string(), self.clone()))?;
        let mut hash = AHasher::default();
        hash.write(salt_data.as_slice());
        let salt = hash.finish().to_le_bytes();

        let contract_address = deps.api.addr_humanize(
            &instantiate2_address(
                deps.querier
                    .query_wasm_code_info(code_id)?
                    .checksum
                    .as_slice(),
                &deps.api.addr_canonicalize(env.contract.address.as_str())?,
                &salt,
            )
            .map_err(|e| {
                StdError::generic_err(format!("Failed to instantiate contract address: {e}"))
            })?,
        )?;

        let instantiable_strategy = Strategy {
            owner: self.owner.clone(),
            action: self.action.clone(),
            state: Instantiable {
                contract_address,
                label,
                salt: Binary::from(salt),
                code_id,
            },
        };

        save(deps.storage, &instantiable_strategy)?;

        Ok(instantiable_strategy)
    }

    pub fn update_index<F>(
        self,
        deps: &mut DepsMut,
        contract_address: Addr,
        save: F,
    ) -> StdResult<Strategy<Updatable>>
    where
        F: FnOnce(&mut dyn Storage) -> StdResult<()>,
    {
        let indexed_strategy = Strategy {
            owner: self.owner.clone(),
            action: self.action.clone(),
            state: Updatable { contract_address },
        };

        save(deps.storage)?;

        Ok(indexed_strategy)
    }
}

impl Strategy<Instantiable> {
    pub fn instantiate_msg(self, info: MessageInfo) -> StdResult<CosmosMsg> {
        Ok(WasmMsg::Instantiate2 {
            admin: Some(self.owner.to_string()),
            code_id: self.state.code_id,
            label: self.state.label,
            salt: self.state.salt,
            msg: to_json_binary(&StrategyInstantiateMsg {
                owner: self.owner,
                action: self.action,
                state: Indexed {
                    contract_address: self.state.contract_address.clone(),
                },
            })?,
            funds: info.funds,
        }
        .into())
    }
}

impl Strategy<Updatable> {
    pub fn update_msg(self, info: MessageInfo) -> StdResult<CosmosMsg> {
        Ok(Contract(self.state.contract_address.clone()).call(
            to_json_binary(&StrategyExecuteMsg::Update(Strategy {
                owner: self.owner,
                action: self.action,
                state: Indexed {
                    contract_address: self.state.contract_address,
                },
            }))?,
            info.funds,
        ))
    }
}

impl Strategy<Indexed> {
    pub fn init<F>(self, deps: &mut DepsMut, env: &Env, save: F) -> StdResult<Response>
    where
        F: FnOnce(&mut dyn Storage, Strategy<Committed>) -> StdResult<()>,
    {
        if self.size() > MAX_STRATEGY_SIZE {
            println!("Strategy size: {}, max: {}", self.size(), MAX_STRATEGY_SIZE);
            return Err(StdError::generic_err(format!(
                "Strategy size exceeds maximum limit of {MAX_STRATEGY_SIZE} actions"
            )));
        }

        let (messages, events, action) = self.action.init(deps.as_ref(), env)?;

        save(
            deps.storage,
            Strategy {
                owner: self.owner,
                action,
                state: Committed {
                    contract_address: self.state.contract_address.clone(),
                },
            },
        )?;

        Ok(Response::default()
            .add_submessages(messages.into_iter().map(SubMsg::from).collect::<Vec<_>>())
            .add_events(events))
    }
}

impl Strategy<Committed> {
    pub fn activate(self) -> Strategy<Active> {
        Strategy {
            owner: self.owner,
            action: self.action,
            state: Active {
                contract_address: self.state.contract_address,
            },
        }
    }
}

impl Strategy<Active> {
    pub fn prepare_to_execute(self, deps: Deps, env: &Env) -> StdResult<Strategy<Executable>> {
        let (messages, events, action) = self.action.execute(deps, env);

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
        let (messages, events, action) = self.action.withdraw(deps, env, desired)?;

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
        let (messages, events, action) = self.action.cancel(deps, env)?;

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

    pub fn prepare_to_commit(self, deps: Deps, env: &Env) -> StdResult<Strategy<Committable>> {
        let (messages, events, action) = self.action.commit(deps, env)?;

        Ok(Strategy {
            owner: self.owner,
            action,
            state: Committable {
                contract_address: self.state.contract_address.clone(),
                messages,
                events,
            },
        })
    }
}

impl Strategy<Committable> {
    pub fn commit<F>(self, deps: &mut DepsMut, save: F) -> StdResult<Response>
    where
        F: FnOnce(&mut dyn Storage, Strategy<Committed>) -> StdResult<()>,
    {
        save(
            deps.storage,
            Strategy {
                owner: self.owner,
                action: self.action,
                state: Committed {
                    contract_address: self.state.contract_address.clone(),
                },
            },
        )?;

        Ok(Response::default()
            .add_submessages(
                self.state
                    .messages
                    .into_iter()
                    .map(SubMsg::from)
                    .collect::<Vec<_>>(),
            )
            .add_events(self.state.events))
    }
}

impl Strategy<Executable> {
    pub fn execute<F>(self, deps: &mut DepsMut, env: &Env, save: F) -> StdResult<Response>
    where
        F: FnOnce(&mut dyn Storage, Strategy<Active>) -> StdResult<()>,
    {
        save(
            deps.storage,
            Strategy {
                owner: self.owner,
                action: self.action,
                state: Active {
                    contract_address: self.state.contract_address.clone(),
                },
            },
        )?;

        if self.state.messages.is_empty() {
            return Ok(Response::default().add_events(self.state.events));
        }

        let commit_message = SubMsg::reply_always(
            Contract(env.contract.address.clone())
                .call(to_json_binary(&StrategyExecuteMsg::Commit)?, vec![]),
            LOG_ERRORS_REPLY_ID,
        );

        Ok(Response::default()
            .add_submessages(
                self.state
                    .messages
                    .into_iter()
                    .map(SubMsg::from)
                    .collect::<Vec<_>>(),
            )
            .add_submessage(commit_message)
            .add_events(self.state.events))
    }
}
