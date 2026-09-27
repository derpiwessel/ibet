use anchor_lang::prelude::*;

use crate::{constants::*, error::EscrowError, events::ConfigUpdated, state::Config};

/// Every field is optional: `None` leaves the stored value alone.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct UpdateConfigArgs {
    pub admin: Option<Pubkey>,
    pub resolver: Option<Pubkey>,
    pub fee_wallet: Option<Pubkey>,
    pub fee_bps: Option<u16>,
    pub min_stake: Option<u64>,
    pub max_stake: Option<u64>,
    pub grace_secs: Option<i64>,
    pub paused: Option<bool>,
}

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    pub admin: Signer<'info>,
    #[account(
        mut,
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin @ EscrowError::NotAdmin,
    )]
    pub config: Account<'info, Config>,
}

pub fn handle_update_config(ctx: Context<UpdateConfig>, args: UpdateConfigArgs) -> Result<()> {
    let config = &mut ctx.accounts.config;

    if let Some(v) = args.admin {
        config.admin = v;
    }
    if let Some(v) = args.resolver {
        config.resolver = v;
    }
    if let Some(v) = args.fee_wallet {
        config.fee_wallet = v;
    }
    if let Some(v) = args.fee_bps {
        config.fee_bps = v;
    }
    if let Some(v) = args.min_stake {
        config.min_stake = v;
    }
    if let Some(v) = args.max_stake {
        config.max_stake = v;
    }
    if let Some(v) = args.grace_secs {
        config.grace_secs = v;
    }
    if let Some(v) = args.paused {
        config.paused = v;
    }

    // Re-check the invariants against the merged result, not just the new values.
    require!(config.fee_bps <= MAX_FEE_BPS, EscrowError::FeeTooHigh);
    require!(config.min_stake > 0, EscrowError::InvalidStakeLimits);
    require!(
        config.max_stake >= config.min_stake,
        EscrowError::InvalidStakeLimits
    );
    require!(config.grace_secs > 0, EscrowError::InvalidGrace);

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
