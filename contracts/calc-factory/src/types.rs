use calc_rs::types::StrategyStatus;

pub struct Config {
    valid_code_ids: Vec<u64>,
}

pub struct StrategyIndexItem {
    pub owner: Addr,
    pub contract: Addr,
    pub status: StrategyStatus,
    pub updated_at: u64,
}

// Fetch all vaults that are ready to be checked
// Fetch all vaults for an owner
// Fetch all (active | paused | archived) vaults for an owner
