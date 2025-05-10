use calc_rs::types::StrategyStatus;
use cosmwasm_std::{Addr, StdResult, Storage, Timestamp};
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, Item, Map, UniqueIndex};

use crate::types::{Config, StrategyHandle};

const CONFIG: Item<Config> = Item::new("config");

pub fn get_config(store: &dyn Storage) -> StdResult<Config> {
    CONFIG.load(store)
}

pub fn update_config(store: &mut dyn Storage, config: Config) -> StdResult<Config> {
    CONFIG.save(store, &config)?;
    Ok(config)
}

const STRATEGY_COUNTER: Item<u64> = Item::new("strategy_counter");

struct StrategyHandles<'a> {
    pub owner: UniqueIndex<'a, (Addr, Addr), StrategyHandle, Addr>,
    pub owner_status: UniqueIndex<'a, (Addr, u8, Addr), StrategyHandle, Addr>,
}

impl<'a> IndexList<StrategyHandle> for StrategyHandles<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<StrategyHandle>> + '_> {
        let s: Vec<&dyn Index<StrategyHandle>> = vec![&self.owner, &self.owner_status];
        Box::new(s.into_iter())
    }
}

fn strategy_store<'a>() -> IndexedMap<Addr, StrategyHandle, StrategyHandles<'a>> {
    IndexedMap::new(
        "strategies_v1",
        StrategyHandles {
            owner: UniqueIndex::new(
                |s| (s.owner.clone(), s.contract_address.clone()),
                "strategies_v1__owner",
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

pub const TIME_TRIGGERS: Map<(u64, Addr), Addr> = Map::new("strategy_time_triggers");

pub fn save_time_trigger(
    store: &mut dyn Storage,
    time: Timestamp,
    contract_address: Addr,
) -> StdResult<()> {
    TIME_TRIGGERS.save(
        store,
        (time.nanos(), contract_address.clone()),
        &contract_address,
    )
}

pub fn get_time_triggers(store: &dyn Storage, after: Timestamp) -> StdResult<Vec<Addr>> {
    Ok(TIME_TRIGGERS
        .range(
            store,
            Some(Bound::inclusive((after.nanos(), Addr::unchecked("")))),
            None,
            cosmwasm_std::Order::Ascending,
        )
        .map(|result| result.map(|(_, addr)| addr))
        .collect::<StdResult<Vec<Addr>>>()?)
}

pub const BLOCK_TRIGGERS: Map<(u64, Addr), Addr> = Map::new("strategy_block_triggers");

pub fn save_block_trigger(
    store: &mut dyn Storage,
    block: u64,
    contract_address: Addr,
) -> StdResult<()> {
    BLOCK_TRIGGERS.save(store, (block, contract_address.clone()), &contract_address)
}

pub fn get_block_triggers(store: &dyn Storage, after: u64) -> StdResult<Vec<Addr>> {
    Ok(BLOCK_TRIGGERS
        .range(
            store,
            Some(Bound::inclusive((after, Addr::unchecked("")))),
            None,
            cosmwasm_std::Order::Ascending,
        )
        .map(|result| result.map(|(_, addr)| addr))
        .collect::<StdResult<Vec<Addr>>>()?)
}

pub struct AddStrategyHandleCommand {
    pub owner: Addr,
    pub contract_address: Addr,
    pub status: StrategyStatus,
    pub updated_at: u64,
}

impl From<AddStrategyHandleCommand> for StrategyHandle {
    fn from(cmd: AddStrategyHandleCommand) -> Self {
        StrategyHandle {
            owner: cmd.owner,
            contract_address: cmd.contract_address,
            status: cmd.status,
            updated_at: cmd.updated_at,
        }
    }
}

pub struct UpdateStrategyHandleCommand {
    pub contract_address: Addr,
    pub status: Option<StrategyStatus>,
    pub updated_at: u64,
}

pub fn create_strategy_handle(
    store: &mut dyn Storage,
    command: AddStrategyHandleCommand,
) -> StdResult<()> {
    let total = STRATEGY_COUNTER.may_load(store)?.unwrap_or_default() + 1;
    STRATEGY_COUNTER.save(store, &total)?;
    strategy_store().save(store, command.contract_address.clone(), &command.into())
}

pub fn update_strategy_handle(
    store: &mut dyn Storage,
    command: UpdateStrategyHandleCommand,
) -> StdResult<()> {
    let strategies = strategy_store();
    let handle = strategies.load(store, command.contract_address.clone())?;
    strategy_store().save(
        store,
        command.contract_address,
        &StrategyHandle {
            status: command.status.unwrap_or(handle.status),
            updated_at: command.updated_at,
            ..handle
        },
    )
}

pub fn get_strategy_handles(
    store: &dyn Storage,
    owner: Addr,
    status: Option<StrategyStatus>,
    start_after: Option<Addr>,
    limit: Option<u16>,
) -> StdResult<Vec<StrategyHandle>> {
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
    .flat_map(|result| result.map(|(_, handle)| handle))
    .collect::<Vec<StrategyHandle>>())
}

#[cfg(test)]
mod tests {
    use cosmwasm_std::testing::MockStorage;

    use super::*;

    #[test]
    fn test_get_config_returns_saved_config() {
        let mut store = MockStorage::default();
        let config = Config { vault_code_id: 1 };
        CONFIG.save(&mut store, &config).unwrap();

        let result = get_config(&store).unwrap();
        assert_eq!(result.vault_code_id, config.vault_code_id);
    }

    #[test]
    fn test_update_config_saves_and_returns_config() {
        let mut store = MockStorage::default();
        CONFIG
            .save(&mut store, &Config { vault_code_id: 1 })
            .unwrap();
        let config = Config { vault_code_id: 2 };
        let result = update_config(&mut store, config.clone()).unwrap();

        assert_eq!(result.vault_code_id, config.vault_code_id);
        assert_eq!(
            CONFIG.load(&store).unwrap().vault_code_id,
            config.vault_code_id
        );
    }

    #[test]
    fn test_add_strategy_handle_increments_counter_and_saves() {
        let mut store = MockStorage::default();
        let command = AddStrategyHandleCommand {
            owner: Addr::unchecked(format!(
                "owner-{}",
                STRATEGY_COUNTER
                    .may_load(&store)
                    .unwrap()
                    .unwrap_or_default()
            )),
            contract_address: Addr::unchecked("contract"),
            status: StrategyStatus::Active,
            updated_at: 1234567890,
        };

        let original_count = STRATEGY_COUNTER
            .may_load(&store)
            .unwrap()
            .unwrap_or_default();
        create_strategy_handle(&mut store, command).unwrap();
        let subsequent_count = STRATEGY_COUNTER.load(&store).unwrap();
        assert_eq!(subsequent_count, original_count + 1);
    }

    #[test]
    fn test_update_strategy_handle_updates_existing_item() {
        let mut store = MockStorage::default();

        let command = AddStrategyHandleCommand {
            owner: Addr::unchecked(format!(
                "owner-{}",
                STRATEGY_COUNTER
                    .may_load(&store)
                    .unwrap()
                    .unwrap_or_default()
            )),
            contract_address: Addr::unchecked("contract"),
            status: StrategyStatus::Active,
            updated_at: 1234567890,
        };
        create_strategy_handle(&mut store, command).unwrap();

        let handle = strategy_store()
            .load(&store, Addr::unchecked("contract"))
            .unwrap();
        assert_eq!(handle.status, StrategyStatus::Active);

        let command = UpdateStrategyHandleCommand {
            contract_address: Addr::unchecked("contract"),
            status: Some(StrategyStatus::Archived),
            updated_at: 1234567890,
        };
        update_strategy_handle(&mut store, command).unwrap();

        let handle = strategy_store()
            .load(&store, Addr::unchecked("contract"))
            .unwrap();
        assert_eq!(handle.status, StrategyStatus::Archived);
    }

    #[test]
    fn test_get_strategy_handles_by_owner_returns_correct_items() {
        let mut store = MockStorage::default();
        let owner = Addr::unchecked(format!(
            "owner-{}",
            STRATEGY_COUNTER
                .may_load(&store)
                .unwrap()
                .unwrap_or_default()
        ));
        let contract1 = Addr::unchecked("contract1");
        let contract2 = Addr::unchecked("contract2");

        create_strategy_handle(
            &mut store,
            AddStrategyHandleCommand {
                owner: owner.clone(),
                contract_address: contract1.clone(),
                status: StrategyStatus::Active,
                updated_at: 1234567890,
            },
        )
        .unwrap();

        create_strategy_handle(
            &mut store,
            AddStrategyHandleCommand {
                owner: owner.clone(),
                contract_address: contract2.clone(),
                status: StrategyStatus::Archived,
                updated_at: 1234567890,
            },
        )
        .unwrap();

        let handles = get_strategy_handles(&store, owner, None, None, None).unwrap();
        assert_eq!(handles.len(), 2);
        assert_eq!(handles[0].contract_address, contract1);
        assert_eq!(handles[1].contract_address, contract2);
    }

    #[test]
    fn test_get_strategy_handles_by_owner_and_status_returns_correct_items() {
        let mut store = MockStorage::default();
        let owner = Addr::unchecked(format!(
            "owner-{}",
            STRATEGY_COUNTER
                .may_load(&store)
                .unwrap()
                .unwrap_or_default()
        ));
        let contract1 = Addr::unchecked("contract1");
        let contract2 = Addr::unchecked("contract2");

        create_strategy_handle(
            &mut store,
            AddStrategyHandleCommand {
                owner: owner.clone(),
                contract_address: contract1.clone(),
                status: StrategyStatus::Active,
                updated_at: 1234567890,
            },
        )
        .unwrap();

        create_strategy_handle(
            &mut store,
            AddStrategyHandleCommand {
                owner: owner.clone(),
                contract_address: contract2,
                status: StrategyStatus::Archived,
                updated_at: 1234567890,
            },
        )
        .unwrap();

        let handles =
            get_strategy_handles(&store, owner, Some(StrategyStatus::Active), None, None).unwrap();
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].contract_address, contract1);
    }

    #[test]
    fn test_get_strategy_handles_with_pagination() {
        let mut store = MockStorage::default();
        let owner = Addr::unchecked(format!(
            "owner-{}",
            STRATEGY_COUNTER
                .may_load(&store)
                .unwrap()
                .unwrap_or_default()
        ));
        let contract1 = Addr::unchecked("contract1");
        let contract2 = Addr::unchecked("contract2");

        create_strategy_handle(
            &mut store,
            AddStrategyHandleCommand {
                owner: owner.clone(),
                contract_address: contract1.clone(),
                status: StrategyStatus::Active,
                updated_at: 1234567890,
            },
        )
        .unwrap();

        create_strategy_handle(
            &mut store,
            AddStrategyHandleCommand {
                owner: owner.clone(),
                contract_address: contract2.clone(),
                status: StrategyStatus::Archived,
                updated_at: 1234567890,
            },
        )
        .unwrap();

        let handles = get_strategy_handles(&store, owner, None, Some(contract1), Some(1)).unwrap();
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].contract_address, contract2);
    }

    #[test]
    fn test_get_strategy_handles_returns_empty_when_none_found() {
        let store = MockStorage::default();
        let owner = Addr::unchecked(format!(
            "owner-{}",
            STRATEGY_COUNTER
                .may_load(&store)
                .unwrap()
                .unwrap_or_default()
        ));
        let handles = get_strategy_handles(&store, owner, None, None, None).unwrap();
        assert_eq!(handles.len(), 0);
    }

    #[test]
    fn test_add_strategy_handle_multiple_times_increments_counter() {
        let mut store = MockStorage::default();
        let command = AddStrategyHandleCommand {
            owner: Addr::unchecked(format!(
                "owner-{}",
                STRATEGY_COUNTER
                    .may_load(&store)
                    .unwrap()
                    .unwrap_or_default()
            )),
            contract_address: Addr::unchecked("contract"),
            status: StrategyStatus::Active,
            updated_at: 1234567890,
        };
        create_strategy_handle(&mut store, command).unwrap();

        let count = STRATEGY_COUNTER.load(&store).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_update_strategy_handle_fails_for_nonexistent_item() {
        let mut store = MockStorage::default();
        let command = UpdateStrategyHandleCommand {
            contract_address: Addr::unchecked("contract"),
            status: None,
            updated_at: 1234567890,
        };

        update_strategy_handle(&mut store, command).unwrap_err();
    }

    #[test]
    fn test_save_time_trigger_saves_and_retrieves() {
        let mut store = MockStorage::default();
        let time = Timestamp::from_nanos(1_000_000_000);
        let contract_address = Addr::unchecked("contract");

        save_time_trigger(&mut store, time, contract_address.clone()).unwrap();
        let result = TIME_TRIGGERS
            .load(&store, (time.nanos(), contract_address.clone()))
            .unwrap();

        assert_eq!(result, contract_address);
    }

    #[test]
    fn test_get_time_triggers_returns_correct_items() {
        let mut store = MockStorage::default();
        let time1 = Timestamp::from_nanos(1_000_000_000);
        let time2 = Timestamp::from_nanos(2_000_000_000);
        let contract1 = Addr::unchecked("contract1");
        let contract2 = Addr::unchecked("contract2");

        save_time_trigger(&mut store, time1, contract1.clone()).unwrap();
        save_time_trigger(&mut store, time2, contract2.clone()).unwrap();
        let triggers = get_time_triggers(&store, time1).unwrap();

        assert_eq!(triggers.len(), 2);
        assert_eq!(triggers[0], contract1);
        assert_eq!(triggers[1], contract2);
    }

    #[test]
    fn test_get_time_triggers_returns_empty_when_none_found() {
        let store = MockStorage::default();
        let time = Timestamp::from_nanos(1_000_000_000);

        let triggers = get_time_triggers(&store, time).unwrap();

        assert_eq!(triggers.len(), 0);
    }

    #[test]
    fn test_save_and_get_time_triggers_at_the_same_time() {
        let mut store = MockStorage::default();
        let time = Timestamp::from_nanos(1_000_000_000);
        let contract1 = Addr::unchecked("contract1");
        let contract2 = Addr::unchecked("contract2");

        save_time_trigger(&mut store, time, contract1.clone()).unwrap();
        save_time_trigger(&mut store, time, contract2.clone()).unwrap();
        let triggers = get_time_triggers(&store, time).unwrap();

        assert_eq!(triggers.len(), 2);
        assert_eq!(triggers[0], contract1);
        assert_eq!(triggers[1], contract2);
    }

    #[test]
    fn test_save_block_trigger_saves_and_retrieves() {
        let mut store = MockStorage::default();
        let block = 1;
        let contract_address = Addr::unchecked("contract");

        save_block_trigger(&mut store, block, contract_address.clone()).unwrap();
        let result = BLOCK_TRIGGERS
            .load(&store, (block, contract_address.clone()))
            .unwrap();

        assert_eq!(result, contract_address);
    }

    #[test]
    fn test_get_block_triggers_returns_correct_items() {
        let mut store = MockStorage::default();
        let block1 = 1;
        let block2 = 2;
        let contract1 = Addr::unchecked("contract1");
        let contract2 = Addr::unchecked("contract2");

        save_block_trigger(&mut store, block1, contract1.clone()).unwrap();
        save_block_trigger(&mut store, block2, contract2.clone()).unwrap();
        let triggers = get_block_triggers(&store, block1).unwrap();

        assert_eq!(triggers.len(), 2);
        assert_eq!(triggers[0], contract1);
        assert_eq!(triggers[1], contract2);
    }

    #[test]
    fn test_get_block_triggers_returns_empty_when_none_found() {
        let store = MockStorage::default();
        let block = 1;

        let triggers = get_block_triggers(&store, block).unwrap();

        assert_eq!(triggers.len(), 0);
    }

    #[test]
    fn test_save_and_get_block_triggers_at_the_same_time() {
        let mut store = MockStorage::default();
        let block = 1;
        let contract1 = Addr::unchecked("contract1");
        let contract2 = Addr::unchecked("contract2");

        save_block_trigger(&mut store, block, contract1.clone()).unwrap();
        save_block_trigger(&mut store, block, contract2.clone()).unwrap();
        let triggers = get_block_triggers(&store, block).unwrap();

        assert_eq!(triggers.len(), 2);
        assert_eq!(triggers[0], contract1);
        assert_eq!(triggers[1], contract2);
    }
}
