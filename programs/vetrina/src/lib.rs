//! Spotlight — "vetrina" mechanic for a Solana memecoin launchpad.
//!
//! Scope: this program contains ONLY the showcase ("vetrina") mechanic — an
//! all-pay auction with linear decay and a consumable score. Bonding curve,
//! graduation and migration are delegated to Meteora Dynamic Bonding Curve
//! (external program) and are intentionally out of scope here.
//!
//! Instructions:
//!   * `create_priority`   — init the per-mint `Priority` PDA (plain `init`).
//!   * `bump`              — add lamports; grow `paid` and `effective` together.
//!   * `claim_spotlight`   — take the spotlight if `effective > bar(now)`
//!                           (permissionless).
//!   * `sweep`             — move the un-swept backing to the treasury
//!                           (permissionless).
//!
//! Invariants (verified structurally by the account model + checked in code):
//!   I1  `Priority.paid` is monotone, never decremented.
//!   I2  `claim` requires `effective > bar(now)`.
//!   I3  `candidate.mint != spotlight.mint`.
//!   I4  `paid_snapshot = effective` of the candidate POST-consumption.
//!   I5  `lease_end` is absolute, written only in `claim`.
//!   I7  `lamports(Priority) >= rent_min + (paid - swept)`, checked BEFORE the
//!       lamport movement in `sweep`.
//!   I8  `swept <= paid`.
//!   I17 `effective <= paid` always (structural: they grow together in `bump`,
//!       only `effective` descends, in `claim`).

use anchor_lang::prelude::*;
use anchor_lang::system_program::{self, Transfer};
use anchor_spl::token_interface::Mint;

declare_id!("gCxZar2tTSVKE1amCvMW5BcjMXvbqGRCf52n5P15gM4");

/// Fixed treasury that receives swept backing. `sweep` is permissionless, so
/// the destination is pinned by an `address =` constraint rather than a signer.
#[constant]
pub const TREASURY: Pubkey = pubkey!("6omNt7ZZvew6hZn9N3cJpzYczL1F4Gp5azFzfvVKkDyD");

/// Lease granted to a fresh spotlight holder, in seconds. During the lease the
/// bar stays full (P1); decay starts at `lease_end`.
pub const LEASE_SECONDS: i64 = 3_600;

/// Linear decay window after `lease_end`, in seconds. The bar reaches zero once
/// `now - lease_end >= DECAY_SECONDS` (P2).
pub const DECAY_SECONDS: i64 = 3_600;

/// Decaying entry bar the challenger must strictly beat.
///
/// Properties (exercised by the proptests):
///   P1  full (`== paid_snapshot`) during the lease, i.e. while `now <= lease_end`.
///   P2  zero once decay is complete (`now - lease_end >= decay_secs`).
///   P3  monotone non-increasing in `now`.
///   P4  never above `paid_snapshot`.
///
/// At `now == lease_end` exactly the bar is FULL (`elapsed == 0`): intended.
pub fn bar(paid_snapshot: u64, lease_end: i64, now: i64, decay_secs: i64) -> u64 {
    // P1: full for the whole lease, and exactly at `lease_end` (elapsed == 0).
    // Checked first so the lease is honored regardless of the decay window.
    if now <= lease_end {
        return paid_snapshot;
    }
    // Past lease_end: a non-positive decay window means already fully decayed
    // (this also guards the div-by-zero below).
    if decay_secs <= 0 {
        return 0;
    }
    let elapsed = now.saturating_sub(lease_end); // > 0 here
    // P2: fully decayed.
    if elapsed >= decay_secs {
        return 0;
    }
    // Linear ramp down: paid_snapshot * (decay - elapsed) / decay.
    // u128 intermediate cannot overflow for u64 * (positive i64); checked ops
    // keep us inside the "only checked_*" rule. Result <= paid_snapshot (P4).
    let remaining = (decay_secs - elapsed) as u128;
    (paid_snapshot as u128)
        .checked_mul(remaining)
        .and_then(|v| v.checked_div(decay_secs as u128))
        .unwrap_or(0) as u64
}

#[program]
pub mod vetrina {
    use super::*;

    /// Initialize the per-mint `Priority` PDA. Plain `init` (never
    /// `init_if_needed`): a second `create_priority` on the same mint fails.
    pub fn create_priority(ctx: Context<CreatePriority>) -> Result<()> {
        let p = &mut ctx.accounts.priority;
        p.mint = ctx.accounts.mint.key();
        p.paid = 0;
        p.effective = 0;
        p.swept = 0;
        p.bump = ctx.bumps.priority;

        emit_cpi!(PriorityCreated { mint: p.mint });
        Ok(())
    }

    /// Add lamports and grow `paid` and `effective` together (preserves I17).
    /// Lamports are deposited into the PDA so they back `paid - swept` (I7).
    pub fn bump(ctx: Context<Bump>, amount: u64) -> Result<()> {
        require!(amount > 0, VetrinaError::ZeroAmount);

        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.key(),
                Transfer {
                    from: ctx.accounts.payer.to_account_info(),
                    to: ctx.accounts.priority.to_account_info(),
                },
            ),
            amount,
        )?;

        let p = &mut ctx.accounts.priority;
        // I1: paid only grows. I17: effective grows by the same delta.
        p.paid = p.paid.checked_add(amount).ok_or(VetrinaError::MathOverflow)?;
        p.effective = p
            .effective
            .checked_add(amount)
            .ok_or(VetrinaError::MathOverflow)?;

        emit_cpi!(Bumped {
            mint: p.mint,
            amount,
            paid: p.paid,
            effective: p.effective,
        });
        Ok(())
    }

    /// Take the spotlight. Permissionless. Requires `effective > bar(now)` (I2);
    /// consumes the bar from `effective` and snapshots the remainder (I4).
    pub fn claim_spotlight(ctx: Context<ClaimSpotlight>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let spotlight = &mut ctx.accounts.spotlight;
        let candidate = &mut ctx.accounts.candidate;

        // I3: cannot claim the slot the candidate already holds.
        require_keys_neq!(candidate.mint, spotlight.mint, VetrinaError::AlreadyHolder);

        let b = bar(spotlight.paid_snapshot, spotlight.lease_end, now, DECAY_SECONDS);

        // I2: strictly beat the bar.
        require!(candidate.effective > b, VetrinaError::BelowBar);

        // Consume the bar out of `effective` (only `effective` descends; `paid`
        // untouched -> I1, I17 preserved).
        let post = candidate
            .effective
            .checked_sub(b)
            .ok_or(VetrinaError::MathOverflow)?;
        candidate.effective = post;

        // I4: snapshot = effective POST-consumption. I5: absolute lease_end,
        // written only here.
        spotlight.mint = candidate.mint;
        spotlight.paid_snapshot = post;
        spotlight.lease_end = now
            .checked_add(LEASE_SECONDS)
            .ok_or(VetrinaError::MathOverflow)?;
        spotlight.bump = ctx.bumps.spotlight;

        emit_cpi!(Claimed {
            mint: candidate.mint,
            paid_snapshot: post,
            lease_end: spotlight.lease_end,
            bar: b,
        });
        Ok(())
    }

    /// Move the un-swept backing (`paid - swept`) to the treasury. Permissionless.
    pub fn sweep(ctx: Context<Sweep>) -> Result<()> {
        let priority_ai = ctx.accounts.priority.to_account_info();
        let p = &mut ctx.accounts.priority;

        // I8: swept <= paid, so owed is well-defined and non-negative.
        let owed = p.paid.checked_sub(p.swept).ok_or(VetrinaError::MathOverflow)?;
        require!(owed > 0, VetrinaError::NothingToSweep);

        // I7: verify the PDA still backs rent_min + owed BEFORE moving anything.
        let rent_min = Rent::get()?.minimum_balance(priority_ai.data_len());
        let need = rent_min
            .checked_add(owed)
            .ok_or(VetrinaError::MathOverflow)?;
        require!(
            priority_ai.lamports() >= need,
            VetrinaError::InsufficientBacking
        );

        // Direct lamport move: the PDA is program-owned, so we debit it and
        // credit the treasury. Leaves exactly rent_min behind (rent-exempt).
        **priority_ai.try_borrow_mut_lamports()? = priority_ai
            .lamports()
            .checked_sub(owed)
            .ok_or(VetrinaError::MathOverflow)?;
        let treasury_ai = ctx.accounts.treasury.to_account_info();
        **treasury_ai.try_borrow_mut_lamports()? = treasury_ai
            .lamports()
            .checked_add(owed)
            .ok_or(VetrinaError::MathOverflow)?;

        // I8: swept advances to paid; owed becomes 0 -> I7 still holds after.
        p.swept = p.paid;

        emit_cpi!(Swept {
            mint: p.mint,
            owed,
            swept: p.swept,
        });
        Ok(())
    }
}

#[derive(Accounts)]
#[event_cpi]
pub struct CreatePriority<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// The mint whose priority we track. `InterfaceAccount<Mint>` validates it
    /// is a genuine SPL-Token or Token-2022 mint (the sole use of anchor-spl).
    pub mint: InterfaceAccount<'info, Mint>,

    #[account(
        init,
        payer = payer,
        space = 8 + Priority::INIT_SPACE,
        seeds = [b"priority", mint.key().as_ref()],
        bump,
    )]
    pub priority: Account<'info, Priority>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[event_cpi]
pub struct Bump<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        seeds = [b"priority", priority.mint.as_ref()],
        bump = priority.bump,
    )]
    pub priority: Account<'info, Priority>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[event_cpi]
pub struct ClaimSpotlight<'info> {
    /// Pays for the one-time spotlight init. Claim itself is permissionless.
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + Spotlight::INIT_SPACE,
        seeds = [b"spotlight"],
        bump,
    )]
    pub spotlight: Account<'info, Spotlight>,

    /// Candidate's own `Priority` PDA. The seeds are re-derived from the
    /// account's stored `mint`/`bump`, so a substituted account at a
    /// non-canonical address fails validation (I3 substitution guard).
    #[account(
        mut,
        seeds = [b"priority", candidate.mint.as_ref()],
        bump = candidate.bump,
    )]
    pub candidate: Account<'info, Priority>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[event_cpi]
pub struct Sweep<'info> {
    #[account(
        mut,
        seeds = [b"priority", priority.mint.as_ref()],
        bump = priority.bump,
    )]
    pub priority: Account<'info, Priority>,

    #[account(mut, address = TREASURY)]
    pub treasury: SystemAccount<'info>,
}

#[account]
#[derive(InitSpace)]
pub struct Spotlight {
    /// Mint currently holding the spotlight.
    pub mint: Pubkey,
    /// Effective of the holder POST-consumption at claim time (I4).
    pub paid_snapshot: u64,
    /// Absolute unix timestamp at which the lease ends (I5).
    pub lease_end: i64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct Priority {
    pub mint: Pubkey,
    /// Monotone, never decremented (I1).
    pub paid: u64,
    /// Consumable score; `effective <= paid` always (I17).
    pub effective: u64,
    /// Amount already swept to the treasury; `swept <= paid` (I8).
    pub swept: u64,
    pub bump: u8,
}

#[event]
pub struct PriorityCreated {
    pub mint: Pubkey,
}

#[event]
pub struct Bumped {
    pub mint: Pubkey,
    pub amount: u64,
    pub paid: u64,
    pub effective: u64,
}

#[event]
pub struct Claimed {
    pub mint: Pubkey,
    pub paid_snapshot: u64,
    pub lease_end: i64,
    pub bar: u64,
}

#[event]
pub struct Swept {
    pub mint: Pubkey,
    pub owed: u64,
    pub swept: u64,
}

#[error_code]
pub enum VetrinaError {
    #[msg("amount must be greater than zero")]
    ZeroAmount,
    #[msg("nothing to sweep: paid == swept")]
    NothingToSweep,
    #[msg("effective does not beat the current bar")]
    BelowBar,
    #[msg("candidate already holds the spotlight")]
    AlreadyHolder,
    #[msg("priority account does not back rent_min + owed")]
    InsufficientBacking,
    #[msg("arithmetic overflow")]
    MathOverflow,
}

#[cfg(test)]
mod bar_unit_tests {
    use super::*;

    #[test]
    fn p1_full_during_lease_and_at_lease_end() {
        // now < lease_end and now == lease_end -> full.
        assert_eq!(bar(100, 1_000, 500, DECAY_SECONDS), 100);
        assert_eq!(bar(100, 1_000, 1_000, DECAY_SECONDS), 100); // elapsed == 0
    }

    #[test]
    fn p2_zero_after_full_decay() {
        assert_eq!(bar(100, 1_000, 1_000 + DECAY_SECONDS, DECAY_SECONDS), 0);
        assert_eq!(bar(100, 1_000, 1_000 + DECAY_SECONDS + 1, DECAY_SECONDS), 0);
    }

    #[test]
    fn p4_midpoint_is_half() {
        // halfway through the decay window -> ~half the snapshot.
        let half = bar(100, 0, DECAY_SECONDS / 2, DECAY_SECONDS);
        assert_eq!(half, 50);
    }

    #[test]
    fn degenerate_window_is_zero_past_end() {
        assert_eq!(bar(100, 0, 1, 0), 0);
    }
}
