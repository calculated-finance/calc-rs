/**
 * Reply IDs for handling SubMsg responses.
 *
 * 1. LOG_ERRORS_REPLY_ID is used to log errors from SubMsgs.
 * 2. PROCESS_PAYLOAD_REPLY_ID is used to process payloads from SubMsgs,
 *    including committing cached state and confirming statistics updates.
 *
 * We use hand rolled IDs to avoid accidental reply id collisions
 */
pub const LOG_ERRORS_REPLY_ID: u64 = 756328923;
pub const PROCESS_PAYLOAD_REPLY_ID: u64 = 324623423;

/**
 * Base fee in basis points (bps) for the strategy.
 *
 * Taken on all distributions out of the strategy.
 * Affiliates can take up to 10 bps out of this base fee,
 * with any affiliate bps over 10 adding to the total
 * fees taken by the strategy.
 */
pub const BASE_FEE_BPS: u64 = 25;

/**
 * Maximum size of a strategy in terms of actions & conditions.
 *
 * Action sizes:
 * - Distribute: number of destinations + 1
 * - FinSwap: 4
 * - ThorSwap: 4
 * - OptimalSwap: number of routes * 4
 * - LimitOrder: 4
 * - Schedule: size of scheduled action + 1
 * - Conditional: size of action + size of condition + 1
 * - Many: sum of sizes of actions + 1
 *
 * Condition sizes:
 * - Timestamp elapsed: 1
 * - Blocks completed: 1
 * - Can swap: 2
 * - Limit order filled: 2
 * - Balance available: 1
 * - Strategy balance available: 1
 * - Strategy in status: 2
 * - Not: size of condition
 * - Composite: sum of sizes of conditions + 1
 */
pub const MAX_STRATEGY_SIZE: usize = 35;
