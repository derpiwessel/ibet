use anchor_lang::prelude::*;

use crate::{
    constants::*,
    error::EscrowError,
    events::BetRefunded,
    state::{Bet, BetStatus, Config},
    utils::move_lamports,
};

/// The escape hatch: once the resolver's grace period has passed without a
/// settlement, anyone may unwind the bet and both sides get their own stake
/// back. No fee is taken on a refund.
#[derive(Accounts)]
pub struct RefundExpired<'info> {
    /// Anyone — they only pay the transaction fee.
    pub caller: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(
        mut,
        seeds = [BET_SEED, bet.creator.as_ref(), &bet.bet_id.to_le_bytes()],
        bump = bet.bump,
        close = creator,
    )]
    pub bet: Account<'info, Bet>,
    /// CHECK: pinned to the creator stored on the bet.
    #[account(mut, address = bet.creator @ EscrowError::CreatorMismatch)]
    pub creator: UncheckedAccount<'info>,
    /// CHECK: pinned to the taker stored on the bet.
    #[account(
        mut,
        constraint = bet.taker == Some(taker.key()) @ EscrowError::TakerMismatch,
    )]
    pub taker: UncheckedAccount<'info>,
}

pub fn handle_refund_expired(ctx: Context<RefundExpired>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let grace_secs = ctx.accounts.config.grace_secs;

    let (status, expires_at, stake) = {
        let bet = &ctx.accounts.bet;
        (bet.status, bet.expires_at, bet.stake)
    };

    require!(status == BetStatus::Matched, EscrowError::BetNotMatched);
    let deadline = expires_at
        .checked_add(grace_secs)
        .ok_or(EscrowError::MathOverflow)?;
    require!(now > deadline, EscrowError::GraceNotOver);

    let bet_info = ctx.accounts.bet.to_account_info();
    require!(
        bet_info.lamports() >= stake,
        EscrowError::InsufficientEscrow
    );

    // The taker is paid out directly; the creator's stake rides along with the
    // rent when `close = creator` empties the account.
    move_lamports(&bet_info, &ctx.accounts.taker.to_account_info(), stake)?;

    let bet = &mut ctx.accounts.bet;
    bet.status = BetStatus::Refunded;

    emit!(BetRefunded {
        bet: bet.key(),
        creator: bet.creator,
        taker: ctx.accounts.taker.key(),
        bet_id: bet.bet_id,
        stake,
        refunded_at: now,
    });
    Ok(())
}
