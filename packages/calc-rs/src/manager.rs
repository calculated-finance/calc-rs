use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Addr;
use cw_storage_plus::{Key, Prefixer, PrimaryKey};

use crate::actions::action::Action;

#[cw_serde]
pub struct ManagerConfig {
    pub fee_collector: Addr,
    pub strategy_code_id: u64,
}

#[cw_serde]
pub enum StrategyStatus {
    Active,
    Paused,
    Archived,
}

impl<'a> Prefixer<'a> for StrategyStatus {
    fn prefix(&self) -> Vec<Key> {
        vec![Key::Val8([self.clone() as u8])]
    }
}

impl<'a> PrimaryKey<'a> for StrategyStatus {
    type Prefix = Self;
    type SubPrefix = Self;
    type Suffix = ();
    type SuperSuffix = ();

    fn key(&self) -> Vec<Key> {
        vec![Key::Val8([self.clone() as u8])]
    }
}

#[cw_serde]
pub struct Affiliate {
    pub label: String,
    pub address: Addr,
    pub bps: u64,
}

#[cw_serde]
pub struct Strategy {
    pub id: u64,
    pub owner: Addr,
    pub contract_address: Addr,
    pub created_at: u64,
    pub updated_at: u64,
    pub label: String,
    pub status: StrategyStatus,
}

#[cw_serde]
pub enum ManagerExecuteMsg {
    InstantiateStrategy {
        owner: Addr,
        label: String,
        affiliates: Vec<Affiliate>,
        action: Action,
    },
    ExecuteStrategy {
        contract_address: Addr,
    },
    UpdateStrategyStatus {
        contract_address: Addr,
        status: StrategyStatus,
    },
    UpdateStrategy {
        contract_address: Addr,
        update: Action,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum ManagerQueryMsg {
    #[returns(ManagerConfig)]
    Config {},
    #[returns(Strategy)]
    Strategy { address: Addr },
    #[returns(Vec<Strategy>)]
    Strategies {
        owner: Option<Addr>,
        status: Option<StrategyStatus>,
        start_after: Option<u64>,
        limit: Option<u16>,
    },
}
