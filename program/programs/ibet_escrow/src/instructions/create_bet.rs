use anchor_lang::prelude::*;

use crate::{
    constants::*,
    error::EscrowError,
    events::BetCreated,
    state::{Bet, BetStatus, Config},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CreateBetArgs {
    /// Client-chosen id; part of the bet's PDA seeds, so it must be unused for
    /// this creator.
    pub bet_id: u64,
    /// The coin's contract address.
    pub token_mint: Pubkey,
    pub direction: u8,
    pub target_mcap_usd: u64,
    /// Market cap at creation time, or 0 when it was not available.
    pub start_mcap_usd: u64,
    /// Lamports staked by each side.
    pub stake: u64,
    pub duration_secs: i64,
}

#[derive(Accounts)]
#[instruction(args: CreateBetArgs)]
pub struct CreateBet<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    /// Holds the escrowed lamports on top of its own rent.
    #[account(
        init,
        payer = creator,
        space = 8 + Bet::INIT_SPACE,
        seeds = [BET_SEED, creator.key().as_ref(), &args.bet_id.to_le_bytes()],
        bump,
    )]
    pub bet: Account<'info, Bet>,
    pub system_program: Program<'info, System>,
}

pub fn handle_create_bet(ctx: Context<CreateBet>, args: CreateBetArgs) -> Result<()> {
    let config = &ctx.accounts.config;
    require!(!config.paused, EscrowError::Paused);
    require!(
        args.direction == DIRECTION_HIGHER || args.direction == DIRECTION_LOWER,
        EscrowError::InvalidDirection
    );
    require!(
        args.stake >= config.min_stake && args.stake <= config.max_stake,
        EscrowError::StakeOutOfRange
    );
    require!(
        is_allowed_duration(args.duration_secs),
        EscrowError::InvalidDuration
    );
    require!(args.target_mcap_usd > 0, EscrowError::InvalidTarget);

    // Only checked when we know where the coin started: a Higher bet has to aim
    // above that, a Lower bet below it. A start of 0 means "not available".
    if args.start_mcap_usd > 0 {
        let consistent = match args.direction {
            DIRECTION_HIGHER => args.target_mcap_usd > args.start_mcap_usd,
            _ => args.target_mcap_usd < args.start_mcap_usd,
        };
        require!(consistent, EscrowError::TargetContradictsDirection);
    }

    let now = Clock::get()?.unix_timestamp;

    // Escrow the creator's stake in the bet account.
    anchor_lang::system_program::transfer(
        CpiContext::new(
            anchor_lang::system_program::ID,
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.creator.to_account_info(),
                to: ctx.accounts.bet.to_account_info(),
            },
        ),
        args.stake,
    )?;

    let bet = &mut ctx.accounts.bet;
    bet.creator = ctx.accounts.creator.key();
    bet.taker = None;
    bet.token_mint = args.token_mint;
    bet.direction = args.direction;
    bet.target_mcap_usd = args.target_mcap_usd;
    bet.start_mcap_usd = args.start_mcap_usd;
    bet.stake = args.stake;
    bet.duration_secs = args.duration_secs;
    bet.created_at = now;
    bet.taken_at = 0;
    // The clock only starts when someone takes the bet.
    bet.expires_at = 0;
    bet.status = BetStatus::Open;
    bet.winner = None;
    bet.observed_mcap_usd = 0;
    bet.observed_at = 0;
    bet.bet_id = args.bet_id;
    bet.bump = ctx.bumps.bet;

    emit!(BetCreated {
        bet: bet.key(),
        creator: bet.creator,
        bet_id: bet.bet_id,
        token_mint: bet.token_mint,
        direction: bet.direction,
        target_mcap_usd: bet.target_mcap_usd,
        start_mcap_usd: bet.start_mcap_usd,
        stake: bet.stake,
        duration_secs: bet.duration_secs,
        created_at: bet.created_at,
    });
    Ok(())
}
