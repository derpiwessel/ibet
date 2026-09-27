use anchor_lang::prelude::*;

use crate::{
    constants::*,
    error::EscrowError,
    events::BetTaken,
    state::{Bet, BetStatus, Config},
};

#[derive(Accounts)]
pub struct TakeBet<'info> {
    #[account(mut)]
    pub taker: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(
        mut,
        seeds = [BET_SEED, bet.creator.as_ref(), &bet.bet_id.to_le_bytes()],
        bump = bet.bump,
    )]
    pub bet: Account<'info, Bet>,
    pub system_program: Program<'info, System>,
}

pub fn handle_take_bet(ctx: Context<TakeBet>) -> Result<()> {
    require!(!ctx.accounts.config.paused, EscrowError::Paused);
    require!(
        ctx.accounts.bet.status == BetStatus::Open,
        EscrowError::BetNotOpen
    );
    require_keys_neq!(
        ctx.accounts.taker.key(),
        ctx.accounts.bet.creator,
        EscrowError::CannotTakeOwnBet
    );

    let stake = ctx.accounts.bet.stake;

    // Escrow the taker's matching stake.
    anchor_lang::system_program::transfer(
        CpiContext::new(
            anchor_lang::system_program::ID,
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.taker.to_account_info(),
                to: ctx.accounts.bet.to_account_info(),
            },
        ),
        stake,
    )?;

    let now = Clock::get()?.unix_timestamp;
    let bet = &mut ctx.accounts.bet;
    bet.taker = Some(ctx.accounts.taker.key());
    bet.taken_at = now;
    bet.expires_at = now
        .checked_add(bet.duration_secs)
        .ok_or(EscrowError::MathOverflow)?;
    bet.status = BetStatus::Matched;

    emit!(BetTaken {
        bet: bet.key(),
        creator: bet.creator,
        taker: ctx.accounts.taker.key(),
        bet_id: bet.bet_id,
        taken_at: bet.taken_at,
        expires_at: bet.expires_at,
    });
    Ok(())
}
