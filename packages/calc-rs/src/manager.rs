use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, HexBinary};
use cw_storage_plus::{Key, Prefixer, PrimaryKey};

use crate::strategy::Node;

#[cw_serde]
pub struct ManagerConfig {
    pub owner: Addr,
    pub fee_collector: Addr,
    pub strategy_code_id: u64,
}

#[cw_serde]
pub struct ManagerSudoMsg {
    pub fee_collector: Addr,
    pub strategy_code_id: u64,
}

#[cw_serde]
pub enum NodeStatus {
    Active,
    Deprecated,
    Disabled,
}

impl NodeStatus {
    pub fn as_str(&self) -> &str {
        match self {
            NodeStatus::Active => "active",
            NodeStatus::Deprecated => "deprecated",
            NodeStatus::Disabled => "disabled",
        }
    }
}

#[cw_serde]
pub struct RegisteredNode {
    pub address: Addr,
    pub code_id: u64,
    pub checksum: HexBinary,
    pub status: NodeStatus,
    pub size_weight: u16,
}

#[cw_serde]
pub enum StrategyStatus {
    Active,
    Paused,
    Archived,
}

impl StrategyStatus {
    pub fn as_str(&self) -> &str {
        match self {
            StrategyStatus::Active => "active",
            StrategyStatus::Paused => "paused",
            StrategyStatus::Archived => "archived",
        }
    }
}

impl<'a> Prefixer<'a> for StrategyStatus {
    fn prefix(&self) -> Vec<Key<'_>> {
        vec![Key::Val8([self.clone() as u8])]
    }
}

impl<'a> PrimaryKey<'a> for StrategyStatus {
    type Prefix = Self;
    type SubPrefix = Self;
    type Suffix = ();
    type SuperSuffix = ();

    fn key(&self) -> Vec<Key<'_>> {
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
    pub source: Option<String>,
    pub owner: Addr,
    pub contract_address: Addr,
    pub created_at: u64,
    pub updated_at: u64,
    pub label: String,
    pub status: StrategyStatus,
}

#[cw_serde]
pub enum ManagerExecuteMsg {
    Instantiate {
        source: Option<String>,
        owner: Option<Addr>,
        label: String,
        affiliates: Vec<Affiliate>,
        nodes: Vec<Node>,
    },
    Execute {
        contract_address: Addr,
    },
    UpdateStatus {
        contract_address: Addr,
        status: StrategyStatus,
    },
    Update {
        contract_address: Addr,
        nodes: Vec<Node>,
    },
    UpdateLabel {
        contract_address: Addr,
        label: String,
    },
    DeployNode {
        code_id: u64,
        label: String,
        instantiate_msg: Binary,
        size_weight: u16,
    },
    UpdateNodeStatus {
        address: Addr,
        status: NodeStatus,
    },
    MigrateNode {
        address: Addr,
        new_code_id: u64,
        migrate_msg: Binary,
        new_size_weight: u16,
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
    #[returns(Vec<Strategy>)]
    StrategiesById {
        start_after: Option<Addr>,
        limit: Option<u16>,
    },
    #[returns(u64)]
    Count {},
    #[returns(RegisteredNode)]
    Node { address: Addr },
    #[returns(Vec<RegisteredNode>)]
    Nodes {
        start_after: Option<Addr>,
        limit: Option<u16>,
    },
}
