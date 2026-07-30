//! litesvm integration tests for the vetrina program.
//!
//! Build the artifact first:
//!
//!     anchor build           # -> target/deploy/vetrina.so + keypair
//!     cd tests-litesvm && cargo test -- --nocapture
//!
//! Without the artifact the tests print a SKIP notice and pass, so the crate
//! stays green in a toolchain-less environment.
//!
//! Every scenario starts from `initialize(treasury, decay, lease_duration)`.
//!
//! Coverage (task 4 + j/k/l):
//!   a. double create_priority on the same mint -> fails
//!   b. bump with lamports = 0 -> ZeroAmount
//!   c. sweep with owed = 0 -> NothingToSweep
//!   d. sweep after bump -> treasury gets owed exactly, PDA rent-exempt, swept == paid
//!   e. claim with effective <= bar -> BelowBar
//!   f. claim of the mint already in the vetrina -> AlreadyHolder
//!   g. bump A, bump B > A, claim B (effective_B -= bar, paid_B unchanged); then
//!      claim A below the bar -> fails
//!   h. initialize -> create_priority -> bump(1) -> claim (bar 0) -> passes
//!   i. substituted candidate at a non-canonical address -> seeds constraint
//!   j. update_config with decay out of cap -> ParamOutOfBounds
//!   k. update_config signed by a non-authority -> has_one violation
//!   l. sweep toward an account != config.treasury -> address constraint

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
const E_PARAM_OUT_OF_BOUNDS: u32 = 6000;
const E_ZERO_AMOUNT: u32 = 6001;
const E_BELOW_BAR: u32 = 6002;
const E_ALREADY_HOLDER: u32 = 6003;
const E_NOTHING_TO_SWEEP: u32 = 6006;
// Anchor framework constraint codes.
const E_CONSTRAINT_HAS_ONE: u32 = 2001;
const E_CONSTRAINT_SEEDS: u32 = 2006;
const E_CONSTRAINT_ADDRESS: u32 = 2012;

// Must equal `declare_id!` in programs/vetrina/src/lib.rs: the compiled .so has
// this id baked in, so we deploy it here and derive every PDA from it. (The
// target/deploy keypair is only for real on-chain deploy and may not match it
// on a fresh CI build — declare_id is the source of truth.)
const PROGRAM_ID: Address = address!("gCxZar2tTSVKE1amCvMW5BcjMXvbqGRCf52n5P15gM4");
const SPL_TOKEN: Address = address!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const SYSTEM: Address = address!("11111111111111111111111111111111");

const DECAY: u32 = 3_600;
const LEASE: i64 = 3_600;

type TxResult = Result<TransactionMetadata, FailedTransactionMetadata>;

struct Env {
    svm: LiteSVM,
    program_id: Address,
    authority: Keypair,
    payer: Keypair,
    treasury: Address,
}

/// Load the compiled program at its declared id. Returns None (with a SKIP
/// notice) when the .so is absent, so the crate stays green pre-build.
fn setup() -> Option<Env> {
    let so = format!("{}/../target/deploy/vetrina.so", env!("CARGO_MANIFEST_DIR"));
    let bytes = match std::fs::read(&so) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("SKIP: {so} not found — run `anchor build` first");
            return None;
        }
    };

    let mut svm = LiteSVM::new();
    svm.add_program(PROGRAM_ID, &bytes).unwrap();
    let authority = Keypair::new();
    let payer = Keypair::new();
    svm.airdrop(&authority.pubkey(), 100_000_000_000).unwrap();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    let treasury = Address::new_unique();
    Some(Env {
        svm,
        program_id: PROGRAM_ID,
        authority,
        payer,
        treasury,
    })
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

fn event_authority(pid: Address) -> Address {
    Address::find_program_address(&[b"__event_authority"], &pid).0
}

/// Append the two accounts that `#[event_cpi]` adds at the end of a context.
fn with_event_cpi(pid: Address, mut accts: Vec<AccountMeta>) -> Vec<AccountMeta> {
    accts.push(meta(event_authority(pid), false, false));
    accts.push(meta(pid, false, false));
    accts
}

fn config_pda(pid: Address) -> (Address, u8) {
    Address::find_program_address(&[b"config"], &pid)
}

fn spotlight_pda(pid: Address) -> (Address, u8) {
    Address::find_program_address(&[b"spotlight"], &pid)
}

fn priority_pda(pid: Address, mint: Address) -> (Address, u8) {
    Address::find_program_address(&[b"priority", mint.as_ref()], &pid)
}

fn ix_initialize(pid: Address, authority: Address, treasury: Address, decay: u32, lease: i64) -> Instruction {
    let (config, _) = config_pda(pid);
    let (spotlight, _) = spotlight_pda(pid);
    let mut data = disc("initialize").to_vec();
    data.extend_from_slice(treasury.as_ref());
    data.extend_from_slice(&decay.to_le_bytes());
    data.extend_from_slice(&lease.to_le_bytes());
    Instruction {
        program_id: pid,
        accounts: vec![
            meta(authority, true, true),
            meta(config, false, true),
            meta(spotlight, false, true),
            meta(SYSTEM, false, false),
        ],
        data,
    }
}

fn ix_update_config(pid: Address, authority: Address, treasury: Address, decay: u32, lease: i64) -> Instruction {
    let (config, _) = config_pda(pid);
    let mut data = disc("update_config").to_vec();
    data.extend_from_slice(treasury.as_ref());
    data.extend_from_slice(&decay.to_le_bytes());
    data.extend_from_slice(&lease.to_le_bytes());
    Instruction {
        program_id: pid,
        accounts: vec![meta(authority, true, true), meta(config, false, true)],
        data,
    }
}

fn ix_create_priority(pid: Address, payer: Address, mint: Address) -> Instruction {
    let (priority, _) = priority_pda(pid, mint);
    Instruction {
        program_id: pid,
        accounts: vec![
            meta(payer, true, true),
            meta(mint, false, false),
            meta(priority, false, true),
            meta(SYSTEM, false, false),
        ],
        data: disc("create_priority").to_vec(),
    }
}

fn ix_bump(pid: Address, payer: Address, mint: Address, lamports: u64) -> Instruction {
    let (priority, _) = priority_pda(pid, mint);
    let mut data = disc("bump").to_vec();
    data.extend_from_slice(&lamports.to_le_bytes());
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

fn ix_sweep(pid: Address, mint: Address, treasury: Address) -> Instruction {
    let (config, _) = config_pda(pid);
    let (priority, _) = priority_pda(pid, mint);
    Instruction {
        program_id: pid,
        accounts: with_event_cpi(
            pid,
            vec![
                meta(config, false, false),
                meta(priority, false, true),
                meta(treasury, false, true),
            ],
        ),
        data: disc("sweep").to_vec(),
    }
}

fn ix_claim(pid: Address, payer: Address, candidate_mint: Address) -> Instruction {
    let (config, _) = config_pda(pid);
    let (spotlight, _) = spotlight_pda(pid);
    let (candidate, _) = priority_pda(pid, candidate_mint);
    Instruction {
        program_id: pid,
        accounts: with_event_cpi(
            pid,
            vec![
                meta(payer, true, true),
                meta(config, false, false),
                meta(spotlight, false, true),
                meta(candidate, false, true),
            ],
        ),
        data: disc("claim_spotlight").to_vec(),
    }
}

fn send(svm: &mut LiteSVM, ix: Instruction, signer: &Keypair) -> TxResult {
    // Advance the blockhash so otherwise-identical transactions (e.g. a repeated
    // claim/create) get distinct signatures and are not rejected as duplicates
    // (AlreadyProcessed) before the program runs.
    svm.expire_blockhash();
    let msg = Message::new(&[ix], Some(&signer.pubkey()));
    let tx = Transaction::new(&[signer], msg, svm.latest_blockhash());
    svm.send_transaction(tx)
}

fn init(env: &mut Env, decay: u32, lease: i64) -> TxResult {
    let ix = ix_initialize(env.program_id, env.authority.pubkey(), env.treasury, decay, lease);
    send(&mut env.svm, ix, &env.authority)
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

fn lamports_of(env: &Env, addr: Address) -> u64 {
    env.svm.get_account(&addr).map(|a| a.lamports).unwrap_or(0)
}

// ---- tests ----------------------------------------------------------------

#[test]
fn a_double_create_priority_fails() {
    let Some(mut env) = setup() else { return };
    init(&mut env, DECAY, LEASE).expect("initialize");
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let mint = create_mint(&mut env);
    send(&mut env.svm, ix_create_priority(pid, payer, mint), &env.payer).expect("first create ok");
    let second = send(&mut env.svm, ix_create_priority(pid, payer, mint), &env.payer);
    assert!(second.is_err(), "second create_priority must fail (init)");
}

#[test]
fn b_bump_zero_is_zero_amount() {
    let Some(mut env) = setup() else { return };
    init(&mut env, DECAY, LEASE).expect("initialize");
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let mint = create_mint(&mut env);
    send(&mut env.svm, ix_create_priority(pid, payer, mint), &env.payer).expect("create ok");
    let res = send(&mut env.svm, ix_bump(pid, payer, mint, 0), &env.payer);
    assert_custom(res, E_ZERO_AMOUNT);
}

#[test]
fn c_sweep_owed_zero_is_nothing_to_sweep() {
    let Some(mut env) = setup() else { return };
    init(&mut env, DECAY, LEASE).expect("initialize");
    let (pid, payer, treasury) = (env.program_id, env.payer.pubkey(), env.treasury);
    let mint = create_mint(&mut env);
    send(&mut env.svm, ix_create_priority(pid, payer, mint), &env.payer).expect("create ok");
    let res = send(&mut env.svm, ix_sweep(pid, mint, treasury), &env.payer);
    assert_custom(res, E_NOTHING_TO_SWEEP);
}

#[test]
fn d_sweep_after_bump_moves_exact_owed() {
    let Some(mut env) = setup() else { return };
    init(&mut env, DECAY, LEASE).expect("initialize");
    let (pid, payer, treasury) = (env.program_id, env.payer.pubkey(), env.treasury);
    let mint = create_mint(&mut env);
    send(&mut env.svm, ix_create_priority(pid, payer, mint), &env.payer).expect("create ok");

    let amount = 5_000_000u64;
    send(&mut env.svm, ix_bump(pid, payer, mint, amount), &env.payer).expect("bump ok");

    // Ensure the treasury account exists so we can measure the delta cleanly.
    env.svm.airdrop(&treasury, 1_000_000).unwrap();
    let treasury_before = lamports_of(&env, treasury);
    let rent_min = {
        let (pda, _) = priority_pda(pid, mint);
        let len = env.svm.get_account(&pda).unwrap().data.len();
        env.svm.minimum_balance_for_rent_exemption(len)
    };

    send(&mut env.svm, ix_sweep(pid, mint, treasury), &env.payer).expect("sweep ok");

    let (paid, _eff, swept) = read_priority(&env, mint);
    assert_eq!(swept, paid, "I8: swept advances to paid");
    assert_eq!(swept, amount, "swept equals the bumped amount");
    assert_eq!(
        lamports_of(&env, treasury) - treasury_before,
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
    init(&mut env, DECAY, LEASE).expect("initialize");
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    // Establish a holder A with a full bar during its lease.
    let a = create_mint(&mut env);
    send(&mut env.svm, ix_create_priority(pid, payer, a), &env.payer).expect("create a");
    send(&mut env.svm, ix_bump(pid, payer, a, 100), &env.payer).expect("bump a");
    send(&mut env.svm, ix_claim(pid, payer, a), &env.payer).expect("claim a (bar 0)");

    // B with effective below the (now full) bar.
    let b = create_mint(&mut env);
    send(&mut env.svm, ix_create_priority(pid, payer, b), &env.payer).expect("create b");
    send(&mut env.svm, ix_bump(pid, payer, b, 50), &env.payer).expect("bump b");
    let res = send(&mut env.svm, ix_claim(pid, payer, b), &env.payer);
    assert_custom(res, E_BELOW_BAR);
}

#[test]
fn f_claim_already_holder_fails() {
    let Some(mut env) = setup() else { return };
    init(&mut env, DECAY, LEASE).expect("initialize");
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let a = create_mint(&mut env);
    send(&mut env.svm, ix_create_priority(pid, payer, a), &env.payer).expect("create a");
    send(&mut env.svm, ix_bump(pid, payer, a, 100), &env.payer).expect("bump a");
    send(&mut env.svm, ix_claim(pid, payer, a), &env.payer).expect("claim a");
    // A is already the holder -> I3.
    let res = send(&mut env.svm, ix_claim(pid, payer, a), &env.payer);
    assert_custom(res, E_ALREADY_HOLDER);
}

#[test]
fn g_claim_consumes_bar_then_a_below_bar_fails() {
    let Some(mut env) = setup() else { return };
    init(&mut env, DECAY, LEASE).expect("initialize");
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let a = create_mint(&mut env);
    let b = create_mint(&mut env);
    send(&mut env.svm, ix_create_priority(pid, payer, a), &env.payer).expect("create a");
    send(&mut env.svm, ix_create_priority(pid, payer, b), &env.payer).expect("create b");
    send(&mut env.svm, ix_bump(pid, payer, a, 30), &env.payer).expect("bump a");
    send(&mut env.svm, ix_bump(pid, payer, b, 50), &env.payer).expect("bump b");

    let (paid_b_before, eff_b_before, _) = read_priority(&env, b);
    assert_eq!((paid_b_before, eff_b_before), (50, 50));

    // First claim -> bar is 0, so effective_B is decremented by 0, paid_B unchanged (I1/I4).
    send(&mut env.svm, ix_claim(pid, payer, b), &env.payer).expect("claim b");
    let (paid_b, eff_b, _) = read_priority(&env, b);
    assert_eq!(paid_b, 50, "I1: paid_B unchanged by claim");
    assert_eq!(eff_b, 50, "effective_B == paid_B - bar(0)");

    // B now holds with a full bar (50); A (30) is below it.
    let res = send(&mut env.svm, ix_claim(pid, payer, a), &env.payer);
    assert_custom(res, E_BELOW_BAR);
}

#[test]
fn h_first_claim_effective_one_passes() {
    let Some(mut env) = setup() else { return };
    init(&mut env, DECAY, LEASE).expect("initialize");
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let m = create_mint(&mut env);
    send(&mut env.svm, ix_create_priority(pid, payer, m), &env.payer).expect("create");
    send(&mut env.svm, ix_bump(pid, payer, m, 1), &env.payer).expect("bump 1");
    send(&mut env.svm, ix_claim(pid, payer, m), &env.payer)
        .expect("first claim with effective=1 must pass (bar 0)");

    let (spotlight, _) = spotlight_pda(pid);
    let s = env.svm.get_account(&spotlight).expect("spotlight");
    let snapshot = u64::from_le_bytes(s.data[40..48].try_into().unwrap());
    assert_eq!(snapshot, 1, "paid_snapshot = effective post-consumption (I4)");
}

#[test]
fn i_substituted_candidate_fails_validation() {
    let Some(mut env) = setup() else { return };
    init(&mut env, DECAY, LEASE).expect("initialize");
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let a = create_mint(&mut env);
    send(&mut env.svm, ix_create_priority(pid, payer, a), &env.payer).expect("create a");
    send(&mut env.svm, ix_bump(pid, payer, a, 100), &env.payer).expect("bump a");

    // Copy A's real Priority data to a NON-canonical address and pass that as
    // the candidate. Anchor re-derives [b"priority", mint] and rejects it.
    let (real_pda, _) = priority_pda(pid, a);
    let real = env.svm.get_account(&real_pda).unwrap();
    let fake_addr = Address::new_unique();
    env.svm.set_account(fake_addr, real).unwrap();

    let (config, _) = config_pda(pid);
    let (spotlight, _) = spotlight_pda(pid);
    let ix = Instruction {
        program_id: pid,
        accounts: with_event_cpi(
            pid,
            vec![
                meta(payer, true, true),
                meta(config, false, false),
                meta(spotlight, false, true),
                meta(fake_addr, false, true), // substituted candidate
            ],
        ),
        data: disc("claim_spotlight").to_vec(),
    };
    let res = send(&mut env.svm, ix, &env.payer);
    assert_custom(res, E_CONSTRAINT_SEEDS);
}

#[test]
fn j_update_config_out_of_cap_fails() {
    let Some(mut env) = setup() else { return };
    init(&mut env, DECAY, LEASE).expect("initialize");
    let (pid, authority, treasury) = (env.program_id, env.authority.pubkey(), env.treasury);
    // decay = 30 < MIN_DECAY_SECS (60).
    let ix = ix_update_config(pid, authority, treasury, 30, LEASE);
    let res = send(&mut env.svm, ix, &env.authority);
    assert_custom(res, E_PARAM_OUT_OF_BOUNDS);
}

#[test]
fn k_update_config_non_authority_fails() {
    let Some(mut env) = setup() else { return };
    init(&mut env, DECAY, LEASE).expect("initialize");
    let (pid, treasury) = (env.program_id, env.treasury);
    // A funded impostor signs, but it is not config.authority.
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let ix = ix_update_config(pid, attacker.pubkey(), treasury, DECAY, LEASE);
    let res = send(&mut env.svm, ix, &attacker);
    assert_custom(res, E_CONSTRAINT_HAS_ONE);
}

#[test]
fn l_sweep_wrong_treasury_fails() {
    let Some(mut env) = setup() else { return };
    init(&mut env, DECAY, LEASE).expect("initialize");
    let (pid, payer) = (env.program_id, env.payer.pubkey());
    let mint = create_mint(&mut env);
    send(&mut env.svm, ix_create_priority(pid, payer, mint), &env.payer).expect("create");
    send(&mut env.svm, ix_bump(pid, payer, mint, 5_000_000), &env.payer).expect("bump");

    // Sweep toward an address != config.treasury.
    let wrong = Address::new_unique();
    env.svm.airdrop(&wrong, 1_000_000).unwrap();
    let res = send(&mut env.svm, ix_sweep(pid, mint, wrong), &env.payer);
    assert_custom(res, E_CONSTRAINT_ADDRESS);
}

/// Report compute units for the happy path (task 6). Prints with --nocapture.
#[test]
fn z_report_compute_units() {
    let Some(mut env) = setup() else { return };
    let cu_init = init(&mut env, DECAY, LEASE).unwrap().compute_units_consumed;
    let (pid, payer, treasury) = (env.program_id, env.payer.pubkey(), env.treasury);
    let m = create_mint(&mut env);

    let cu_create = send(&mut env.svm, ix_create_priority(pid, payer, m), &env.payer).unwrap().compute_units_consumed;
    let cu_bump = send(&mut env.svm, ix_bump(pid, payer, m, 10_000_000), &env.payer).unwrap().compute_units_consumed;
    let cu_claim = send(&mut env.svm, ix_claim(pid, payer, m), &env.payer).unwrap().compute_units_consumed;
    let cu_sweep = send(&mut env.svm, ix_sweep(pid, m, treasury), &env.payer).unwrap().compute_units_consumed;

    eprintln!("CU  initialize      = {cu_init}");
    eprintln!("CU  create_priority = {cu_create}");
    eprintln!("CU  bump            = {cu_bump}");
    eprintln!("CU  claim_spotlight = {cu_claim}");
    eprintln!("CU  sweep           = {cu_sweep}");
}
