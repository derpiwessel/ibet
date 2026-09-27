//! Shared harness for the escrow tests.
//!
//! Everything runs on LiteSVM rather than a local validator, because most of
//! these paths need the clock moved forward days at a time — a validator's
//! clock only advances at wall-clock speed, so expiry and grace windows would
//! be untestable.
//!
//! One dedicated `payer` pays every transaction fee, so the balances of the
//! creator, taker and fee wallet move only through escrow and can be asserted
//! down to the lamport.

#![allow(dead_code)]

use {
    anchor_lang::{
        prelude::Pubkey, solana_program::instruction::Instruction,
        solana_program::system_program, AccountDeserialize, InstructionData, Space,
        ToAccountMetas,
    },
    ibet_escrow::{
        instructions::{CreateBetArgs, InitConfigArgs, UpdateConfigArgs},
        state::{Bet, Outcome},
    },
    litesvm::{types::TransactionResult, LiteSVM},
    solana_clock::Clock,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

pub const SOL: u64 = 1_000_000_000;
pub const DAY: i64 = 86_400;

/// The phase 1 production values.
pub const FEE_BPS: u16 = 200;
pub const MIN_STAKE: u64 = SOL / 100;
pub const MAX_STAKE: u64 = 5 * SOL;
pub const GRACE_SECS: i64 = 48 * 3_600;

/// A plausible "now" to start from; LiteSVM's default clock sits at 0.
pub const BASE_TIME: i64 = 1_800_000_000;

pub const HIGHER: u8 = 0;
pub const LOWER: u8 = 1;

pub struct Env {
    pub svm: LiteSVM,
    pub program_id: Pubkey,
    /// Pays every transaction fee, so the parties' balances stay clean.
    pub payer: Keypair,
    pub admin: Keypair,
    pub resolver: Keypair,
    pub fee_wallet: Keypair,
    pub creator: Keypair,
    pub taker: Keypair,
    pub config: Pubkey,
}

impl Env {
    pub fn new() -> Self {
        Self::with_fee_bps(FEE_BPS)
    }

    pub fn with_fee_bps(fee_bps: u16) -> Self {
        let program_id = ibet_escrow::id();
        let mut svm = LiteSVM::new();
        let bytes = include_bytes!(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/../deploy/ibet_escrow.so"
        ));
        svm.add_program(program_id, bytes).unwrap();

        let payer = Keypair::new();
        let admin = Keypair::new();
        let resolver = Keypair::new();
        let fee_wallet = Keypair::new();
        let creator = Keypair::new();
        let taker = Keypair::new();

        svm.airdrop(&payer.pubkey(), 100 * SOL).unwrap();
        svm.airdrop(&admin.pubkey(), SOL).unwrap();
        svm.airdrop(&fee_wallet.pubkey(), SOL).unwrap();
        svm.airdrop(&creator.pubkey(), 20 * SOL).unwrap();
        svm.airdrop(&taker.pubkey(), 20 * SOL).unwrap();

        let config = Pubkey::find_program_address(&[ibet_escrow::CONFIG_SEED], &program_id).0;

        let mut env = Self {
            svm,
            program_id,
            payer,
            admin,
            resolver,
            fee_wallet,
            creator,
            taker,
            config,
        };
        env.set_time(BASE_TIME);

        let args = InitConfigArgs {
            resolver: env.resolver.pubkey(),
            fee_wallet: env.fee_wallet.pubkey(),
            fee_bps,
            min_stake: MIN_STAKE,
            max_stake: MAX_STAKE,
            grace_secs: GRACE_SECS,
        };
        let ix = Instruction::new_with_bytes(
            program_id,
            &ibet_escrow::instruction::InitializeConfig { args }.data(),
            ibet_escrow::accounts::InitializeConfig {
                admin: env.admin.pubkey(),
                config,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        );
        let admin = env.admin.insecure_clone();
        env.send(&[ix], &[&admin]).unwrap();
        env
    }

    pub fn send(&mut self, ixs: &[Instruction], extra_signers: &[&Keypair]) -> TransactionResult {
        // A fresh blockhash per transaction keeps signatures unique, so
        // otherwise-identical transactions aren't rejected as duplicates.
        self.svm.expire_blockhash();
        let blockhash = self.svm.latest_blockhash();
        let msg = Message::new_with_blockhash(ixs, Some(&self.payer.pubkey()), &blockhash);
        let mut signers: Vec<&Keypair> = vec![&self.payer];
        signers.extend_from_slice(extra_signers);
        let tx =
            VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &signers[..]).unwrap();
        self.svm.send_transaction(tx)
    }

    pub fn now(&self) -> i64 {
        self.svm.get_sysvar::<Clock>().unix_timestamp
    }

    pub fn set_time(&mut self, unix_timestamp: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp = unix_timestamp;
        self.svm.set_sysvar::<Clock>(&clock);
    }

    pub fn advance(&mut self, secs: i64) {
        let t = self.now() + secs;
        self.set_time(t);
    }

    pub fn bet_pda(&self, creator: &Pubkey, bet_id: u64) -> Pubkey {
        Pubkey::find_program_address(
            &[
                ibet_escrow::BET_SEED,
                creator.as_ref(),
                &bet_id.to_le_bytes(),
            ],
            &self.program_id,
        )
        .0
    }

    pub fn bet(&self, pda: &Pubkey) -> Bet {
        let account = self.svm.get_account(pda).expect("bet account is gone");
        let mut data: &[u8] = &account.data;
        Bet::try_deserialize(&mut data).expect("bet did not deserialize")
    }

    pub fn bal(&self, key: &Pubkey) -> u64 {
        self.svm.get_balance(key).unwrap_or(0)
    }

    /// Rent the bet account holds on top of the two stakes.
    pub fn bet_rent(&self) -> u64 {
        self.svm
            .minimum_balance_for_rent_exemption(8 + Bet::INIT_SPACE)
    }

    // ── instruction builders ──────────────────────────────────────────────

    pub fn ix_create(&self, creator: &Pubkey, args: CreateBetArgs) -> Instruction {
        let bet = self.bet_pda(creator, args.bet_id);
        Instruction::new_with_bytes(
            self.program_id,
            &ibet_escrow::instruction::CreateBet { args }.data(),
            ibet_escrow::accounts::CreateBet {
                creator: *creator,
                config: self.config,
                bet,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn ix_take(&self, taker: &Pubkey, bet: &Pubkey) -> Instruction {
        Instruction::new_with_bytes(
            self.program_id,
            &ibet_escrow::instruction::TakeBet {}.data(),
            ibet_escrow::accounts::TakeBet {
                taker: *taker,
                config: self.config,
                bet: *bet,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
        )
    }

    pub fn ix_cancel(&self, creator: &Pubkey, bet: &Pubkey) -> Instruction {
        Instruction::new_with_bytes(
            self.program_id,
            &ibet_escrow::instruction::CancelBet {}.data(),
            ibet_escrow::accounts::CancelBet {
                creator: *creator,
                bet: *bet,
            }
            .to_account_metas(None),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ix_settle(
        &self,
        resolver: &Pubkey,
        bet: &Pubkey,
        creator: &Pubkey,
        taker: &Pubkey,
        fee_wallet: &Pubkey,
        outcome: Outcome,
        observed_mcap_usd: u64,
        observed_at: i64,
    ) -> Instruction {
        Instruction::new_with_bytes(
            self.program_id,
            &ibet_escrow::instruction::SettleBet {
                outcome,
                observed_mcap_usd,
                observed_at,
            }
            .data(),
            ibet_escrow::accounts::SettleBet {
                resolver: *resolver,
                config: self.config,
                bet: *bet,
                creator: *creator,
                taker: *taker,
                fee_wallet: *fee_wallet,
            }
            .to_account_metas(None),
        )
    }

    pub fn ix_refund(
        &self,
        caller: &Pubkey,
        bet: &Pubkey,
        creator: &Pubkey,
        taker: &Pubkey,
    ) -> Instruction {
        Instruction::new_with_bytes(
            self.program_id,
            &ibet_escrow::instruction::RefundExpired {}.data(),
            ibet_escrow::accounts::RefundExpired {
                caller: *caller,
                config: self.config,
                bet: *bet,
                creator: *creator,
                taker: *taker,
            }
            .to_account_metas(None),
        )
    }

    pub fn ix_update_config(&self, admin: &Pubkey, args: UpdateConfigArgs) -> Instruction {
        Instruction::new_with_bytes(
            self.program_id,
            &ibet_escrow::instruction::UpdateConfig { args }.data(),
            ibet_escrow::accounts::UpdateConfig {
                admin: *admin,
                config: self.config,
            }
            .to_account_metas(None),
        )
    }

    // ── common flows ──────────────────────────────────────────────────────

    /// A moment inside a default bet's window, usable as settlement evidence.
    pub fn mid_window(&self) -> i64 {
        BASE_TIME + 3 * DAY
    }

    /// The last moment that still counts as inside a default bet's window.
    pub fn window_end(&self) -> i64 {
        BASE_TIME + 7 * DAY
    }

    /// A standard 1 SOL / 7 day Higher bet from `self.creator`.
    pub fn default_args(&self, bet_id: u64) -> CreateBetArgs {
        CreateBetArgs {
            bet_id,
            token_mint: Pubkey::new_unique(),
            direction: HIGHER,
            target_mcap_usd: 5_000_000,
            start_mcap_usd: 1_000_000,
            stake: SOL,
            duration_secs: 7 * DAY,
        }
    }

    pub fn open_bet(&mut self, bet_id: u64) -> Pubkey {
        let args = self.default_args(bet_id);
        self.open_bet_with(args)
    }

    pub fn open_bet_with(&mut self, args: CreateBetArgs) -> Pubkey {
        let creator = self.creator.insecure_clone();
        let bet = self.bet_pda(&creator.pubkey(), args.bet_id);
        let ix = self.ix_create(&creator.pubkey(), args);
        self.send(&[ix], &[&creator]).unwrap();
        bet
    }

    pub fn matched_bet(&mut self, bet_id: u64) -> Pubkey {
        let bet = self.open_bet(bet_id);
        self.take(&bet);
        bet
    }

    pub fn take(&mut self, bet: &Pubkey) {
        let taker = self.taker.insecure_clone();
        let ix = self.ix_take(&taker.pubkey(), bet);
        self.send(&[ix], &[&taker]).unwrap();
    }

    pub fn set_paused(&mut self, paused: bool) {
        let admin = self.admin.insecure_clone();
        let ix = self.ix_update_config(
            &admin.pubkey(),
            UpdateConfigArgs {
                paused: Some(paused),
                ..Default::default()
            },
        );
        self.send(&[ix], &[&admin]).unwrap();
    }

    /// Total lamports held by everyone who can gain or lose through escrow.
    /// Transaction fees come out of `payer`, so this sum is conserved exactly.
    pub fn escrow_total(&self, bet: &Pubkey) -> u64 {
        self.bal(&self.creator.pubkey())
            + self.bal(&self.taker.pubkey())
            + self.bal(&self.fee_wallet.pubkey())
            + self.bal(bet)
    }
}

/// Asserts the transaction failed with one of our own `EscrowError`s.
pub fn assert_escrow_err(result: TransactionResult, expected: ibet_escrow::error::EscrowError) {
    assert_custom_err(
        result,
        expected as u32 + anchor_lang::error::ERROR_CODE_OFFSET,
    );
}

/// Asserts the transaction failed with a specific custom program error code.
///
/// The error is read out of the Debug form rather than matched on the
/// `TransactionError` type, so this does not pin the test suite to whichever
/// version of the solana error crates LiteSVM happens to depend on.
pub fn assert_custom_err(result: TransactionResult, expected_code: u32) {
    let err = result.expect_err("transaction was expected to fail");
    let rendered = format!("{:?}", err.err);
    let code = custom_error_code(&rendered).unwrap_or_else(|| {
        panic!("expected a custom program error, got {rendered}; logs: {:?}", err.meta.logs)
    });
    assert_eq!(
        code, expected_code,
        "expected custom error {expected_code}, got {code}; logs: {:?}",
        err.meta.logs
    );
}

/// Pulls the number out of a `... Custom(6002) ...` debug rendering.
fn custom_error_code(rendered: &str) -> Option<u32> {
    let start = rendered.find("Custom(")? + "Custom(".len();
    let rest = &rendered[start..];
    let end = rest.find(')')?;
    rest[..end].parse().ok()
}

/// Asserts the transaction failed, without pinning the exact reason. Used where
/// the runtime, not our program, rejects the transaction.
pub fn assert_failed(result: TransactionResult) {
    assert!(
        result.is_err(),
        "transaction was expected to fail but succeeded"
    );
}
