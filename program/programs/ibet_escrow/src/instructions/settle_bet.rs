use anchor_lang::prelude::*;

use crate::{
    constants::*,
    error::EscrowError,
    events::BetSettled,
    state::{Bet, BetStatus, Config},
    utils::move_lamports,
};

/// Settling a matched bet. Every money destination is pinned to state we stored
/// ourselves — the creator and taker on the bet, the fee wallet on the config —
/// so the resolver cannot redirect a payout by passing different accounts.
///
/// Not gated on `config.paused`: a pause must never trap a matched pot.
#[derive(Accounts)]
pub struct SettleBet<'info> {
    pub resolver: Signer<'info>,
    #[account(
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = resolver @ EscrowError::NotResolver,
    )]
    pub config: Account<'info, Config>,
    #[account(
        mut,
        seeds = [BET_SEED, bet.creator.as_ref(), &bet.bet_id.to_le_bytes()],
        bump = bet.bump,
        close = creator,
    )]
    pub bet: Account<'info, Bet>,
    /// CHECK: pinned to the creator stored on the bet; receives the rent back
    /// and, if their call was right, the payout.
    #[account(mut, address = bet.creator @ EscrowError::CreatorMismatch)]
    pub creator: UncheckedAccount<'info>,
    /// CHECK: pinned to the taker stored on the bet.
    #[account(
        mut,
        constraint = bet.taker == Some(taker.key()) @ EscrowError::TakerMismatch,
    )]
    pub taker: UncheckedAccount<'info>,
    /// CHECK: pinned to the fee wallet stored on the config.
    #[account(mut, address = config.fee_wallet @ EscrowError::FeeWalletMismatch)]
    pub fee_wallet: UncheckedAccount<'info>,
}

pub fn handle_settle_bet(ctx: Context<SettleBet>, final_mcap_usd: u64) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let grace_secs = ctx.accounts.config.grace_secs;
    let fee_bps = ctx.accounts.config.fee_bps;

    let (status, expires_at, stake) = {
        let bet = &ctx.accounts.bet;
        (bet.status, bet.expires_at, bet.stake)
    };

    require!(status == BetStatus::Matched, EscrowError::BetNotMatched);
    require!(now >= expires_at, EscrowError::NotYetExpired);
    let deadline = expires_at
        .checked_add(grace_secs)
        .ok_or(EscrowError::MathOverflow)?;
    require!(now <= deadline, EscrowError::GracePeriodOver);

    let pot = stake.checked_mul(2).ok_or(EscrowError::MathOverflow)?;
    let fee = u64::try_from(
        (pot as u128)
            .checked_mul(fee_bps as u128)
            .ok_or(EscrowError::MathOverflow)?
            .checked_div(BPS_DENOMINATOR)
            .ok_or(EscrowError::MathOverflow)?,
    )
    .map_err(|_| EscrowError::MathOverflow)?;
    let payout = pot.checked_sub(fee).ok_or(EscrowError::MathOverflow)?;

    let creator_wins = ctx.accounts.bet.creator_wins(final_mcap_usd)?;
    let bet_info = ctx.accounts.bet.to_account_info();
    let winner_info = if creator_wins {
        ctx.accounts.creator.to_account_info()
    } else {
        ctx.accounts.taker.to_account_info()
    };
    let winner_key = winner_info.key();

    // The escrow still has to hold both stakes on top of its rent.
    require!(
        bet_info.lamports() >= pot,
        EscrowError::InsufficientEscrow
    );

    move_lamports(&bet_info, &ctx.accounts.fee_wallet.to_account_info(), fee)?;
    move_lamports(&bet_info, &winner_info, payout)?;

    let bet = &mut ctx.accounts.bet;
    bet.status = BetStatus::Settled;
    bet.winner = Some(winner_key);
    bet.final_mcap_usd = final_mcap_usd;

    emit!(BetSettled {
        bet: bet.key(),
        creator: bet.creator,
        taker: ctx.accounts.taker.key(),
        bet_id: bet.bet_id,
        winner: winner_key,
        token_mint: bet.token_mint,
        direction: bet.direction,
        target_mcap_usd: bet.target_mcap_usd,
        final_mcap_usd,
        stake,
        fee,
        payout,
        settled_at: now,
    });

    // `close = creator` returns the account's rent to whoever paid it.
    Ok(())
}
