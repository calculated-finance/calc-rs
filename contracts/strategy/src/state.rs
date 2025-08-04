use std::collections::HashSet;

use calc_rs::{
    statistics::Statistics,
    strategy::{Indexed, OpNode, Strategy, StrategyConfig, StrategyExecuteMsg},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Deps, Env, StdError, StdResult, Storage};
use cw_storage_plus::{Item, Map};

pub const DENOMS: Item<HashSet<String>> = Item::new("denoms");
pub const ESCROWED: Item<HashSet<String>> = Item::new("escrowed");
pub const STATE: Item<StrategyExecuteMsg> = Item::new("state");
pub const STATS: Item<Statistics> = Item::new("stats");

pub const CONFIG: StrategyStore = StrategyStore {
    store: Item::new("config"),
};

#[cw_serde]
pub struct StoredStrategy {
    pub manager: Addr,
    pub strategy: Strategy<Indexed>,
}

pub struct StrategyStore {
    store: Item<StoredStrategy>,
}

impl StrategyStore {
    pub fn init(&self, storage: &mut dyn Storage, config: StrategyConfig) -> StdResult<()> {
        DENOMS.save(storage, &config.denoms)?;
        ESCROWED.save(storage, &config.escrowed)?;
        STATS.save(storage, &Statistics::default())?;
        ACTIONS.init(storage, config.strategy.clone())?;

        self.store.save(
            storage,
            &StoredStrategy {
                manager: config.manager,
                strategy: config.strategy,
            },
        )
    }

    pub fn update(&self, storage: &mut dyn Storage, config: StrategyConfig) -> StdResult<()> {
        let existing_denoms = DENOMS.load(storage)?;

        DENOMS.save(
            storage,
            &config
                .denoms
                .union(&existing_denoms)
                .cloned()
                .collect::<HashSet<String>>(),
        )?;

        let existing_escrowed = ESCROWED.load(storage)?;

        ESCROWED.save(
            storage,
            &config
                .escrowed
                .union(&existing_escrowed)
                .cloned()
                .collect::<HashSet<String>>(),
        )?;

        ACTIONS.init(storage, config.strategy.clone())?;

        self.store.update(storage, |config| {
            Ok::<StoredStrategy, StdError>(StoredStrategy {
                manager: config.manager,
                strategy: config.strategy,
            })
        })?;

        Ok(())
    }

    // pub fn save(&self, storage: &mut dyn Storage, update: Strategy<Indexed>) -> StdResult<()> {
    //     self.store.update(storage, |config| {
    //         Ok::<StoredStrategy, StdError>(StoredStrategy {
    //             manager: config.manager,
    //             strategy: update,
    //         })
    //     })?;

    //     Ok(())
    // }

    pub fn load(&self, storage: &dyn Storage) -> StdResult<StrategyConfig> {
        let stored_strategy = self.store.load(storage)?;
        Ok(StrategyConfig {
            manager: stored_strategy.manager,
            strategy: stored_strategy.strategy,
            denoms: DENOMS.load(storage)?,
            escrowed: ESCROWED.load(storage)?,
        })
    }
}

pub const ACTIONS: ActionStore = ActionStore {
    store: Map::new("actions"),
};

pub struct ActionStore {
    store: Map<u16, OpNode>,
}

impl ActionStore {
    pub fn init(&self, storage: &mut dyn Storage, strategy: Strategy<Indexed>) -> StdResult<()> {
        for action_node in strategy.get_operations() {
            self.store.save(storage, action_node.index, &action_node)?;
        }
        Ok(())
    }

    pub fn save(&self, storage: &mut dyn Storage, action_node: &OpNode) -> StdResult<()> {
        self.store.save(storage, action_node.index, action_node)
    }

    pub fn get_next(
        &self,
        deps: Deps,
        env: &Env,
        current: Option<OpNode>,
    ) -> StdResult<Option<OpNode>> {
        let index = current.map_or(Some(0), |node| node.next_index(deps, env));

        if let Some(index) = index {
            if let Some(action_node) = self.store.may_load(deps.storage, index)? {
                return Ok(Some(action_node));
            }
        }

        Ok(None)
    }

    pub fn load(&self, storage: &dyn Storage) -> StdResult<Vec<OpNode>> {
        let mut index = 0;
        let mut actions = vec![];

        loop {
            let action = self.store.may_load(storage, index)?;

            if let Some(action) = action {
                actions.push(action);
            } else {
                break;
            }

            index += 1;
        }

        Ok(actions)
    }
}
