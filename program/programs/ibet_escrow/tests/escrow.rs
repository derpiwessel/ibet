//! Behaviour and safety tests for `ibet_escrow`.
//!
//! Run with `cargo test` from `program/`, after `anchor build --arch v0` has
//! produced `target/deploy/ibet_escrow.so` — the tests load that exact artifact.

mod common;

use {
    anchor_lang::{prelude::Pubkey, Space},
    common::*,
    ibet_escrow::{
        error::EscrowError,
        instructions::{CreateBetArgs, UpdateConfigArgs},
        state::{Bet, BetStatus},
    },
    litesvm::types::TransactionMetadata,
    solana_keypair::Keypair,
    solana_signer::Signer,
};

/// Anchor emits events as a `Program data:` log line.
fn assert_event_emitted(meta: &TransactionMetadata) {
    assert!(
        meta.logs.iter().any(|l| l.contains("Program data:")),
        "expected an event to be emitted, logs: {:?}",
        meta.logs
    );
}

// ── config ────────────────────────────────────────────────────────────────

#[test]
fn config_is_initialized_with_the_phase_1_values() {
    let env = Env::new();
    let account = env.svm.get_account(&env.config).unwrap();
    let mut data: &[u8] = &account.data;
    let config = <ibet_escrow::state::Config as anchor_lang::AccountDeserialize>::try_deserialize(
        &mut data,
    )
    .unwrap();

    assert_eq!(config.admin, env.admin.pubkey());
    assert_eq!(config.resolver, env.resolver.pubkey());
    assert_eq!(config.fee_wallet, env.fee_wallet.pubkey());
    assert_eq!(config.fee_bps, 200);
    assert_eq!(config.min_stake, MIN_STAKE);
    assert_eq!(config.max_stake, MAX_STAKE);
    assert_eq!(config.grace_secs, 48 * 3_600);
    assert!(!config.paused);
}

#[test]
fn only_the_admin_can_update_the_config() {
    let mut env = Env::new();
    let stranger = Keypair::new();
    env.svm.airdrop(&stranger.pubkey(), SOL).unwrap();

    let ix = env.ix_update_config(
        &stranger.pubkey(),
        UpdateConfigArgs {
            fee_bps: Some(0),
            ..Default::default()
        },
    );
    assert_escrow_err(env.send(&[ix], &[&stranger]), EscrowError::NotAdmin);
}

#[test]
fn the_fee_ceiling_cannot_be_exceeded() {
    let mut env = Env::new();
    let admin = env.admin.insecure_clone();
    let ix = env.ix_update_config(
        &admin.pubkey(),
        UpdateConfigArgs {
            fee_bps: Some(501),
            ..Default::default()
        },
    );
    assert_escrow_err(env.send(&[ix], &[&admin]), EscrowError::FeeTooHigh);
}

#[test]
fn updating_one_config_field_leaves_the_rest_alone() {
    let mut env = Env::new();
    let admin = env.admin.insecure_clone();
    let ix = env.ix_update_config(
        &admin.pubkey(),
        UpdateConfigArgs {
            grace_secs: Some(24 * 3_600),
            ..Default::default()
        },
    );
    env.send(&[ix], &[&admin]).unwrap();

    let account = env.svm.get_account(&env.config).unwrap();
    let mut data: &[u8] = &account.data;
    let config = <ibet_escrow::state::Config as anchor_lang::AccountDeserialize>::try_deserialize(
        &mut data,
    )
    .unwrap();
    assert_eq!(config.grace_secs, 24 * 3_600);
    assert_eq!(config.fee_bps, 200, "fee should not have moved");
    assert_eq!(config.resolver, env.resolver.pubkey());
    assert_eq!(config.min_stake, MIN_STAKE);
}

// ── create ────────────────────────────────────────────────────────────────

#[test]
fn creating_a_bet_escrows_the_stake() {
    let mut env = Env::new();
    let creator = env.creator.pubkey();
    let rent = env.bet_rent();
    let before = env.bal(&creator);

    let bet_pda = env.open_bet(1);
    let bet = env.bet(&bet_pda);

    assert_eq!(bet.creator, creator);
    assert_eq!(bet.taker, None);
    assert_eq!(bet.status, BetStatus::Open);
    assert_eq!(bet.stake, SOL);
    assert_eq!(bet.duration_secs, 7 * DAY);
    assert_eq!(bet.created_at, BASE_TIME);
    assert_eq!(bet.taken_at, 0);
    assert_eq!(bet.expires_at, 0, "the clock only starts on take");
    assert_eq!(bet.bet_id, 1);

    assert_eq!(env.bal(&bet_pda), rent + SOL);
    assert_eq!(env.bal(&creator), before - rent - SOL);
}

#[test]
fn the_bet_account_is_exactly_as_big_as_its_layout() {
    let mut env = Env::new();
    let bet_pda = env.open_bet(1);
    let account = env.svm.get_account(&bet_pda).unwrap();
    assert_eq!(account.data.len(), 8 + Bet::INIT_SPACE);
}

#[test]
fn the_account_sizes_the_frontend_hardcodes_still_hold() {
    // index.html filters `getProgramAccounts` on these byte sizes and decodes
    // the fields by hand, so a layout change has to fail here first.
    assert_eq!(8 + Bet::INIT_SPACE, 213, "BET_SIZE in index.html");
    assert_eq!(
        8 + ibet_escrow::state::Config::INIT_SPACE,
        132,
        "CONFIG_SIZE in index.html"
    );
}

#[test]
fn a_stake_below_the_minimum_is_rejected() {
    let mut env = Env::new();
    let creator = env.creator.insecure_clone();
    let args = CreateBetArgs {
        stake: MIN_STAKE - 1,
        ..env.default_args(1)
    };
    let ix = env.ix_create(&creator.pubkey(), args);
    assert_escrow_err(env.send(&[ix], &[&creator]), EscrowError::StakeOutOfRange);
}

#[test]
fn a_stake_above_the_maximum_is_rejected() {
    let mut env = Env::new();
    let creator = env.creator.insecure_clone();
    let args = CreateBetArgs {
        stake: MAX_STAKE + 1,
        ..env.default_args(1)
    };
    let ix = env.ix_create(&creator.pubkey(), args);
    assert_escrow_err(env.send(&[ix], &[&creator]), EscrowError::StakeOutOfRange);
}

#[test]
fn a_duration_outside_the_whitelist_is_rejected() {
    let mut env = Env::new();
    let creator = env.creator.insecure_clone();
    for duration in [2 * DAY, 6 * DAY, 31 * DAY, 0, -7 * DAY, 7 * DAY + 1] {
        let args = CreateBetArgs {
            duration_secs: duration,
            ..env.default_args(1)
        };
        let ix = env.ix_create(&creator.pubkey(), args);
        assert_escrow_err(env.send(&[ix], &[&creator]), EscrowError::InvalidDuration);
    }
}

#[test]
fn every_whitelisted_duration_is_accepted() {
    let mut env = Env::new();
    for (i, days) in [1i64, 3, 5, 7, 14, 30].iter().enumerate() {
        let args = CreateBetArgs {
            duration_secs: days * DAY,
            ..env.default_args(100 + i as u64)
        };
        let bet_pda = env.open_bet_with(args);
        assert_eq!(env.bet(&bet_pda).duration_secs, days * DAY);
    }
}

#[test]
fn a_zero_target_is_rejected() {
    let mut env = Env::new();
    let creator = env.creator.insecure_clone();
    let args = CreateBetArgs {
        target_mcap_usd: 0,
        ..env.default_args(1)
    };
    let ix = env.ix_create(&creator.pubkey(), args);
    assert_escrow_err(env.send(&[ix], &[&creator]), EscrowError::InvalidTarget);
}

#[test]
fn an_unknown_direction_is_rejected() {
    let mut env = Env::new();
    let creator = env.creator.insecure_clone();
    let args = CreateBetArgs {
        direction: 2,
        ..env.default_args(1)
    };
    let ix = env.ix_create(&creator.pubkey(), args);
    assert_escrow_err(env.send(&[ix], &[&creator]), EscrowError::InvalidDirection);
}

#[test]
fn the_target_has_to_agree_with_the_direction() {
    let mut env = Env::new();
    let creator = env.creator.insecure_clone();

    // Higher, but the target sits below the starting market cap.
    let args = CreateBetArgs {
        direction: HIGHER,
        start_mcap_usd: 1_000_000,
        target_mcap_usd: 500_000,
        ..env.default_args(1)
    };
    let ix = env.ix_create(&creator.pubkey(), args);
    assert_escrow_err(
        env.send(&[ix], &[&creator]),
        EscrowError::TargetContradictsDirection,
    );

    // Lower, but the target sits above it.
    let args = CreateBetArgs {
        direction: LOWER,
        start_mcap_usd: 1_000_000,
        target_mcap_usd: 2_000_000,
        ..env.default_args(2)
    };
    let ix = env.ix_create(&creator.pubkey(), args);
    assert_escrow_err(
        env.send(&[ix], &[&creator]),
        EscrowError::TargetContradictsDirection,
    );

    // Equal to the start is not "higher" either.
    let args = CreateBetArgs {
        direction: HIGHER,
        start_mcap_usd: 1_000_000,
        target_mcap_usd: 1_000_000,
        ..env.default_args(3)
    };
    let ix = env.ix_create(&creator.pubkey(), args);
    assert_escrow_err(
        env.send(&[ix], &[&creator]),
        EscrowError::TargetContradictsDirection,
    );
}

#[test]
fn without_a_starting_market_cap_any_target_is_allowed() {
    let mut env = Env::new();
    let args = CreateBetArgs {
        direction: HIGHER,
        start_mcap_usd: 0,
        target_mcap_usd: 1,
        ..env.default_args(1)
    };
    let bet_pda = env.open_bet_with(args);
    assert_eq!(env.bet(&bet_pda).start_mcap_usd, 0);
}

#[test]
fn a_bet_id_cannot_be_reused_by_the_same_creator() {
    let mut env = Env::new();
    env.open_bet(1);

    let creator = env.creator.insecure_clone();
    let args = env.default_args(1);
    let ix = env.ix_create(&creator.pubkey(), args);
    assert_failed(env.send(&[ix], &[&creator]));
}

// ── take ──────────────────────────────────────────────────────────────────

#[test]
fn taking_a_bet_escrows_the_second_stake_and_starts_the_clock() {
    let mut env = Env::new();
    let taker = env.taker.pubkey();
    let rent = env.bet_rent();
    let before = env.bal(&taker);

    let bet_pda = env.matched_bet(1);
    let bet = env.bet(&bet_pda);

    assert_eq!(bet.status, BetStatus::Matched);
    assert_eq!(bet.taker, Some(taker));
    assert_eq!(bet.taken_at, BASE_TIME);
    assert_eq!(bet.expires_at, BASE_TIME + 7 * DAY);
    assert_eq!(env.bal(&bet_pda), rent + 2 * SOL);
    assert_eq!(env.bal(&taker), before - SOL);
}

#[test]
fn you_cannot_take_your_own_bet() {
    let mut env = Env::new();
    let bet_pda = env.open_bet(1);
    let creator = env.creator.insecure_clone();
    let ix = env.ix_take(&creator.pubkey(), &bet_pda);
    assert_escrow_err(env.send(&[ix], &[&creator]), EscrowError::CannotTakeOwnBet);
}

#[test]
fn a_bet_cannot_be_taken_twice() {
    let mut env = Env::new();
    let bet_pda = env.matched_bet(1);

    let second = Keypair::new();
    env.svm.airdrop(&second.pubkey(), 10 * SOL).unwrap();
    let ix = env.ix_take(&second.pubkey(), &bet_pda);
    assert_escrow_err(env.send(&[ix], &[&second]), EscrowError::BetNotOpen);
}

// ── cancel ────────────────────────────────────────────────────────────────

#[test]
fn cancelling_returns_the_stake_and_the_rent() {
    let mut env = Env::new();
    let creator_key = env.creator.pubkey();
    let before = env.bal(&creator_key);

    let bet_pda = env.open_bet(1);
    let creator = env.creator.insecure_clone();
    let ix = env.ix_cancel(&creator.pubkey(), &bet_pda);
    let meta = env.send(&[ix], &[&creator]).unwrap();
    assert_event_emitted(&meta);

    assert_eq!(env.bal(&creator_key), before, "creator is made whole");
    assert_eq!(env.bal(&bet_pda), 0, "bet account is closed");
}

#[test]
fn a_taken_bet_cannot_be_cancelled() {
    let mut env = Env::new();
    let bet_pda = env.matched_bet(1);
    let creator = env.creator.insecure_clone();
    let ix = env.ix_cancel(&creator.pubkey(), &bet_pda);
    assert_escrow_err(env.send(&[ix], &[&creator]), EscrowError::BetNotOpen);
}

#[test]
fn only_the_creator_can_cancel() {
    let mut env = Env::new();
    let bet_pda = env.open_bet(1);
    let stranger = Keypair::new();
    env.svm.airdrop(&stranger.pubkey(), SOL).unwrap();

    // The seeds are derived from the bet's stored creator, so a stranger
    // signing in the creator's slot cannot match the PDA.
    let ix = env.ix_cancel(&stranger.pubkey(), &bet_pda);
    assert_failed(env.send(&[ix], &[&stranger]));
}

// ── settle ────────────────────────────────────────────────────────────────

#[test]
fn settling_pays_the_creator_when_their_call_was_right() {
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let (c0, t0, f0) = (env.bal(&creator), env.bal(&taker), env.bal(&fee_wallet));
    let total_before = c0 + t0 + f0;

    let bet_pda = env.matched_bet(1);
    env.advance(7 * DAY);

    let resolver = env.resolver.insecure_clone();
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &creator,
        &taker,
        &fee_wallet,
        6_000_000, // above the 5M target
    );
    let meta = env.send(&[ix], &[&resolver]).unwrap();
    assert_event_emitted(&meta);

    // 2 SOL pot, 2% fee: 0.04 SOL to the fee wallet, 1.96 SOL to the winner.
    let fee = 40_000_000;
    let payout = 1_960_000_000;
    assert_eq!(env.bal(&fee_wallet), f0 + fee);
    assert_eq!(env.bal(&creator), c0 - SOL + payout);
    assert_eq!(env.bal(&taker), t0 - SOL);
    assert_eq!(env.bal(&bet_pda), 0, "bet account is closed");
    assert_eq!(
        env.escrow_total(&bet_pda),
        total_before,
        "not a lamport created or destroyed"
    );
}

#[test]
fn settling_pays_the_taker_when_the_call_was_wrong() {
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let (c0, t0, f0) = (env.bal(&creator), env.bal(&taker), env.bal(&fee_wallet));
    let total_before = c0 + t0 + f0;

    let bet_pda = env.matched_bet(1);
    env.advance(7 * DAY);

    let resolver = env.resolver.insecure_clone();
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &creator,
        &taker,
        &fee_wallet,
        4_999_999, // just under the 5M target
    );
    env.send(&[ix], &[&resolver]).unwrap();

    let fee = 40_000_000;
    let payout = 1_960_000_000;
    assert_eq!(env.bal(&fee_wallet), f0 + fee);
    // The creator only gets their rent back, which nets to -1 SOL.
    assert_eq!(env.bal(&creator), c0 - SOL);
    assert_eq!(env.bal(&taker), t0 - SOL + payout);
    assert_eq!(env.escrow_total(&bet_pda), total_before);
}

#[test]
fn landing_exactly_on_the_target_goes_to_the_creator() {
    // Higher: final == target counts as "at or above".
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let c0 = env.bal(&creator);

    let bet_pda = env.matched_bet(1);
    env.advance(7 * DAY);
    let resolver = env.resolver.insecure_clone();
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &creator,
        &taker,
        &fee_wallet,
        5_000_000,
    );
    env.send(&[ix], &[&resolver]).unwrap();
    assert_eq!(env.bal(&creator), c0 - SOL + 1_960_000_000);
}

#[test]
fn landing_exactly_on_the_target_goes_to_the_creator_for_lower_bets_too() {
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let c0 = env.bal(&creator);

    let args = CreateBetArgs {
        direction: LOWER,
        start_mcap_usd: 1_000_000,
        target_mcap_usd: 400_000,
        ..env.default_args(1)
    };
    let bet_pda = env.open_bet_with(args);
    env.take(&bet_pda);
    env.advance(7 * DAY);

    let resolver = env.resolver.insecure_clone();
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &creator,
        &taker,
        &fee_wallet,
        400_000,
    );
    env.send(&[ix], &[&resolver]).unwrap();
    assert_eq!(env.bal(&creator), c0 - SOL + 1_960_000_000);
}

#[test]
fn a_zero_fee_sends_the_whole_pot_to_the_winner() {
    let mut env = Env::with_fee_bps(0);
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let (c0, t0, f0) = (env.bal(&creator), env.bal(&taker), env.bal(&fee_wallet));

    let bet_pda = env.matched_bet(1);
    env.advance(7 * DAY);

    let resolver = env.resolver.insecure_clone();
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &creator,
        &taker,
        &fee_wallet,
        6_000_000,
    );
    env.send(&[ix], &[&resolver]).unwrap();

    assert_eq!(env.bal(&fee_wallet), f0, "no fee taken");
    assert_eq!(env.bal(&creator), c0 + SOL, "won the other side's stake");
    assert_eq!(env.bal(&taker), t0 - SOL);
    assert_eq!(env.escrow_total(&bet_pda), c0 + t0 + f0);
}

#[test]
fn only_the_resolver_can_settle() {
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let bet_pda = env.matched_bet(1);
    env.advance(7 * DAY);

    // Not even the admin may settle.
    for signer in [env.admin.insecure_clone(), Keypair::new()] {
        env.svm.airdrop(&signer.pubkey(), SOL).unwrap();
        let ix = env.ix_settle(
            &signer.pubkey(),
            &bet_pda,
            &creator,
            &taker,
            &fee_wallet,
            6_000_000,
        );
        assert_escrow_err(env.send(&[ix], &[&signer]), EscrowError::NotResolver);
    }
}

#[test]
fn a_bet_cannot_be_settled_before_it_expires() {
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let bet_pda = env.matched_bet(1);
    env.advance(7 * DAY - 1);

    let resolver = env.resolver.insecure_clone();
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &creator,
        &taker,
        &fee_wallet,
        6_000_000,
    );
    assert_escrow_err(env.send(&[ix], &[&resolver]), EscrowError::NotYetExpired);
}

#[test]
fn a_bet_can_still_be_settled_on_the_last_second_of_the_grace_window() {
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let bet_pda = env.matched_bet(1);
    env.advance(7 * DAY + GRACE_SECS);

    let resolver = env.resolver.insecure_clone();
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &creator,
        &taker,
        &fee_wallet,
        6_000_000,
    );
    env.send(&[ix], &[&resolver]).unwrap();
    assert_eq!(env.bal(&bet_pda), 0);
}

#[test]
fn a_bet_cannot_be_settled_after_the_grace_window() {
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let bet_pda = env.matched_bet(1);
    env.advance(7 * DAY + GRACE_SECS + 1);

    let resolver = env.resolver.insecure_clone();
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &creator,
        &taker,
        &fee_wallet,
        6_000_000,
    );
    assert_escrow_err(env.send(&[ix], &[&resolver]), EscrowError::GracePeriodOver);
}

#[test]
fn an_open_bet_cannot_be_settled() {
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let bet_pda = env.open_bet(1);
    env.advance(30 * DAY);

    let resolver = env.resolver.insecure_clone();
    // An open bet has no taker, so the taker slot cannot be satisfied at all.
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &creator,
        &taker,
        &fee_wallet,
        6_000_000,
    );
    assert_escrow_err(env.send(&[ix], &[&resolver]), EscrowError::TakerMismatch);
}

#[test]
fn the_resolver_cannot_redirect_a_payout() {
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let bet_pda = env.matched_bet(1);
    env.advance(7 * DAY);
    let resolver = env.resolver.insecure_clone();
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), SOL).unwrap();

    // Someone else in the fee wallet's slot.
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &creator,
        &taker,
        &attacker.pubkey(),
        6_000_000,
    );
    assert_escrow_err(
        env.send(&[ix], &[&resolver]),
        EscrowError::FeeWalletMismatch,
    );

    // Someone else in the taker's slot.
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &creator,
        &attacker.pubkey(),
        &fee_wallet,
        6_000_000,
    );
    assert_escrow_err(env.send(&[ix], &[&resolver]), EscrowError::TakerMismatch);

    // Someone else in the creator's slot.
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &attacker.pubkey(),
        &taker,
        &fee_wallet,
        6_000_000,
    );
    assert_escrow_err(env.send(&[ix], &[&resolver]), EscrowError::CreatorMismatch);

    // And the bet is untouched by all that.
    assert_eq!(env.bet(&bet_pda).status, BetStatus::Matched);
    assert_eq!(env.bal(&bet_pda), env.bet_rent() + 2 * SOL);
}

#[test]
fn the_fee_wallet_may_also_be_the_winner() {
    // In production the owner is admin, resolver and fee wallet at once, so the
    // same account can legitimately show up in two slots of one settlement.
    let mut env = Env::new();
    let (creator, taker) = (env.creator.pubkey(), env.taker.pubkey());
    let admin = env.admin.insecure_clone();
    let ix = env.ix_update_config(
        &admin.pubkey(),
        UpdateConfigArgs {
            fee_wallet: Some(creator),
            ..Default::default()
        },
    );
    env.send(&[ix], &[&admin]).unwrap();

    let c0 = env.bal(&creator);
    let bet_pda = env.matched_bet(1);
    env.advance(7 * DAY);

    let resolver = env.resolver.insecure_clone();
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &bet_pda,
        &creator,
        &taker,
        &creator, // fee wallet == creator == winner
        6_000_000,
    );
    env.send(&[ix], &[&resolver]).unwrap();

    // Creator receives the payout and the fee: the whole pot, less their stake.
    assert_eq!(env.bal(&creator), c0 - SOL + 2 * SOL);
    assert_eq!(env.bal(&bet_pda), 0);
}

// ── refund ────────────────────────────────────────────────────────────────

#[test]
fn after_the_grace_window_anyone_can_refund_both_sides() {
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let (c0, t0, f0) = (env.bal(&creator), env.bal(&taker), env.bal(&fee_wallet));

    let bet_pda = env.matched_bet(1);
    env.advance(7 * DAY + GRACE_SECS + 1);

    // A passer-by, not one of the players.
    let stranger = Keypair::new();
    env.svm.airdrop(&stranger.pubkey(), SOL).unwrap();
    let ix = env.ix_refund(&stranger.pubkey(), &bet_pda, &creator, &taker);
    let meta = env.send(&[ix], &[&stranger]).unwrap();
    assert_event_emitted(&meta);

    assert_eq!(env.bal(&creator), c0, "creator made whole");
    assert_eq!(env.bal(&taker), t0, "taker made whole");
    assert_eq!(env.bal(&fee_wallet), f0, "no fee on a refund");
    assert_eq!(env.bal(&bet_pda), 0);
    assert_eq!(env.escrow_total(&bet_pda), c0 + t0 + f0);
}

#[test]
fn a_bet_cannot_be_refunded_while_the_resolver_still_has_time() {
    let mut env = Env::new();
    let (creator, taker) = (env.creator.pubkey(), env.taker.pubkey());
    let bet_pda = env.matched_bet(1);
    let caller = env.payer.insecure_clone();

    // Before expiry.
    let ix = env.ix_refund(&caller.pubkey(), &bet_pda, &creator, &taker);
    assert_escrow_err(env.send(&[ix], &[]), EscrowError::GraceNotOver);

    // Expired, but still inside the grace window — and on its very last second.
    env.advance(7 * DAY + GRACE_SECS);
    let ix = env.ix_refund(&caller.pubkey(), &bet_pda, &creator, &taker);
    assert_escrow_err(env.send(&[ix], &[]), EscrowError::GraceNotOver);
}

#[test]
fn an_open_bet_cannot_be_refunded() {
    let mut env = Env::new();
    let (creator, taker) = (env.creator.pubkey(), env.taker.pubkey());
    let bet_pda = env.open_bet(1);
    env.advance(30 * DAY + GRACE_SECS + 1);

    let caller = env.payer.insecure_clone();
    let ix = env.ix_refund(&caller.pubkey(), &bet_pda, &creator, &taker);
    assert_escrow_err(env.send(&[ix], &[]), EscrowError::TakerMismatch);
}

// ── pause ─────────────────────────────────────────────────────────────────

#[test]
fn pausing_blocks_creating_a_bet() {
    let mut env = Env::new();
    env.set_paused(true);

    let creator = env.creator.insecure_clone();
    let args = env.default_args(1);
    let ix = env.ix_create(&creator.pubkey(), args);
    assert_escrow_err(env.send(&[ix], &[&creator]), EscrowError::Paused);
}

#[test]
fn pausing_blocks_taking_a_bet() {
    let mut env = Env::new();
    let bet_pda = env.open_bet(1);
    env.set_paused(true);

    let taker = env.taker.insecure_clone();
    let ix = env.ix_take(&taker.pubkey(), &bet_pda);
    assert_escrow_err(env.send(&[ix], &[&taker]), EscrowError::Paused);
}

#[test]
fn pausing_never_traps_money() {
    // Cancel, settle and refund all have to keep working while paused,
    // otherwise a pause could strand funds in escrow.
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );

    let open = env.open_bet(1);
    let to_settle = env.matched_bet(2);
    let to_refund = env.matched_bet(3);
    env.set_paused(true);

    // Cancel still works.
    let creator_kp = env.creator.insecure_clone();
    let ix = env.ix_cancel(&creator_kp.pubkey(), &open);
    env.send(&[ix], &[&creator_kp]).unwrap();
    assert_eq!(env.bal(&open), 0);

    // Settle still works.
    env.advance(7 * DAY);
    let resolver = env.resolver.insecure_clone();
    let ix = env.ix_settle(
        &resolver.pubkey(),
        &to_settle,
        &creator,
        &taker,
        &fee_wallet,
        6_000_000,
    );
    env.send(&[ix], &[&resolver]).unwrap();
    assert_eq!(env.bal(&to_settle), 0);

    // Refund still works.
    env.advance(GRACE_SECS + 1);
    let caller = env.payer.insecure_clone();
    let ix = env.ix_refund(&caller.pubkey(), &to_refund, &creator, &taker);
    env.send(&[ix], &[]).unwrap();
    assert_eq!(env.bal(&to_refund), 0);
}

// ── end to end ────────────────────────────────────────────────────────────

#[test]
fn many_bets_in_parallel_all_balance_out() {
    let mut env = Env::new();
    let (creator, taker, fee_wallet) = (
        env.creator.pubkey(),
        env.taker.pubkey(),
        env.fee_wallet.pubkey(),
    );
    let total_before = env.bal(&creator) + env.bal(&taker) + env.bal(&fee_wallet);

    // One of each ending: settled to the creator, settled to the taker,
    // cancelled, and refunded.
    let settle_creator = env.matched_bet(1);
    let settle_taker = env.matched_bet(2);
    let cancelled = env.open_bet(3);
    let refunded = env.matched_bet(4);

    let creator_kp = env.creator.insecure_clone();
    let ix = env.ix_cancel(&creator_kp.pubkey(), &cancelled);
    env.send(&[ix], &[&creator_kp]).unwrap();

    env.advance(7 * DAY);
    let resolver = env.resolver.insecure_clone();
    for (bet, final_mcap) in [(settle_creator, 9_000_000u64), (settle_taker, 1_000u64)] {
        let ix = env.ix_settle(
            &resolver.pubkey(),
            &bet,
            &creator,
            &taker,
            &fee_wallet,
            final_mcap,
        );
        env.send(&[ix], &[&resolver]).unwrap();
    }

    env.advance(GRACE_SECS + 1);
    let caller = env.payer.insecure_clone();
    let ix = env.ix_refund(&caller.pubkey(), &refunded, &creator, &taker);
    env.send(&[ix], &[]).unwrap();

    let total_after = env.bal(&creator) + env.bal(&taker) + env.bal(&fee_wallet);
    assert_eq!(total_after, total_before, "no lamport lost or invented");

    // Two settled pots at 2% each.
    assert_eq!(env.bal(&fee_wallet), SOL + 2 * 40_000_000);
    for bet in [settle_creator, settle_taker, cancelled, refunded] {
        assert_eq!(env.bal(&bet), 0, "every bet account is closed");
    }
}

#[test]
fn the_bet_pda_is_derived_from_creator_and_id() {
    let env = Env::new();
    let creator = env.creator.pubkey();
    let expected = Pubkey::find_program_address(
        &[b"bet", creator.as_ref(), &7u64.to_le_bytes()],
        &env.program_id,
    )
    .0;
    assert_eq!(env.bet_pda(&creator, 7), expected);
    assert_ne!(env.bet_pda(&creator, 7), env.bet_pda(&creator, 8));
}
