use anchor_lang::prelude::*;

use crate::{constants::*, error::EscrowError};

#[account]
#[derive(InitSpace)]
pub struct Config {
    /// May update this config.
    pub admin: Pubkey,
    /// The only key allowed to settle bets. Phase 2 replaces this with an oracle.
    pub resolver: Pubkey,
    /// Receives the platform fee on settled pots.
    pub fee_wallet: Pubkey,
    /// Platform fee on the pot, in basis points. Capped at MAX_FEE_BPS.
    pub fee_bps: u16,
    pub min_stake: u64,
    pub max_stake: u64,
    /// How long after expiry the resolver still may settle. After this window
    /// anyone can trigger a no-fee refund instead.
    pub grace_secs: i64,
    /// Blocks create and take. Never blocks cancel, settle or refund, so funds
    /// already in escrow always have a way out.
    pub paused: bool,
    pub bump: u8,
}

/// Who the resolver says won. The program cannot see price history, so it
/// verifies what it can: a creator win has to come with a market cap that
/// actually reaches the target, observed inside the bet's window.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    CreatorWins,
    TakerWins,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BetStatus {
    Open,
    Matched,
    Settled,
    Cancelled,
    Refunded,
}

// All variants are unit variants, so borsh encodes these in one byte.
impl anchor_lang::Space for BetStatus {
    const INIT_SPACE: usize = 1;
}

impl anchor_lang::Space for Outcome {
    const INIT_SPACE: usize = 1;
}

#[account]
#[derive(InitSpace)]
pub struct Bet {
    pub creator: Pubkey,
    pub taker: Option<Pubkey>,
    /// The coin being bet on (its contract address).
    pub token_mint: Pubkey,
    pub direction: u8,
    /// Whole USD, not lamports or cents.
    pub target_mcap_usd: u64,
    /// Market cap when the bet was created; 0 means "was not available".
    pub start_mcap_usd: u64,
    /// Lamports staked by each side.
    pub stake: u64,
    pub duration_secs: i64,
    pub created_at: i64,
    /// 0 until the bet is taken.
    pub taken_at: i64,
    /// 0 until the bet is taken; the clock starts when a taker matches it.
    pub expires_at: i64,
    pub status: BetStatus,
    pub winner: Option<Pubkey>,
    /// The market cap the resolver settled on: the candle high that touched a
    /// Higher target, the low that touched a Lower one, or the last observed
    /// value when the target was never reached.
    pub observed_mcap_usd: u64,
    /// When that market cap was observed — the candle's own time, not the time
    /// the settlement was sent.
    pub observed_at: i64,
    /// Client-chosen id, part of the PDA seeds.
    pub bet_id: u64,
    pub bump: u8,
}

impl Bet {
    /// Both stakes together.
    pub fn pot(&self) -> Result<u64> {
        self.stake
            .checked_mul(2)
            .ok_or_else(|| EscrowError::MathOverflow.into())
    }

    /// Does this market cap touch the target? Inclusive on both sides: landing
    /// exactly on the target counts as a touch.
    pub fn touches_target(&self, mcap_usd: u64) -> Result<bool> {
        match self.direction {
            DIRECTION_HIGHER => Ok(mcap_usd >= self.target_mcap_usd),
            DIRECTION_LOWER => Ok(mcap_usd <= self.target_mcap_usd),
            _ => Err(EscrowError::InvalidDirection.into()),
        }
    }
}
