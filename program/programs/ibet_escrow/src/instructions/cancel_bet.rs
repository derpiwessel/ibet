use anchor_lang::prelude::*;

use crate::{
    constants::*,
    error::EscrowError,
    events::BetCancelled,
    state::{Bet, BetStatus},
};

/// Pulling an untaken bet. Deliberately does not look at `config.paused`: a
/// pause must never trap a stake that nobody has matched.
#[derive(Accounts)]
pub struct CancelBet<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(
        mut,
        seeds = [BET_SEED, bet.creator.as_ref(), &bet.bet_id.to_le_bytes()],
        bump = bet.bump,
        has_one = creator @ EscrowError::NotCreator,
        close = creator,
    )]
    pub bet: Account<'info, Bet>,
}

pub fn handle_cancel_bet(ctx: Context<CancelBet>) -> Result<()> {
    require!(
        ctx.accounts.bet.status == BetStatus::Open,
        EscrowError::BetNotOpen
    );

    let bet = &mut ctx.accounts.bet;
    bet.status = BetStatus::Cancelled;

    emit!(BetCancelled {
        bet: bet.key(),
        creator: bet.creator,
        bet_id: bet.bet_id,
        refunded: bet.stake,
    });

    // `close = creator` hands back the stake together with the account's rent.
    Ok(())
}
