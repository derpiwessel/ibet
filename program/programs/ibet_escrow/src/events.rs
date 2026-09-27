use anchor_lang::prelude::*;

#[event]
pub struct ConfigUpdated {
    pub admin: Pubkey,
    pub resolver: Pubkey,
    pub fee_wallet: Pubkey,
    pub fee_bps: u16,
    pub min_stake: u64,
    pub max_stake: u64,
    pub grace_secs: i64,
    pub paused: bool,
}

#[event]
pub struct BetCreated {
    pub bet: Pubkey,
    pub creator: Pubkey,
    pub bet_id: u64,
    pub token_mint: Pubkey,
    pub direction: u8,
    pub target_mcap_usd: u64,
    pub start_mcap_usd: u64,
    pub stake: u64,
    pub duration_secs: i64,
    pub created_at: i64,
}

#[event]
pub struct BetTaken {
    pub bet: Pubkey,
    pub creator: Pubkey,
    pub taker: Pubkey,
    pub bet_id: u64,
    pub taken_at: i64,
    pub expires_at: i64,
}

#[event]
pub struct BetCancelled {
    pub bet: Pubkey,
    pub creator: Pubkey,
    pub bet_id: u64,
    pub refunded: u64,
}

#[event]
pub struct BetSettled {
    pub bet: Pubkey,
    pub creator: Pubkey,
    pub taker: Pubkey,
    pub bet_id: u64,
    pub winner: Pubkey,
    pub token_mint: Pubkey,
    pub direction: u8,
    pub target_mcap_usd: u64,
    pub final_mcap_usd: u64,
    pub stake: u64,
    pub fee: u64,
    pub payout: u64,
    pub settled_at: i64,
}

#[event]
pub struct BetRefunded {
    pub bet: Pubkey,
    pub creator: Pubkey,
    pub taker: Pubkey,
    pub bet_id: u64,
    pub stake: u64,
    pub refunded_at: i64,
}
