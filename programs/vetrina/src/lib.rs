use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};
use anchor_spl::token_interface::Mint;

// Sostituire con la chiave reale: `anchor keys sync`
declare_id!("gCxZar2tTSVKE1amCvMW5BcjMXvbqGRCf52n5P15gM4");

// ---------------------------------------------------------------------------
// Costanti compile-time (I15-analoghe: parametri fuori dal controllo runtime)
// ---------------------------------------------------------------------------
pub const CONFIG_SEED: &[u8] = b"config";
pub const SPOTLIGHT_SEED: &[u8] = b"spotlight";
pub const PRIORITY_SEED: &[u8] = b"priority";

/// Limiti hard sui parametri di Config: l'authority non può uscirne.
pub const MIN_DECAY_SECS: u32 = 60; // 1 min
pub const MAX_DECAY_SECS: u32 = 30 * 24 * 3600; // 30 giorni
pub const MIN_LEASE_SECS: i64 = 60;
pub const MAX_LEASE_SECS: i64 = 30 * 24 * 3600;

// ---------------------------------------------------------------------------
// Funzione pura — fuzzabile in isolamento (proptest)
// ---------------------------------------------------------------------------

/// Barra di soglia per il claim.
/// - lease attivo: barra piena = paid_snapshot
/// - dopo lease_end: decadimento lineare intero fino a 0 in `decay` secondi
///
/// Proprietà (verificate da proptest):
///  P1: now < lease_end            => bar == paid_snapshot
///  P2: now >= lease_end + decay   => bar == 0
///  P3: monotona non crescente in `now`
///  P4: bar <= paid_snapshot sempre
pub fn bar(paid_snapshot: u64, lease_end: i64, now: i64, decay: u32) -> u64 {
    if now < lease_end {
        return paid_snapshot;
    }
    let elapsed = now.saturating_sub(lease_end);
    let decay = decay as i64;
    if elapsed >= decay {
        return 0;
    }
    // paid_snapshot * (decay - elapsed) / decay, floor.
    // u128 intermedio: nessun overflow possibile (u64 * i64 positivo < 2^127).
    let remaining = (decay - elapsed) as u128;
    let num = (paid_snapshot as u128).saturating_mul(remaining);
    (num / decay as u128) as u64
}

// ---------------------------------------------------------------------------
// Account
// ---------------------------------------------------------------------------

#[account]
#[derive(InitSpace)]
pub struct Config {
    pub authority: Pubkey,
    pub treasury: Pubkey,
    /// Secondi di decadimento lineare della barra dopo lease_end.
    pub decay: u32,
    /// Durata della vetrina in secondi.
    pub lease_duration: i64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct Spotlight {
    /// Token attualmente in vetrina. Pubkey::default() = vetrina mai assegnata.
    pub mint: Pubkey,
    /// effective del vincitore al momento del claim (I4).
    pub paid_snapshot: u64,
    /// Timestamp assoluto di fine lease (I5: scritto solo in claim).
    pub lease_end: i64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct Priority {
    pub mint: Pubkey,
    /// I1: monotono, mai decrementato. Contabilità per sweep.
    pub paid: u64,
    /// Punteggio spendibile. Cresce con bump, consumato al claim.
    /// I17: effective <= paid (strutturale: crescono insieme, solo effective scende).
    pub effective: u64,
    /// I8: swept <= paid. Quanto già trasferito al treasury.
    pub swept: u64,
    pub bump: u8,
}

// ---------------------------------------------------------------------------
// Eventi (emit_cpi!: transaction metadata, non log troncabili)
// ---------------------------------------------------------------------------

#[event]
pub struct BumpEvent {
    pub mint: Pubkey,
    pub payer: Pubkey,
    pub lamports: u64,
    pub paid: u64,
    pub effective: u64,
}

#[event]
pub struct SweepEvent {
    pub mint: Pubkey,
    pub amount: u64,
    pub swept_total: u64,
}

#[event]
pub struct ClaimEvent {
    pub mint: Pubkey,
    pub previous_mint: Pubkey,
    pub bar_paid: u64,
    pub paid_snapshot: u64,
    pub lease_end: i64,
}

// ---------------------------------------------------------------------------
// Errori
// ---------------------------------------------------------------------------

#[error_code]
pub enum VetrinaError {
    #[msg("Parametro fuori dai limiti compile-time")]
    ParamOutOfBounds,
    #[msg("Importo nullo")]
    ZeroAmount,
    #[msg("I2: effective non supera la barra corrente")]
    BelowBar,
    #[msg("I3: il token è già in vetrina")]
    AlreadyHolder,
    #[msg("I7: PDA Priority insolvente rispetto a paid - swept")]
    Insolvent,
    #[msg("Overflow aritmetico")]
    MathOverflow,
    #[msg("Nulla da trasferire")]
    NothingToSweep,
}

// ---------------------------------------------------------------------------
// Programma
// ---------------------------------------------------------------------------

#[program]
pub mod vetrina {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, treasury: Pubkey, decay: u32, lease_duration: i64) -> Result<()> {
        require!(
            (MIN_DECAY_SECS..=MAX_DECAY_SECS).contains(&decay)
                && (MIN_LEASE_SECS..=MAX_LEASE_SECS).contains(&lease_duration),
            VetrinaError::ParamOutOfBounds
        );

        let config = &mut ctx.accounts.config;
        config.authority = ctx.accounts.authority.key();
        config.treasury = treasury;
        config.decay = decay;
        config.lease_duration = lease_duration;
        config.bump = ctx.bumps.config;

        let spotlight = &mut ctx.accounts.spotlight;
        spotlight.mint = Pubkey::default();
        spotlight.paid_snapshot = 0;
        spotlight.lease_end = 0;
        spotlight.bump = ctx.bumps.spotlight;
        Ok(())
    }

    /// L'authority può aggiornare i parametri, sempre dentro i cap compile-time.
    pub fn update_config(ctx: Context<UpdateConfig>, treasury: Pubkey, decay: u32, lease_duration: i64) -> Result<()> {
        require!(
            (MIN_DECAY_SECS..=MAX_DECAY_SECS).contains(&decay)
                && (MIN_LEASE_SECS..=MAX_LEASE_SECS).contains(&lease_duration),
            VetrinaError::ParamOutOfBounds
        );
        let config = &mut ctx.accounts.config;
        config.treasury = treasury;
        config.decay = decay;
        config.lease_duration = lease_duration;
        Ok(())
    }

    /// Permissionless: chiunque crea il PDA Priority per un mint (una volta sola: `init`).
    pub fn create_priority(ctx: Context<CreatePriority>) -> Result<()> {
        let p = &mut ctx.accounts.priority;
        p.mint = ctx.accounts.mint.key();
        p.paid = 0;
        p.effective = 0;
        p.swept = 0;
        p.bump = ctx.bumps.priority;
        Ok(())
    }

    /// Versa lamports nel PDA Priority del token.
    /// I1: paid monotono. I17: effective cresce con paid, quindi effective <= paid.
    pub fn bump(ctx: Context<BumpCtx>, lamports: u64) -> Result<()> {
        require!(lamports > 0, VetrinaError::ZeroAmount);

        // Transfer SystemProgram: payer (system-owned) -> PDA Priority.
        transfer(
            CpiContext::new(
                ctx.accounts.system_program.key(),
                Transfer {
                    from: ctx.accounts.payer.to_account_info(),
                    to: ctx.accounts.priority.to_account_info(),
                },
            ),
            lamports,
        )?;

        let p = &mut ctx.accounts.priority;
        p.paid = p.paid.checked_add(lamports).ok_or(VetrinaError::MathOverflow)?;
        p.effective = p.effective.checked_add(lamports).ok_or(VetrinaError::MathOverflow)?;

        emit_cpi!(BumpEvent {
            mint: p.mint,
            payer: ctx.accounts.payer.key(),
            lamports,
            paid: p.paid,
            effective: p.effective,
        });
        Ok(())
    }

    /// Permissionless: trasferisce paid - swept dal PDA al treasury.
    /// I7 verificata prima del movimento, I8 dopo.
    pub fn sweep(ctx: Context<Sweep>) -> Result<()> {
        let owed = {
            let p = &ctx.accounts.priority;
            p.paid.checked_sub(p.swept).ok_or(VetrinaError::MathOverflow)?
        };
        require!(owed > 0, VetrinaError::NothingToSweep);

        // I7: il PDA deve restare rent-exempt dopo il prelievo.
        let priority_info = ctx.accounts.priority.to_account_info();
        let rent_min = Rent::get()?.minimum_balance(priority_info.data_len());
        let lamports_now = priority_info.lamports();
        require!(
            lamports_now >= rent_min.checked_add(owed).ok_or(VetrinaError::MathOverflow)?,
            VetrinaError::Insolvent
        );

        // PDA program-owned: movimento diretto di lamports.
        {
            let mut from = priority_info.try_borrow_mut_lamports()?;
            **from = from.checked_sub(owed).ok_or(VetrinaError::MathOverflow)?;
        }
        {
            let treasury_info = ctx.accounts.treasury.to_account_info();
            let mut to = treasury_info.try_borrow_mut_lamports()?;
            **to = to.checked_add(owed).ok_or(VetrinaError::MathOverflow)?;
        }

        let p = &mut ctx.accounts.priority;
        p.swept = p.paid; // I8

        emit_cpi!(SweepEvent {
            mint: p.mint,
            amount: owed,
            swept_total: p.swept,
        });
        Ok(())
    }

    /// O(1). I2: effective > bar. I3: mint diverso. I4: snapshot = effective post-consumo.
    /// I5: lease_end assoluto. Consumo: effective -= bar (anti-ratchet).
    pub fn claim_spotlight(ctx: Context<ClaimSpotlight>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let config = &ctx.accounts.config;
        let spotlight = &mut ctx.accounts.spotlight;
        let candidate = &mut ctx.accounts.candidate;

        require!(candidate.mint != spotlight.mint, VetrinaError::AlreadyHolder); // I3

        let threshold = bar(spotlight.paid_snapshot, spotlight.lease_end, now, config.decay);
        require!(candidate.effective > threshold, VetrinaError::BelowBar); // I2

        // Consumo del punteggio: chi vince paga la barra. I17 preservata (checked_sub).
        candidate.effective = candidate
            .effective
            .checked_sub(threshold)
            .ok_or(VetrinaError::MathOverflow)?;

        let previous_mint = spotlight.mint;
        spotlight.mint = candidate.mint;
        spotlight.paid_snapshot = candidate.effective; // I4
        spotlight.lease_end = now
            .checked_add(config.lease_duration)
            .ok_or(VetrinaError::MathOverflow)?; // I5

        emit_cpi!(ClaimEvent {
            mint: candidate.mint,
            previous_mint,
            bar_paid: threshold,
            paid_snapshot: spotlight.paid_snapshot,
            lease_end: spotlight.lease_end,
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Contesti — validazione interamente dichiarativa
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        seeds = [CONFIG_SEED],
        bump,
        space = 8 + Config::INIT_SPACE
    )]
    pub config: Account<'info, Config>,

    #[account(
        init,
        payer = authority,
        seeds = [SPOTLIGHT_SEED],
        bump,
        space = 8 + Spotlight::INIT_SPACE
    )]
    pub spotlight: Account<'info, Spotlight>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = authority
    )]
    pub config: Account<'info, Config>,
}

#[derive(Accounts)]
pub struct CreatePriority<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// Mint del token (SPL o Token-2022, creato da DBC).
    pub mint: InterfaceAccount<'info, Mint>,

    #[account(
        init,
        payer = payer,
        seeds = [PRIORITY_SEED, mint.key().as_ref()],
        bump,
        space = 8 + Priority::INIT_SPACE
    )]
    pub priority: Account<'info, Priority>,

    pub system_program: Program<'info, System>,
}

#[event_cpi]
#[derive(Accounts)]
pub struct BumpCtx<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        mut,
        seeds = [PRIORITY_SEED, priority.mint.as_ref()],
        bump = priority.bump
    )]
    pub priority: Account<'info, Priority>,

    pub system_program: Program<'info, System>,
}

#[event_cpi]
#[derive(Accounts)]
pub struct Sweep<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [PRIORITY_SEED, priority.mint.as_ref()],
        bump = priority.bump
    )]
    pub priority: Account<'info, Priority>,

    /// CHECK: vincolato all'indirizzo in Config.
    #[account(mut, address = config.treasury)]
    pub treasury: UncheckedAccount<'info>,
}

#[event_cpi]
#[derive(Accounts)]
pub struct ClaimSpotlight<'info> {
    /// Permissionless: paga solo la tx, nessun privilegio.
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,

    #[account(mut, seeds = [SPOTLIGHT_SEED], bump = spotlight.bump)]
    pub spotlight: Account<'info, Spotlight>,

    #[account(
        mut,
        seeds = [PRIORITY_SEED, candidate.mint.as_ref()],
        bump = candidate.bump
    )]
    pub candidate: Account<'info, Priority>,
}

// ---------------------------------------------------------------------------
// Test delle proprietà della funzione pura
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        // P1: lease attivo => barra piena
        #[test]
        fn p1_lease_active(snap in any::<u64>(), lease_end in 1i64..i64::MAX/2, decay in 60u32..=2_592_000) {
            let now = lease_end - 1;
            prop_assert_eq!(bar(snap, lease_end, now, decay), snap);
        }

        // P2: oltre il decadimento => zero
        #[test]
        fn p2_fully_decayed(snap in any::<u64>(), lease_end in 0i64..i64::MAX/4, decay in 60u32..=2_592_000) {
            let now = lease_end + decay as i64;
            prop_assert_eq!(bar(snap, lease_end, now, decay), 0);
        }

        // P3: monotona non crescente
        #[test]
        fn p3_monotone(snap in any::<u64>(), lease_end in 0i64..1_000_000_000, decay in 60u32..=2_592_000, t1 in 0i64..3_000_000_000, dt in 0i64..1_000_000) {
            let t2 = t1 + dt;
            prop_assert!(bar(snap, lease_end, t2, decay) <= bar(snap, lease_end, t1, decay));
        }

        // P4: mai sopra lo snapshot
        #[test]
        fn p4_bounded(snap in any::<u64>(), lease_end in any::<i64>(), now in any::<i64>(), decay in 60u32..=2_592_000) {
            prop_assert!(bar(snap, lease_end, now, decay) <= snap);
        }
    }

    // Caso limite: claim esattamente a lease_end (barra ancora piena? No: now >= lease_end
    // entra nel ramo di decadimento con elapsed = 0 => barra piena). Documentato.
    #[test]
    fn edge_at_lease_end() {
        assert_eq!(bar(1000, 500, 500, 100), 1000);
        assert_eq!(bar(1000, 500, 550, 100), 500);
        assert_eq!(bar(1000, 500, 600, 100), 0);
        assert_eq!(bar(1000, 500, 601, 100), 0);
    }

    // I17 strutturale: simulazione bump/claim
    #[test]
    fn i17_effective_le_paid() {
        let mut paid = 0u64;
        let mut effective = 0u64;
        for x in [100u64, 250, 7] {
            paid += x;
            effective += x;
        }
        // claim con barra 200
        effective -= 200;
        assert!(effective <= paid);
    }
}
