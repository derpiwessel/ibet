use anchor_lang::prelude::*;

use crate::{
    constants::*,
    error::EscrowError,
    events::BetSettled,
    state::{Bet, BetStatus, Config, Outcome},
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
        mut,
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

/// Bets are won by touching the target, not by where the price ends up, so the
/// resolver reports *which* side won and hands over the evidence for it.
///
/// The program has no price history of its own, so it checks what it can:
///
/// - A creator win must come with a market cap that actually reaches the
///   target, observed between `taken_at` and `expires_at`. It may be settled
///   the moment that happens — no waiting for the deadline.
/// - A taker win only means "never touched", which cannot be proven on chain
///   and cannot be known before the deadline, so it is refused until then.
///
/// Either way the resolver only has until `expires_at + grace`; after that the
/// bet can only be refunded.
pub fn handle_settle_bet(
    ctx: Context<SettleBet>,
    outcome: Outcome,
    observed_mcap_usd: u64,
    observed_at: i64,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let grace_secs = ctx.accounts.config.grace_secs;
    let fee_bps = ctx.accounts.config.fee_bps;

    let (status, taken_at, expires_at, stake) = {
        let bet = &ctx.accounts.bet;
        (bet.status, bet.taken_at, bet.expires_at, bet.stake)
    };

    require!(status == BetStatus::Matched, EscrowError::BetNotMatched);

    let deadline = expires_at
        .checked_add(grace_secs)
        .ok_or(EscrowError::MathOverflow)?;
    require!(now <= deadline, EscrowError::GracePeriodOver);

    match outcome {
        Outcome::CreatorWins => {
            // The touch has to have happened inside the bet's own window.
            require!(
                observed_at >= taken_at && observed_at <= expires_at,
                EscrowError::ObservedOutsideWindow
            );
            require!(
                ctx.accounts.bet.touches_target(observed_mcap_usd)?,
                EscrowError::ObservedDoesNotReachTarget
            );
        }
        Outcome::TakerWins => {
            require!(now >= expires_at, EscrowError::TakerCannotWinYet);
        }
    }

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

    let bet_info = ctx.accounts.bet.to_account_info();
    let winner_info = match outcome {
        Outcome::CreatorWins => ctx.accounts.creator.to_account_info(),
        Outcome::TakerWins => ctx.accounts.taker.to_account_info(),
    };
    let winner_key = winner_info.key();

    // The escrow still has to hold both stakes on top of its rent.
    require!(bet_info.lamports() >= pot, EscrowError::InsufficientEscrow);

    move_lamports(&bet_info, &ctx.accounts.fee_wallet.to_account_info(), fee)?;
    move_lamports(&bet_info, &winner_info, payout)?;

    ctx.accounts.config.release_exposure(pot);

    let bet = &mut ctx.accounts.bet;
    bet.status = BetStatus::Settled;
    bet.winner = Some(winner_key);
    bet.observed_mcap_usd = observed_mcap_usd;
    bet.observed_at = observed_at;

    emit!(BetSettled {
        bet: bet.key(),
        creator: bet.creator,
        taker: ctx.accounts.taker.key(),
        bet_id: bet.bet_id,
        winner: winner_key,
        token_mint: bet.token_mint,
        direction: bet.direction,
        target_mcap_usd: bet.target_mcap_usd,
        observed_mcap_usd,
        observed_at,
        stake,
        fee,
        payout,
        settled_at: now,
    });

    // `close = creator` returns the account's rent to whoever paid it.
    Ok(())
}
