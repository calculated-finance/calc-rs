/**
  Base and Minimum fees in basis points (bps) for the strategy.
  Taken on all distributions/withdrawals out of the strategy.
  Affiliates can take up to BASE_FEE_BPS - MIN_FEE_BPS
  without increasing the total fees taken by the strategy.
  Any more than this will be added to the total affiliate fees.
*/
pub const BASE_FEE_BPS: u64 = 25;
pub const MIN_FEE_BPS: u64 = 15;

/**
  Maximum total affiliate basis points (bps) that can be applied to a strategy.
  This is the maximum amount of bps that can be taken by affiliates to
  prevent excessive fees being applied to strategies.
*/
pub const MAX_TOTAL_AFFILIATE_BPS: u64 = 200;

/**
  Maximum size of a strategy as a sum of its node sizes.
  Each node size is determined by the action/condition it contains.
*/
pub const MAX_STRATEGY_SIZE: usize = 50;
