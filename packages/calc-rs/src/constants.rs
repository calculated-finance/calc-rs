// Use hand rolled random numbers to prevent accidental matching
pub const LOG_ERRORS_REPLY_ID: u64 = 756328923;
pub const PROCESS_PAYLOAD_REPLY_ID: u64 = 324623423;

// Base fee in basis points (bps) - taken on all distributions
// out of the strategy. Affiliates can take up to 10 bps out of
// this base fee, with any affiliate bps over 10 adding to the
// total fees taken by the strategy.
pub const BASE_FEE_BPS: u64 = 25;
