use anchor_lang::prelude::*;

#[error_code]
pub enum EscrowError {
    #[msg("Creating and taking bets is paused")]
    Paused,
    #[msg("Fee is above the 5% ceiling")]
    FeeTooHigh,
    #[msg("Stake limits must satisfy 0 < min_stake <= max_stake")]
    InvalidStakeLimits,
    #[msg("Grace period must be positive")]
    InvalidGrace,
    #[msg("Stake is outside the allowed range")]
    StakeOutOfRange,
    #[msg("Duration must be between 5 minutes and 30 days")]
    InvalidDuration,
    #[msg("Target market cap must be greater than zero")]
    InvalidTarget,
    #[msg("Direction must be 0 (higher) or 1 (lower)")]
    InvalidDirection,
    #[msg("Target must be above the starting market cap for a Higher bet, below it for a Lower bet")]
    TargetContradictsDirection,
    #[msg("Bet is not open")]
    BetNotOpen,
    #[msg("Bet is not matched")]
    BetNotMatched,
    #[msg("You cannot take your own bet")]
    CannotTakeOwnBet,
    #[msg("Only the creator can do this")]
    NotCreator,
    #[msg("Only the resolver can settle a bet")]
    NotResolver,
    #[msg("Only the admin can do this")]
    NotAdmin,
    #[msg("Bet has not expired yet")]
    NotYetExpired,
    #[msg("The resolver's grace period has passed; this bet can only be refunded")]
    GracePeriodOver,
    #[msg("The grace period has not passed yet")]
    GraceNotOver,
    #[msg("Account does not match the creator stored on the bet")]
    CreatorMismatch,
    #[msg("Account does not match the taker stored on the bet")]
    TakerMismatch,
    #[msg("Account does not match the fee wallet stored on the config")]
    FeeWalletMismatch,
    #[msg("The observed market cap was not seen inside this bet's window")]
    ObservedOutsideWindow,
    #[msg("The observed market cap does not reach the target")]
    ObservedDoesNotReachTarget,
    #[msg("The taker can only win once the deadline has passed")]
    TakerCannotWinYet,
    #[msg("This bet would push the program past its total exposure cap")]
    ExposureCapReached,
    #[msg("The exposure cap must leave room for at least one full bet")]
    InvalidExposureCap,
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("Escrow does not hold enough lamports")]
    InsufficientEscrow,
}
