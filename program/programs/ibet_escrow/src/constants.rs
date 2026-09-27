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

/// The only durations a bet may run for, in days. Mirrors the UI's timeframe
/// chips; keeping it on-chain stops odd durations being smuggled in.
pub const ALLOWED_DURATION_DAYS: [i64; 6] = [1, 3, 5, 7, 14, 30];

/// direction: creator says the market cap ends at or above the target.
pub const DIRECTION_HIGHER: u8 = 0;
/// direction: creator says the market cap ends at or below the target.
pub const DIRECTION_LOWER: u8 = 1;

/// True when `secs` is one of the whitelisted durations.
pub fn is_allowed_duration(secs: i64) -> bool {
    ALLOWED_DURATION_DAYS
        .iter()
        .any(|days| days.checked_mul(SECS_PER_DAY) == Some(secs))
}
