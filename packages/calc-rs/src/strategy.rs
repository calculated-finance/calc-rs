use std::{collections::HashSet, vec};

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    Addr, BankMsg, Coin, Coins, Decimal, Deps, DepsMut, Env, Event, Response, StdResult, Storage,
    SubMsg, Uint128,
};

use crate::{
    actions::{
        action::Action,
        distribution::{Destination, Distribution, Recipient},
        operation::Operation,
        schedule::Schedule,
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
pub struct StrategyInstantiateMsg(pub Strategy<WithAffiliates>);

#[cw_serde]
pub enum StrategyExecuteMsg {
    Execute {},
    Withdraw(Vec<Coin>),
    Update(Strategy<WithAffiliates>),
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
    Balances { include: Vec<String> },
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

    pub fn balances(&self, deps: Deps, env: &Env, denoms: &[String]) -> StdResult<Coins> {
        self.action.balances(deps, env, denoms)
    }
}

#[cw_serde]
pub struct New;

#[cw_serde]
pub struct WithAffiliates;

#[cw_serde]
pub struct Idle;

#[cw_serde]
pub struct Executable {
    messages: Vec<SubMsg>,
    events: Vec<Event>,
}

impl Strategy<New> {
    pub fn new(owner: Addr, action: Action) -> Self {
        Self {
            owner,
            action,
            state: New,
        }
    }

    pub fn with_affiliates(self, affiliates: &Vec<Affiliate>) -> Strategy<WithAffiliates> {
        let action_with_affiliates = Self::add_affiliates(self.action, affiliates);

        Strategy {
            owner: self.owner,
            action: action_with_affiliates,
            state: WithAffiliates,
        }
    }

    fn add_affiliates(action: Action, affiliates: &Vec<Affiliate>) -> Action {
        match action {
            Action::Distribute(Distribution {
                denoms,
                mutable_destinations,
                immutable_destinations,
            }) => {
                let total_affiliate_bps = affiliates
                    .iter()
                    .fold(0, |acc, affiliate| acc + affiliate.bps);

                let total_shares = mutable_destinations
                    .iter()
                    .chain(immutable_destinations.iter())
                    .fold(Uint128::zero(), |acc, d| acc + d.shares);

                let total_shares_with_fees =
                    total_shares.mul_ceil(Decimal::bps(10_000 + total_affiliate_bps));

                Action::Distribute(Distribution {
                    denoms: denoms.clone(),
                    mutable_destinations: mutable_destinations.clone(),
                    immutable_destinations: [
                        immutable_destinations.clone(),
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
            Action::Schedule(schedule) => Action::Schedule(Schedule {
                action: Box::new(Self::add_affiliates(*schedule.action, affiliates)),
                ..schedule
            }),
            Action::Conditional((conditions, action)) => Action::Conditional((
                conditions,
                Box::new(Self::add_affiliates(*action, affiliates)),
            )),
            Action::Many(actions) => {
                let mut initialised_actions = vec![];

                for action in actions {
                    initialised_actions.push(Self::add_affiliates(action, affiliates));
                }

                Action::Many(initialised_actions)
            }
            _ => action.clone(),
        }
    }
}

impl Strategy<WithAffiliates> {
    pub fn init<F>(self, deps: &mut DepsMut, env: &Env, save: F) -> StdResult<Response>
    where
        F: FnOnce(&mut dyn Storage, Strategy<Idle>) -> StdResult<()>,
    {
        let action = self.action.init(deps.as_ref(), env)?;

        save(
            deps.storage,
            Strategy {
                owner: self.owner,
                action,
                state: Idle,
            },
        )?;

        Ok(Response::default()) // TODO: Add init messages?
    }
}

impl Strategy<Idle> {
    pub fn prepare_to_execute(self, deps: Deps, env: &Env) -> StdResult<Strategy<Executable>> {
        let (action, messages, events) = self.action.execute(deps, env)?;

        Ok(Strategy {
            owner: self.owner,
            action,
            state: Executable { messages, events },
        })
    }

    pub fn prepare_to_withdraw(
        self,
        deps: Deps,
        env: &Env,
        desired: &Coins,
    ) -> StdResult<Strategy<Executable>> {
        let (mut messages, withdrawals) = self.action.withdraw(deps, env, desired)?;

        // Add message to send withdrawn funds to the
        // owner once withdrawal messages have executed
        messages.push(SubMsg::reply_never(BankMsg::Send {
            to_address: self.owner.to_string(),
            amount: withdrawals.to_vec(),
        }));

        Ok(Strategy {
            owner: self.owner,
            action: self.action,
            state: Executable {
                messages,
                events: vec![],
            },
        })
    }

    pub fn prepare_to_cancel(self, deps: Deps, env: &Env) -> StdResult<Strategy<Executable>> {
        let (action, messages, events) = self.action.cancel(deps, env)?;

        Ok(Strategy {
            owner: self.owner,
            action,
            state: Executable { messages, events },
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
                state: Idle,
            },
        )?;

        Ok(Response::default()
            .add_submessages(self.state.messages)
            .add_events(self.state.events))
    }
}
