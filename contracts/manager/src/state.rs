use calc_rs::types::Status;
use cosmwasm_std::{Addr, Order, StdResult, Storage};
use cw_storage_plus::{Bound, Index, IndexList, IndexedMap, Item, UniqueIndex};

use crate::types::{Config, StrategyHandle};

pub const CONFIG: Item<Config> = Item::new("config");

pub fn get_config(store: &dyn Storage) -> StdResult<Config> {
    CONFIG.load(store)
}

pub fn update_config(store: &mut dyn Storage, config: Config) -> StdResult<Config> {
    CONFIG.save(store, &config)?;
    Ok(config)
}

pub const STRATEGY_COUNTER: Item<u64> = Item::new("strategy_counter");

struct StrategyHandles<'a> {
    pub owner_updated_at: UniqueIndex<'a, (Addr, u64, Addr), StrategyHandle, Addr>,
    pub owner_status: UniqueIndex<'a, (Addr, u8, Addr), StrategyHandle, Addr>,
    pub updated_at: UniqueIndex<'a, (u64, Addr), StrategyHandle, u64>,
}

impl<'a> IndexList<StrategyHandle> for StrategyHandles<'a> {
    fn get_indexes(&'_ self) -> Box<dyn Iterator<Item = &'_ dyn Index<StrategyHandle>> + '_> {
        let s: Vec<&dyn Index<StrategyHandle>> =
            vec![&self.owner_updated_at, &self.owner_status, &self.updated_at];
        Box::new(s.into_iter())
    }
}

fn strategy_store<'a>() -> IndexedMap<Addr, StrategyHandle, StrategyHandles<'a>> {
    IndexedMap::new(
        "strategies_v1",
        StrategyHandles {
            owner_updated_at: UniqueIndex::new(
                |s| (s.owner.clone(), s.updated_at, s.contract_address.clone()),
                "strategies_v1__owner_updated_at",
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
            updated_at: UniqueIndex::new(
                |s| (s.updated_at, s.contract_address.clone()),
                "strategies_v1__updated_at",
            ),
        },
    )
}

pub struct CreateStrategyHandleCommand {
    pub owner: Addr,
    pub contract_address: Addr,
    pub status: Status,
    pub updated_at: u64,
}

impl From<CreateStrategyHandleCommand> for StrategyHandle {
    fn from(cmd: CreateStrategyHandleCommand) -> Self {
        StrategyHandle {
            owner: cmd.owner,
            contract_address: cmd.contract_address,
            status: cmd.status,
            updated_at: cmd.updated_at,
        }
    }
}

pub fn create_strategy_handle(
    store: &mut dyn Storage,
    command: CreateStrategyHandleCommand,
) -> StdResult<()> {
    let total = STRATEGY_COUNTER.may_load(store)?.unwrap_or_default() + 1;
    STRATEGY_COUNTER.save(store, &total)?;
    strategy_store().save(store, command.contract_address.clone(), &command.into())
}

pub struct UpdateStrategyStatusCommand {
    pub contract_address: Addr,
    pub status: Status,
    pub updated_at: u64,
}

pub fn update_strategy_status(
    store: &mut dyn Storage,
    command: UpdateStrategyStatusCommand,
) -> StdResult<()> {
    let strategies = strategy_store();
    let handle = strategies.load(store, command.contract_address.clone())?;
    strategy_store().save(
        store,
        command.contract_address,
        &StrategyHandle {
            status: command.status,
            updated_at: command.updated_at,
            ..handle
        },
    )
}

pub fn get_strategy_statuses(
    store: &dyn Storage,
    owner: Addr,
    status: Option<Status>,
    start_after: Option<Addr>,
    limit: Option<u16>,
) -> StdResult<Vec<StrategyHandle>> {
    Ok(match status {
        Some(status) => strategy_store()
            .idx
            .owner_status
            .prefix((owner, status as u8)),
        None => strategy_store()
            .idx
            .owner_updated_at
            .prefix((owner, u64::MAX)),
    }
    .range(
        store,
        start_after.map(Bound::exclusive),
        None,
        Order::Ascending,
    )
    .take(limit.unwrap_or(10) as usize)
    .flat_map(|result| result.map(|(_, handle)| handle))
    .collect::<Vec<StrategyHandle>>())
}

#[cfg(test)]
mod tests {
    use cosmwasm_std::{testing::MockStorage, HexBinary};

    use super::*;

    #[test]
    fn test_get_config_returns_saved_config() {
        let mut store = MockStorage::default();
        let config = Config {
            checksum: HexBinary::default(),
            code_id: 1,
        };
        CONFIG.save(&mut store, &config).unwrap();

        let result = get_config(&store).unwrap();
        assert_eq!(result.code_id, config.code_id);
    }

    #[test]
    fn test_update_config_saves_and_returns_config() {
        let mut store = MockStorage::default();
        CONFIG
            .save(
                &mut store,
                &Config {
                    checksum: HexBinary::default(),
                    code_id: 1,
                },
            )
            .unwrap();
        let config = Config {
            checksum: HexBinary::default(),
            code_id: 2,
        };
        let result = update_config(&mut store, config.clone()).unwrap();

        assert_eq!(result.code_id, config.code_id);
        assert_eq!(CONFIG.load(&store).unwrap().code_id, config.code_id);
    }

    #[test]
    fn test_add_strategy_handle_increments_counter_and_saves() {
        let mut store = MockStorage::default();
        let command = CreateStrategyHandleCommand {
            owner: Addr::unchecked(format!(
                "owner-{}",
                STRATEGY_COUNTER
                    .may_load(&store)
                    .unwrap()
                    .unwrap_or_default()
            )),
            contract_address: Addr::unchecked("contract"),
            status: Status::Active,
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

        let command = CreateStrategyHandleCommand {
            owner: Addr::unchecked(format!(
                "owner-{}",
                STRATEGY_COUNTER
                    .may_load(&store)
                    .unwrap()
                    .unwrap_or_default()
            )),
            contract_address: Addr::unchecked("contract"),
            status: Status::Active,
            updated_at: 1234567890,
        };
        create_strategy_handle(&mut store, command).unwrap();

        let handle = strategy_store()
            .load(&store, Addr::unchecked("contract"))
            .unwrap();
        assert_eq!(handle.status, Status::Active);

        let command = UpdateStrategyStatusCommand {
            contract_address: Addr::unchecked("contract"),
            status: Status::Archived,
            updated_at: 1234567890,
        };
        update_strategy_status(&mut store, command).unwrap();

        let handle = strategy_store()
            .load(&store, Addr::unchecked("contract"))
            .unwrap();
        assert_eq!(handle.status, Status::Archived);
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
            CreateStrategyHandleCommand {
                owner: owner.clone(),
                contract_address: contract1.clone(),
                status: Status::Active,
                updated_at: 1234567890,
            },
        )
        .unwrap();

        create_strategy_handle(
            &mut store,
            CreateStrategyHandleCommand {
                owner: owner.clone(),
                contract_address: contract2.clone(),
                status: Status::Archived,
                updated_at: 1234567890,
            },
        )
        .unwrap();

        let handles = get_strategy_statuses(&store, owner, None, None, None).unwrap();
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
            CreateStrategyHandleCommand {
                owner: owner.clone(),
                contract_address: contract1.clone(),
                status: Status::Active,
                updated_at: 1234567890,
            },
        )
        .unwrap();

        create_strategy_handle(
            &mut store,
            CreateStrategyHandleCommand {
                owner: owner.clone(),
                contract_address: contract2,
                status: Status::Archived,
                updated_at: 1234567890,
            },
        )
        .unwrap();

        let handles =
            get_strategy_statuses(&store, owner, Some(Status::Active), None, None).unwrap();
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
            CreateStrategyHandleCommand {
                owner: owner.clone(),
                contract_address: contract1.clone(),
                status: Status::Active,
                updated_at: 1234567890,
            },
        )
        .unwrap();

        create_strategy_handle(
            &mut store,
            CreateStrategyHandleCommand {
                owner: owner.clone(),
                contract_address: contract2.clone(),
                status: Status::Archived,
                updated_at: 1234567890,
            },
        )
        .unwrap();

        let handles = get_strategy_statuses(&store, owner, None, Some(contract1), Some(1)).unwrap();
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
        let handles = get_strategy_statuses(&store, owner, None, None, None).unwrap();
        assert_eq!(handles.len(), 0);
    }

    #[test]
    fn test_add_strategy_handle_multiple_times_increments_counter() {
        let mut store = MockStorage::default();
        let command = CreateStrategyHandleCommand {
            owner: Addr::unchecked(format!(
                "owner-{}",
                STRATEGY_COUNTER
                    .may_load(&store)
                    .unwrap()
                    .unwrap_or_default()
            )),
            contract_address: Addr::unchecked("contract"),
            status: Status::Active,
            updated_at: 1234567890,
        };
        create_strategy_handle(&mut store, command).unwrap();

        let count = STRATEGY_COUNTER.load(&store).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_update_strategy_handle_fails_for_nonexistent_item() {
        let mut store = MockStorage::default();
        let command = UpdateStrategyStatusCommand {
            contract_address: Addr::unchecked("contract"),
            status: Status::Paused,
            updated_at: 1234567890,
        };

        update_strategy_status(&mut store, command).unwrap_err();
    }
}
