use calc_rs::types::StrategyStatus;
use cosmwasm_std::{Addr, StdResult, Storage};
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, Item, UniqueIndex};

use crate::types::{Config, StrategyIndexItem};

const CONFIG: Item<Config> = Item::new("config");

pub fn get_config(store: &dyn Storage) -> StdResult<Config> {
    CONFIG.load(store)
}

pub fn update_config(store: &mut dyn Storage, config: Config) -> StdResult<Config> {
    CONFIG.save(store, &config)?;
    Ok(config)
}

const STRATEGY_COUNTER: Item<u64> = Item::new("strategy_counter");

struct StrategyIndexes<'a> {
    pub owner: UniqueIndex<'a, (Addr, Addr), StrategyIndexItem, Addr>,
    pub owner_status: UniqueIndex<'a, (Addr, u8, Addr), StrategyIndexItem, Addr>,
}

impl<'a> IndexList<StrategyIndexItem> for StrategyIndexes<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<StrategyIndexItem>> + '_> {
        let s: Vec<&dyn Index<StrategyIndexItem>> = vec![&self.owner, &self.owner_status];
        Box::new(s.into_iter())
    }
}

fn strategy_store<'a>() -> IndexedMap<Addr, StrategyIndexItem, StrategyIndexes<'a>> {
    IndexedMap::new(
        "strategies_v1",
        StrategyIndexes {
            owner: UniqueIndex::new(
                |s| (s.owner.clone(), s.contract_address.clone()),
                "strategies_v1__owner_status",
            ),
            owner_status: UniqueIndex::new(
                |s| {
                    (
                        s.owner.clone(),
                        s.status.clone() as u8,
                        s.contract_address.clone(),
                    )
                },
                "strategies_v1__owner_status",
            ),
        },
    )
}

pub struct AddStrategyIndexCommand {
    pub owner: Addr,
    pub contract_address: Addr,
    pub status: StrategyStatus,
    pub updated_at: u64,
}

impl From<AddStrategyIndexCommand> for StrategyIndexItem {
    fn from(cmd: AddStrategyIndexCommand) -> Self {
        StrategyIndexItem {
            owner: cmd.owner,
            contract_address: cmd.contract_address,
            status: cmd.status,
            updated_at: cmd.updated_at,
        }
    }
}

pub struct UpdateStrategyIndexCommand {
    pub status: Option<StrategyStatus>,
    pub updated_at: u64,
}

pub fn add_strategy_index_item(
    store: &mut dyn Storage,
    command: AddStrategyIndexCommand,
) -> StdResult<()> {
    let total = STRATEGY_COUNTER.may_load(store)?.unwrap_or_default() + 1;
    STRATEGY_COUNTER.save(store, &total)?;
    strategy_store().save(store, command.contract_address.clone(), &command.into())
}

pub fn update_strategy_index_item(
    store: &mut dyn Storage,
    contract_address: Addr,
    command: UpdateStrategyIndexCommand,
) -> StdResult<()> {
    let strategies = strategy_store();
    let item = strategies.load(store, contract_address.clone())?;
    strategy_store().save(
        store,
        contract_address,
        &StrategyIndexItem {
            status: command.status.unwrap_or(item.status),
            updated_at: command.updated_at,
            ..item
        },
    )
}

pub fn get_strategy_index_items(
    store: &dyn Storage,
    owner: Addr,
    status: Option<StrategyStatus>,
    start_after: Option<Addr>,
    limit: Option<u16>,
) -> StdResult<Vec<StrategyIndexItem>> {
    Ok(match status {
        Some(status) => strategy_store()
            .idx
            .owner_status
            .prefix((owner, status as u8)),
        None => strategy_store().idx.owner.prefix(owner),
    }
    .range(
        store,
        start_after.map(Bound::exclusive),
        None,
        cosmwasm_std::Order::Ascending,
    )
    .take(limit.unwrap_or(10) as usize)
    .flat_map(|result| result.map(|(_, strategy)| strategy))
    .collect::<Vec<StrategyIndexItem>>())
}
