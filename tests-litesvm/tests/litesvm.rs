//! litesvm integration tests for the vetrina program.
//!
//! These load the compiled on-chain artifact, so build it first:
//!
//!     anchor build           # produces target/deploy/vetrina.so + keypair
//!     cd tests-litesvm && cargo test -- --nocapture
//!
//! Without the artifact the tests print a SKIP notice and pass, so the crate
//! stays green in a toolchain-less environment.
//!
//! Coverage (task 4):
//!   a. double create_priority on the same mint -> fails
//!   b. bump with lamports = 0 -> ZeroAmount
//!   c. sweep with owed = 0 -> NothingToSweep
//!   d. sweep after bump -> treasury gets owed exactly, PDA rent-exempt, swept == paid
//!   e. claim with effective <= bar -> BelowBar
//!   f. claim of the mint already in the vetrina -> AlreadyHolder
//!   g. bump A, bump B > A, claim B (effective_B -= bar, paid_B unchanged); then
//!      claim A below the bar -> fails
//!   h. first-ever claim with effective = 1 -> passes (bar 0)
//!   i. substitution: a Priority-shaped account at a non-canonical address as
//!      candidate -> fails at account validation (seeds constraint)

use litesvm::types::{FailedTransactionMetadata, TransactionMetadata};
use litesvm::LiteSVM;
use sha2::{Digest, Sha256};
use solana_account::Account;
use solana_address::{address, Address};
use solana_instruction::{account_meta::AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;

// Anchor custom-error codes = 6000 + variant index (declaration order in VetrinaError).
const E_ZERO_AMOUNT: u32 = 6000;
const E_NOTHING_TO_SWEEP: u32 = 6001;
const E_BELOW_BAR: u32 = 6002;
const E_ALREADY_HOLDER: u32 = 6003;
// Anchor framework: ConstraintSeeds.
const E_CONSTRAINT_SEEDS: u32 = 2006;

const SPL_TOKEN: Address = address!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const SYSTEM: Address = address!("11111111111111111111111111111111");
const TREASURY: Address = address!("6omNt7ZZvew6hZn9N3cJpzYczL1F4Gp5azFzfvVKkDyD");

type TxResult = Result<TransactionMetadata, FailedTransactionMetadata>;

struct Env {
    svm: LiteSVM,
    program_id: Address,
    payer: Keypair,
}

/// Locate the workspace `target/deploy` artifacts. Returns None (with a SKIP
/// notice) when they are absent, so the crate stays green pre-build.
fn setup() -> Option<Env> {
    let deploy = format!("{}/../target/deploy", env!("CARGO_MANIFEST_DIR"));
    let so = format!("{deploy}/vetrina.so");
    let keypair = format!("{deploy}/vetrina-keypair.json");

    let bytes = match std::fs::read(&so) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("SKIP: {so} not found — run `anchor build` first");
            return None;
        }
    };
    let program_id = match std::fs::read_to_string(&keypair) {
        Ok(s) => program_id_from_keypair_json(&s),
        Err(_) => {
            eprintln!("SKIP: {keypair} not found — run `anchor build` first");
            return None;
        }
    };

    let mut svm = LiteSVM::new();
    svm.add_program(program_id, &bytes).unwrap();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    Some(Env {
        svm,
        program_id,
        payer,
    })
}

fn program_id_from_keypair_json(s: &str) -> Address {
    // Solana keypair JSON is a 64-byte array; the public key is the last 32.
    let nums: Vec<u8> = s
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(|t| t.trim().parse::<u8>().ok())
        .collect();
    assert_eq!(nums.len(), 64, "keypair json must hold 64 bytes");
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&nums[32..64]);
    Address::from(pk)
}

// ---- instruction encoding -------------------------------------------------

fn disc(name: &str) -> [u8; 8] {
    let h = Sha256::digest(format!("global:{name}").as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h[..8]);
    d
}

fn meta(pubkey: Address, signer: bool, writable: bool) -> AccountMeta {
    AccountMeta {
        pubkey,
        is_signer: signer,
        is_writable: writable,
    }
}

fn event_authority(program_id: Address) -> Address {
    Address::find_program_address(&[b"__event_authority"], &program_id).0
}

/// Append the two accounts that `#[event_cpi]` adds at the end of every context.
fn with_event_cpi(program_id: Address, mut accts: Vec<AccountMeta>) -> Vec<AccountMeta> {
    accts.push(meta(event_authority(program_id), false, false));
    accts.push(meta(program_id, false, false));
    accts
}

fn priority_pda(program_id: Address, mint: Address) -> (Address, u8) {
    Address::find_program_address(&[b"priority", mint.as_ref()], &program_id)
}

fn spotlight_pda(program_id: Address) -> (Address, u8) {
    Address::find_program_address(&[b"spotlight"], &program_id)
}

fn ix_create_priority(pid: Address, payer: Address, mint: Address) -> Instruction {
    let (priority, _) = priority_pda(pid, mint);
    Instruction {
        program_id: pid,
        accounts: with_event_cpi(
            pid,
            vec![
                meta(payer, true, true),
                meta(mint, false, false),
                meta(priority, false, true),
                meta(SYSTEM, false, false),
            ],
        ),
        data: disc("create_priority").to_vec(),
    }
}

fn ix_bump(pid: Address, payer: Address, mint: Address, amount: u64) -> Instruction {
    let (priority, _) = priority_pda(pid, mint);
    let mut data = disc("bump").to_vec();
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id: pid,
        accounts: with_event_cpi(
            pid,
            vec![
                meta(payer, true, true),
                meta(priority, false, true),
                meta(SYSTEM, false, false),
            ],
        ),
        data,
    }
}

fn ix_claim(pid: Address, payer: Address, candidate_mint: Address) -> Instruction {
    let (spotlight, _) = spotlight_pda(pid);
    let (candidate, _) = priority_pda(pid, candidate_mint);
    Instruction {
        program_id: pid,
        accounts: with_event_cpi(
            pid,
            vec![
                meta(payer, true, true),
                meta(spotlight, false, true),
                meta(candidate, false, true),
                meta(SYSTEM, false, false),
            ],
        ),
        data: disc("claim_spotlight").to_vec(),
    }
}

fn ix_sweep(pid: Address, mint: Address) -> Instruction {
    let (priority, _) = priority_pda(pid, mint);
    Instruction {
        program_id: pid,
        accounts: with_event_cpi(
            pid,
            vec![meta(priority, false, true), meta(TREASURY, false, true)],
        ),
        data: disc("sweep").to_vec(),
    }
}

fn send(env: &mut Env, ix: Instruction) -> TxResult {
    let payer_pk = env.payer.pubkey();
    let msg = Message::new(&[ix], Some(&payer_pk));
    let tx = Transaction::new(&[&env.payer], msg, env.svm.latest_blockhash());
    env.svm.send_transaction(tx)
}

fn assert_custom(res: TxResult, code: u32) {
    match res {
        Ok(_) => panic!("expected failure with Custom({code}), got success"),
        Err(f) => {
            let dbg = format!("{:?}", f.err);
            assert!(
                dbg.contains(&format!("Custom({code})")),
                "expected Custom({code}), got: {dbg}"
            );
        }
    }
}

// ---- account helpers ------------------------------------------------------

/// Place a minimal, initialized SPL-Token mint at a fresh address.
fn create_mint(env: &mut Env) -> Address {
    let mint = Address::new_unique();
    let mut data = vec![0u8; 82];
    data[44] = 9; // decimals
    data[45] = 1; // is_initialized = true (mint_authority/freeze_authority = None)
    let lamports = env.svm.minimum_balance_for_rent_exemption(82);
    env.svm
        .set_account(
            mint,
            Account {
                lamports,
                data,
                owner: SPL_TOKEN,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    mint
}

fn read_priority(env: &Env, mint: Address) -> (u64, u64, u64) {
    let (pda, _) = priority_pda(env.program_id, mint);
    let a = env.svm.get_account(&pda).expect("priority account");
    // 8 disc | mint(32) | paid(8) | effective(8) | swept(8) | bump(1)
    let paid = u64::from_le_bytes(a.data[40..48].try_into().unwrap());
    let effective = u64::from_le_bytes(a.data[48..56].try_into().unwrap());
    let swept = u64::from_le_bytes(a.data[56..64].try_into().unwrap());
    (paid, effective, swept)
}

fn priority_lamports(env: &Env, mint: Address) -> u64 {
    let (pda, _) = priority_pda(env.program_id, mint);
    env.svm.get_account(&pda).map(|a| a.lamports).unwrap_or(0)
}

fn treasury_lamports(env: &Env) -> u64 {
    env.svm.get_account(&TREASURY).map(|a| a.lamports).unwrap_or(0)
}

// ---- tests ----------------------------------------------------------------

#[test]
fn a_double_create_priority_fails() {
    let Some(mut env) = setup() else { return };
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let mint = create_mint(&mut env);
    send(&mut env, ix_create_priority(pid, payer, mint)).expect("first create ok");
    let second = send(&mut env, ix_create_priority(pid, payer, mint));
    assert!(second.is_err(), "second create_priority must fail (init)");
}

#[test]
fn b_bump_zero_is_zero_amount() {
    let Some(mut env) = setup() else { return };
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let mint = create_mint(&mut env);
    send(&mut env, ix_create_priority(pid, payer, mint)).expect("create ok");
    let res = send(&mut env, ix_bump(pid, payer, mint, 0));
    assert_custom(res, E_ZERO_AMOUNT);
}

#[test]
fn c_sweep_owed_zero_is_nothing_to_sweep() {
    let Some(mut env) = setup() else { return };
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let mint = create_mint(&mut env);
    send(&mut env, ix_create_priority(pid, payer, mint)).expect("create ok");
    let res = send(&mut env, ix_sweep(pid, mint));
    assert_custom(res, E_NOTHING_TO_SWEEP);
}

#[test]
fn d_sweep_after_bump_moves_exact_owed() {
    let Some(mut env) = setup() else { return };
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let mint = create_mint(&mut env);
    send(&mut env, ix_create_priority(pid, payer, mint)).expect("create ok");

    let amount = 5_000_000u64;
    send(&mut env, ix_bump(pid, payer, mint, amount)).expect("bump ok");

    // Ensure the treasury account exists so we can measure the delta cleanly.
    env.svm.airdrop(&TREASURY, 1_000_000).unwrap();
    let treasury_before = treasury_lamports(&env);
    let rent_min = {
        let (pda, _) = priority_pda(pid, mint);
        let len = env.svm.get_account(&pda).unwrap().data.len();
        env.svm.minimum_balance_for_rent_exemption(len)
    };

    send(&mut env, ix_sweep(pid, mint)).expect("sweep ok");

    let (paid, _eff, swept) = read_priority(&env, mint);
    assert_eq!(swept, paid, "I8: swept advances to paid");
    assert_eq!(swept, amount, "swept equals the bumped amount");
    assert_eq!(
        treasury_lamports(&env) - treasury_before,
        amount,
        "treasury receives exactly owed"
    );
    assert_eq!(
        priority_lamports(&env, mint),
        rent_min,
        "PDA left rent-exempt (exactly rent_min)"
    );
}

#[test]
fn e_claim_below_bar_fails() {
    let Some(mut env) = setup() else { return };
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    // Establish a holder A with a full bar during its lease.
    let a = create_mint(&mut env);
    send(&mut env, ix_create_priority(pid, payer, a)).expect("create a");
    send(&mut env, ix_bump(pid, payer, a, 100)).expect("bump a");
    send(&mut env, ix_claim(pid, payer, a)).expect("claim a (bar 0)");

    // B with effective below the (now full) bar.
    let b = create_mint(&mut env);
    send(&mut env, ix_create_priority(pid, payer, b)).expect("create b");
    send(&mut env, ix_bump(pid, payer, b, 50)).expect("bump b");
    let res = send(&mut env, ix_claim(pid, payer, b));
    assert_custom(res, E_BELOW_BAR);
}

#[test]
fn f_claim_already_holder_fails() {
    let Some(mut env) = setup() else { return };
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let a = create_mint(&mut env);
    send(&mut env, ix_create_priority(pid, payer, a)).expect("create a");
    send(&mut env, ix_bump(pid, payer, a, 100)).expect("bump a");
    send(&mut env, ix_claim(pid, payer, a)).expect("claim a");
    // A is already the holder -> I3.
    let res = send(&mut env, ix_claim(pid, payer, a));
    assert_custom(res, E_ALREADY_HOLDER);
}

#[test]
fn g_claim_consumes_bar_then_a_below_bar_fails() {
    let Some(mut env) = setup() else { return };
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let a = create_mint(&mut env);
    let b = create_mint(&mut env);
    send(&mut env, ix_create_priority(pid, payer, a)).expect("create a");
    send(&mut env, ix_create_priority(pid, payer, b)).expect("create b");
    send(&mut env, ix_bump(pid, payer, a, 30)).expect("bump a");
    send(&mut env, ix_bump(pid, payer, b, 50)).expect("bump b");

    let (paid_b_before, eff_b_before, _) = read_priority(&env, b);
    assert_eq!((paid_b_before, eff_b_before), (50, 50));

    // First claim -> bar is 0, so effective_B is decremented by 0, paid_B unchanged (I1/I4).
    send(&mut env, ix_claim(pid, payer, b)).expect("claim b");
    let (paid_b, eff_b, _) = read_priority(&env, b);
    assert_eq!(paid_b, 50, "I1: paid_B unchanged by claim");
    assert_eq!(eff_b, 50, "effective_B == paid_B - bar(0)");

    // B now holds with a full bar (50); A (30) is below it.
    let res = send(&mut env, ix_claim(pid, payer, a));
    assert_custom(res, E_BELOW_BAR);
}

#[test]
fn h_first_claim_effective_one_passes() {
    let Some(mut env) = setup() else { return };
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let m = create_mint(&mut env);
    send(&mut env, ix_create_priority(pid, payer, m)).expect("create");
    send(&mut env, ix_bump(pid, payer, m, 1)).expect("bump 1");
    send(&mut env, ix_claim(pid, payer, m)).expect("first claim with effective=1 must pass (bar 0)");

    let (spotlight, _) = spotlight_pda(pid);
    let s = env.svm.get_account(&spotlight).expect("spotlight");
    let snapshot = u64::from_le_bytes(s.data[40..48].try_into().unwrap());
    assert_eq!(snapshot, 1, "paid_snapshot = effective post-consumption (I4)");
}

#[test]
fn i_substituted_candidate_fails_validation() {
    let Some(mut env) = setup() else { return };
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let a = create_mint(&mut env);
    send(&mut env, ix_create_priority(pid, payer, a)).expect("create a");
    send(&mut env, ix_bump(pid, payer, a, 100)).expect("bump a");

    // Copy A's real Priority data to a NON-canonical address and pass that as
    // the candidate. Anchor re-derives [b"priority", mint] and rejects the
    // mismatch (seeds constraint).
    let (real_pda, _) = priority_pda(pid, a);
    let real = env.svm.get_account(&real_pda).unwrap();
    let fake_addr = Address::new_unique();
    env.svm.set_account(fake_addr, real).unwrap();

    let (spotlight, _) = spotlight_pda(pid);
    let ix = Instruction {
        program_id: pid,
        accounts: with_event_cpi(
            pid,
            vec![
                meta(payer, true, true),
                meta(spotlight, false, true),
                meta(fake_addr, false, true), // substituted candidate
                meta(SYSTEM, false, false),
            ],
        ),
        data: disc("claim_spotlight").to_vec(),
    };
    let res = send(&mut env, ix);
    assert_custom(res, E_CONSTRAINT_SEEDS);
}

/// Report compute units for the happy path (task 6). Prints with --nocapture.
#[test]
fn z_report_compute_units() {
    let Some(mut env) = setup() else { return };
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let m = create_mint(&mut env);

    let cu_create = send(&mut env, ix_create_priority(pid, payer, m)).unwrap().compute_units_consumed;
    let cu_bump = send(&mut env, ix_bump(pid, payer, m, 10_000_000)).unwrap().compute_units_consumed;
    let cu_claim = send(&mut env, ix_claim(pid, payer, m)).unwrap().compute_units_consumed;
    let cu_sweep = send(&mut env, ix_sweep(pid, m)).unwrap().compute_units_consumed;

    eprintln!("CU  create_priority = {cu_create}");
    eprintln!("CU  bump            = {cu_bump}");
    eprintln!("CU  claim_spotlight = {cu_claim}");
    eprintln!("CU  sweep           = {cu_sweep}");
}
