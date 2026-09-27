use anchor_lang::prelude::*;

use crate::error::EscrowError;

/// Move lamports out of a program-owned escrow account.
///
/// The System Program cannot do this for us: the bet account is owned by this
/// program, not by the system program, so we adjust both balances directly.
/// The first borrow is released before the second is taken, so this stays sound
/// even when `from` and `to` are the same account.
pub fn move_lamports(from: &AccountInfo, to: &AccountInfo, amount: u64) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    {
        let mut from_lamports = from.try_borrow_mut_lamports()?;
        **from_lamports = from_lamports
            .checked_sub(amount)
            .ok_or(EscrowError::InsufficientEscrow)?;
    }
    let mut to_lamports = to.try_borrow_mut_lamports()?;
    **to_lamports = to_lamports
        .checked_add(amount)
        .ok_or(EscrowError::MathOverflow)?;
    Ok(())
}
