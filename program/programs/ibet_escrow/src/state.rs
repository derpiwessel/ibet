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

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BetStatus {
    Open,
    Matched,
    Settled,
    Cancelled,
    Refunded,
}

// All variants are unit variants, so borsh encodes the status in one byte.
impl anchor_lang::Space for BetStatus {
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
    pub final_mcap_usd: u64,
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

    /// Did the creator's call come true? The target is inclusive on both sides:
    /// landing exactly on it means the creator wins.
    pub fn creator_wins(&self, final_mcap_usd: u64) -> Result<bool> {
        match self.direction {
            DIRECTION_HIGHER => Ok(final_mcap_usd >= self.target_mcap_usd),
            DIRECTION_LOWER => Ok(final_mcap_usd <= self.target_mcap_usd),
            _ => Err(EscrowError::InvalidDirection.into()),
        }
    }
}
