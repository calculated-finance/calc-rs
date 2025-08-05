/**
 * Reply IDs for handling SubMsg responses.
 *
 * 1. LOG_ERRORS_REPLY_ID is used to log errors from SubMsgs.
 * 2. PROCESS_PAYLOAD_REPLY_ID is used to process payloads from SubMsgs,
 *    including committing cached state and confirming statistics updates.
 *
 * We use hand rolled random IDs to avoid accidental reply id collisions
 */
pub const LOG_ERRORS_REPLY_ID: u64 = 756328923;
pub const PROCESS_PAYLOAD_REPLY_ID: u64 = 324623423;

/**
 * Base fee in basis points (bps) for the strategy.
 *
 * Taken on all distributions/withdrawals out of the strategy.
 * Affiliates can take up to 10 bps out of this base fee,
 * with any affiliate bps over 10 adding to the total
 * fees taken by the strategy.
 */
pub const BASE_FEE_BPS: u64 = 25;

/**
 * Maximum total affiliate basis points (bps) that can be applied to a strategy.
 *
 * This is the maximum amount of bps that can be taken by affiliates to
 * prevent excessive fees being applied to strategies.
 */
pub const MAX_TOTAL_AFFILIATE_BPS: u64 = 200;

/**
 * Maximum size of a strategy as a sum of its node sizes.
 * Each node size is determined by the action/condition it contains.
 */
pub const MAX_STRATEGY_SIZE: usize = 35;
