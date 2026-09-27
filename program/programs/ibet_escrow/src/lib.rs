//! ibet escrow — two people stake SOL against each other on where a Solana
//! coin's market cap lands by a deadline. The bet's own PDA holds both stakes
//! until it is settled, cancelled or refunded.

pub mod constants;
pub mod error;
pub mod events;
pub mod instructions;
pub mod state;
pub mod utils;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("GACUebjQB1zZWuQW3pkhQLzYJL9yMtU8dX4TsfRyugq1");

#[program]
pub mod ibet_escrow {
    use super::*;

    /// Creates the singleton config. The signer becomes the admin.
    pub fn initialize_config(ctx: Context<InitializeConfig>, args: InitConfigArgs) -> Result<()> {
        instructions::initialize_config::handle_initialize_config(ctx, args)
    }

    /// Admin-only. Any field left as `None` keeps its stored value.
    pub fn update_config(ctx: Context<UpdateConfig>, args: UpdateConfigArgs) -> Result<()> {
        instructions::update_config::handle_update_config(ctx, args)
    }

    /// Opens a bet and escrows the creator's stake.
    pub fn create_bet(ctx: Context<CreateBet>, args: CreateBetArgs) -> Result<()> {
        instructions::create_bet::handle_create_bet(ctx, args)
    }

    /// Matches an open bet, escrows the taker's stake and starts the clock.
    pub fn take_bet(ctx: Context<TakeBet>) -> Result<()> {
        instructions::take_bet::handle_take_bet(ctx)
    }

    /// Creator pulls a bet nobody took, getting their stake back.
    pub fn cancel_bet(ctx: Context<CancelBet>) -> Result<()> {
        instructions::cancel_bet::handle_cancel_bet(ctx)
    }

    /// Resolver-only, inside the grace window: pays the fee and the pot out.
    pub fn settle_bet(ctx: Context<SettleBet>, final_mcap_usd: u64) -> Result<()> {
        instructions::settle_bet::handle_settle_bet(ctx, final_mcap_usd)
    }

    /// Anyone, once the grace window has passed: both sides get their stake back.
    pub fn refund_expired(ctx: Context<RefundExpired>) -> Result<()> {
        instructions::refund_expired::handle_refund_expired(ctx)
    }
}
