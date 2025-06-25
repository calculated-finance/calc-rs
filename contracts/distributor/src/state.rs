use calc_rs::types::{DistributeStatistics, DistributeStrategyConfig};
use cw_storage_plus::Item;

pub const CONFIG: Item<DistributeStrategyConfig> = Item::new("config");

pub const STATISTICS: Item<DistributeStatistics> = Item::new("statistics");
