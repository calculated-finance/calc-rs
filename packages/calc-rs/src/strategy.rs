use std::{
    collections::HashSet,
    hash::{DefaultHasher, Hasher},
    vec,
};

use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    instantiate2_address, to_json_binary, Addr, Binary, Coin, Coins, CosmosMsg, Deps, DepsMut, Env,
    Event, MessageInfo, StdError, StdResult, Storage, SubMsg, WasmMsg,
};

use crate::{
    actions::{action::Action, operation::Operation},
    condition::Condition,
    constants::PROCESS_PAYLOAD_REPLY_ID,
    core::Contract,
    manager::{Affiliate, StrategyStatus},
    statistics::Statistics,
};

#[cw_serde]
pub struct StrategyConfig {
    pub manager: Addr,
    pub strategy: Strategy<Indexed>,
    pub denoms: HashSet<String>,
    pub escrowed: HashSet<String>,
}

#[cw_serde]
pub enum StrategyOperation {
    Init,
    Execute,
    Withdraw(HashSet<String>),
    Cancel,
}

#[cw_serde]
pub enum StrategyExecuteMsg {
    Execute,
    Withdraw {
        denoms: HashSet<String>,
        from_actions: bool,
    },
    Update(Strategy<Indexed>),
    UpdateStatus(StrategyStatus),
    Process {
        operation: StrategyOperation,
        strategy: Strategy<Indexed>,
    },
    ProcessNext {
        operation: StrategyOperation,
        previous: Option<OpNode>,
    },
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
    pub actions: Vec<Action>,
    pub state: S,
}

impl<S> Strategy<S> {
    pub fn size(&self) -> usize {
        self.actions.iter().map(|a| a.size()).sum::<usize>() + 1
    }

    pub fn denoms(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        let mut denoms = HashSet::new();

        for action in self.actions.iter() {
            let action_denoms = action.denoms(deps, env)?;
            denoms.extend(action_denoms);
        }

        Ok(denoms)
    }

    pub fn escrowed(&self, deps: Deps, env: &Env) -> StdResult<HashSet<String>> {
        let mut escrowed = HashSet::new();

        for action in self.actions.iter() {
            let action_escrowed = action.escrowed(deps, env)?;
            escrowed.extend(action_escrowed);
        }

        Ok(escrowed)
    }

    pub fn balances(&self, deps: Deps, env: &Env, denoms: &HashSet<String>) -> StdResult<Coins> {
        let mut balances = Coins::default();

        for action in self.actions.iter() {
            let action_balances = action.balances(deps, env, denoms)?;

            for balance in action_balances {
                balances.add(balance)?;
            }
        }

        Ok(balances)
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
                event.ty = format!("calc/{}/{}", event.ty, decorator);
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
pub struct Active;

#[cw_serde]
pub struct Executable {
    messages: Vec<StrategyMsg>,
    events: Vec<Event>,
}

#[cw_serde]
pub struct Committable {
    messages: Vec<StrategyMsg>,
    events: Vec<Event>,
}

#[cw_serde]
pub struct Committed;

impl Strategy<Json> {
    pub fn with_affiliates(self, affiliates: &Vec<Affiliate>) -> StdResult<Strategy<Indexable>> {
        let mut initialised_actions = vec![];

        for action in self.actions {
            initialised_actions.push(action.add_affiliates(affiliates)?);
        }

        Ok(Strategy {
            owner: self.owner,
            actions: initialised_actions,
            state: Indexable,
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
        let salt_data = to_json_binary(&(self.owner.to_string(), self.clone(), env.block.height))?;
        let mut hash = DefaultHasher::new();
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
            actions: self.actions.clone(),
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
            actions: self.actions.clone(),
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
            msg: to_json_binary(&Strategy {
                owner: self.owner,
                actions: self.actions,
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
                actions: self.actions,
                state: Indexed {
                    contract_address: self.state.contract_address.clone(),
                },
            }))?,
            info.funds,
        ))
    }
}

impl Strategy<Indexed> {
    pub fn get_operations(&self) -> Vec<OpNode> {
        let mut current_index = 0;

        let mut root_node = OpNode {
            operation: OperationImpl::Action(self.actions[0].clone()),
            index: current_index,
            next: None,
        };

        let mut nodes = vec![];

        for action in self.actions.clone().into_iter().skip(1) {
            let action_nodes = action.to_operations(current_index + 1);
            current_index += (action_nodes.len() + 1) as u16;
            nodes.extend(action_nodes);
        }

        root_node.next = Some(root_node.index + (nodes.len() + 1) as u16);
        return vec![root_node].into_iter().chain(nodes).collect();
    }
}

#[cw_serde]
pub enum OperationImpl {
    Action(Action),
    Condition(Condition),
}

#[cw_serde]
pub struct OpNode {
    pub operation: OperationImpl,
    pub index: u16,
    pub next: Option<u16>,
}

impl OpNode {
    pub fn next_index(&self, deps: Deps, env: &Env) -> Option<u16> {
        match &self.operation {
            OperationImpl::Action(_) => self.next,
            OperationImpl::Condition(condition) => {
                if condition.is_satisfied(deps, env).unwrap_or(false) {
                    Some(self.index + 1)
                } else {
                    self.next
                }
            }
        }
    }
}

impl Strategy<Committed> {}
