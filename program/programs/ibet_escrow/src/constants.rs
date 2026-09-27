use anchor_lang::prelude::*;

#[constant]
pub const CONFIG_SEED: &[u8] = b"config";

#[constant]
pub const BET_SEED: &[u8] = b"bet";

/// Hard ceiling on the platform fee (5%). A compromised admin still cannot
/// raise the fee beyond this, so a matched pot always mostly reaches the winner.
#[constant]
pub const MAX_FEE_BPS: u16 = 500;

pub const BPS_DENOMINATOR: u128 = 10_000;

pub const SECS_PER_DAY: i64 = 86_400;

/// How long a bet may run: five minutes at the short end, thirty days at the
/// long end. A range rather than a fixed list, so the UI can offer whatever
/// presets it likes and a custom length besides, while the chain still refuses
/// anything too short to resolve or too long to sit in escrow.
///
/// Five minutes is the floor because settlement reads 1-minute candles: less
/// than that and a bet would be decided by one or two of them.
pub const MIN_DURATION_SECS: i64 = 5 * 60;
pub const MAX_DURATION_SECS: i64 = 30 * SECS_PER_DAY;

/// direction: creator says the market cap ends at or above the target.
pub const DIRECTION_HIGHER: u8 = 0;
/// direction: creator says the market cap ends at or below the target.
pub const DIRECTION_LOWER: u8 = 1;

/// True when `secs` is within the allowed range, both ends inclusive.
pub fn is_allowed_duration(secs: i64) -> bool {
    secs >= MIN_DURATION_SECS && secs <= MAX_DURATION_SECS
}
