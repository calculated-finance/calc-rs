use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Binary, Coin};
use cw_storage_plus::{Key, Prefixer, PrimaryKey};

use crate::strategy::Strategy2;

#[cw_serde]
pub struct ManagerConfig {
    pub admin: Addr,
    pub fee_collector: Addr,
    pub affiliate_creation_fee: Coin,
    pub default_affiliate_bps: u64,
    pub code_ids: Vec<(StrategyType, u64)>,
}

#[derive(Hash, Eq)]
#[cw_serde]
pub enum StrategyType {
    Twap,
    Ladder,
}

impl<'a> Prefixer<'a> for StrategyType {
    fn prefix(&self) -> Vec<Key> {
        vec![Key::Val8([self.clone() as u8])]
    }
}

impl<'a> PrimaryKey<'a> for StrategyType {
    type Prefix = Self;
    type SubPrefix = Self;
    type Suffix = ();
    type SuperSuffix = ();

    fn key(&self) -> Vec<Key> {
        vec![Key::Val8([self.clone() as u8])]
    }
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
    pub code: String,
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
    pub affiliates: Vec<Affiliate>,
}

// #[cw_serde]
// pub enum StrategyStatistics {
//     Twap {
//         remaining: Coin,
//         swapped: Coin,
//         received: Coin,
//         distributed: HashMap<String, Vec<Coin>>,
//         withdrawn: Vec<Coin>,
//     },
// }

// #[cw_serde]
// pub enum CreateStrategyConfig {
//     Twap(InstantiateTwapCommand),
//     Ladder(InstantiateLadderCommand),
// }

// impl CreateStrategyConfig {
//     pub fn strategy_type(&self) -> StrategyType {
//         match self {
//             CreateStrategyConfig::Twap { .. } => StrategyType::Twap,
//             CreateStrategyConfig::Ladder { .. } => StrategyType::Ladder,
//         }
//     }
// }

#[cw_serde]
pub struct ManagerInstantiateMsg {
    pub admin: Addr,
    pub code_ids: Vec<(StrategyType, u64)>,
    pub fee_collector: Addr,
    pub affiliate_creation_fee: Coin,
    pub default_affiliate_bps: u64,
}

#[cw_serde]
pub struct ManagerMigrateMsg {
    pub code_ids: Vec<(StrategyType, u64)>,
    pub fee_collector: Addr,
    pub affiliate_creation_fee: Coin,
    pub default_affiliate_bps: u64,
}

#[cw_serde]
pub enum ManagerExecuteMsg {
    InstantiateStrategy {
        owner: Addr,
        label: String,
        strategy: Strategy2,
    },
    ExecuteStrategy {
        contract_address: Addr,
        msg: Option<Binary>,
    },
    UpdateStrategyStatus {
        contract_address: Addr,
        status: StrategyStatus,
    },
    UpdateStrategy {
        contract_address: Addr,
        update: Strategy2,
    },
    AddAffiliate {
        code: String,
        address: Addr,
        bps: u64,
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
    #[returns(Option<Affiliate>)]
    Affiliate { code: String },
    #[returns(Vec<Affiliate>)]
    Affiliates {
        start_after: Option<Addr>,
        limit: Option<u16>,
    },
}

// pub enum DomainEvent {
//     StrategyInstantiated {
//         contract_address: Addr,
//         config: CreateStrategyConfig,
//     },
//     StrategyExecuted {
//         contract_address: Addr,
//     },
//     StrategyUpdated {
//         contract_address: Addr,
//         update: Strategy2,
//     },
//     StrategyStatusUpdated {
//         contract_address: Addr,
//         status: StrategyStatus,
//     },
// }

// impl From<DomainEvent> for Event {
//     fn from(event: DomainEvent) -> Self {
//         match event {
//             DomainEvent::StrategyInstantiated {
//                 contract_address,
//                 config,
//             } => Event::new("strategy_created")
//                 .add_attribute("contract_address", contract_address.as_str())
//                 .add_attribute("config", format!("{:#?}", config)),
//             DomainEvent::StrategyExecuted { contract_address } => Event::new("strategy_executed")
//                 .add_attribute("contract_address", contract_address.as_str()),
//             DomainEvent::StrategyUpdated {
//                 contract_address,
//                 update,
//             } => Event::new("strategy_updated")
//                 .add_attribute("contract_address", contract_address.as_str())
//                 .add_attribute("update", format!("{:#?}", update)),
//             DomainEvent::StrategyStatusUpdated {
//                 contract_address,
//                 status,
//             } => Event::new("strategy_status_updated")
//                 .add_attribute("contract_address", contract_address.as_str())
//                 .add_attribute("status", format!("{:?}", status)),
//         }
//     }
// }
