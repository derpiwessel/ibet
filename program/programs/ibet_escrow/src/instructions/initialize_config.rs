use anchor_lang::prelude::*;

use crate::{constants::*, error::EscrowError, events::ConfigUpdated, state::Config};

/// Everything the config needs at birth. The admin is the signer, so it is not
/// listed here.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct InitConfigArgs {
    pub resolver: Pubkey,
    pub fee_wallet: Pubkey,
    pub fee_bps: u16,
    pub min_stake: u64,
    pub max_stake: u64,
    pub grace_secs: i64,
    pub max_open_exposure: u64,
}

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    /// Pays for the config account and becomes its admin.
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        init,
        payer = admin,
        space = 8 + Config::INIT_SPACE,
        seeds = [CONFIG_SEED],
        bump,
    )]
    pub config: Account<'info, Config>,
    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_config(
    ctx: Context<InitializeConfig>,
    args: InitConfigArgs,
) -> Result<()> {
    require!(args.fee_bps <= MAX_FEE_BPS, EscrowError::FeeTooHigh);
    require!(args.min_stake > 0, EscrowError::InvalidStakeLimits);
    require!(
        args.max_stake >= args.min_stake,
        EscrowError::InvalidStakeLimits
    );
    require!(args.grace_secs > 0, EscrowError::InvalidGrace);
    // The ceiling has to fit at least one whole bet, or nothing could ever be
    // created and the program would be dead on arrival.
    require!(
        args.max_open_exposure >= args.max_stake.saturating_mul(2),
        EscrowError::InvalidExposureCap
    );

    let config = &mut ctx.accounts.config;
    config.admin = ctx.accounts.admin.key();
    config.resolver = args.resolver;
    config.fee_wallet = args.fee_wallet;
    config.fee_bps = args.fee_bps;
    config.min_stake = args.min_stake;
    config.max_stake = args.max_stake;
    config.grace_secs = args.grace_secs;
    config.paused = false;
    config.bump = ctx.bumps.config;
    config.open_exposure = 0;
    config.max_open_exposure = args.max_open_exposure;

    emit!(ConfigUpdated {
        admin: config.admin,
        resolver: config.resolver,
        fee_wallet: config.fee_wallet,
        fee_bps: config.fee_bps,
        min_stake: config.min_stake,
        max_stake: config.max_stake,
        grace_secs: config.grace_secs,
        paused: config.paused,
    });
    Ok(())
}
