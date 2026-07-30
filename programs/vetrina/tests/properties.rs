//! Property tests for `bar()` — the decaying entry bar.
//!
//! P1 full during lease · P2 zero after full decay · P3 monotone non-increasing
//! · P4 never above `paid_snapshot`. Plus the exact-boundary edge cases.

use proptest::prelude::*;
use vetrina::{bar, DECAY_SECONDS, LEASE_SECONDS};

// Bounded ranges keep `now - lease_end` inside i64 without wrapping; `bar`
// itself is total (saturating/checked), so these are just to keep the
// generated scenarios meaningful.
const T: i64 = 1_000_000_000; // ~1e9 seconds
const D: i64 = 1_000_000_000; // max decay window explored

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// P4: the bar is never above the snapshot, for any inputs.
    #[test]
    fn p4_never_above_snapshot(
        paid in any::<u64>(),
        lease_end in -T..T,
        now in (-2 * T)..(2 * T),
        decay in 1i64..D,
    ) {
        prop_assert!(bar(paid, lease_end, now, decay) <= paid);
    }

    /// P1: full (== paid) for the whole lease, i.e. every `now <= lease_end`.
    #[test]
    fn p1_full_during_lease(
        paid in any::<u64>(),
        lease_end in -T..T,
        delta in 0i64..(2 * T), // now = lease_end - delta  =>  now <= lease_end
        decay in 1i64..D,
    ) {
        let now = lease_end - delta;
        prop_assert_eq!(bar(paid, lease_end, now, decay), paid);
    }

    /// P2: zero once decay is complete (`elapsed >= decay`).
    #[test]
    fn p2_zero_after_full_decay(
        paid in any::<u64>(),
        lease_end in -T..T,
        extra in 0i64..T,
        decay in 1i64..D,
    ) {
        let now = lease_end + decay + extra; // elapsed = decay + extra >= decay
        prop_assert_eq!(bar(paid, lease_end, now, decay), 0);
    }

    /// P3: monotone non-increasing in `now`.
    #[test]
    fn p3_monotone_non_increasing(
        paid in any::<u64>(),
        lease_end in -T..T,
        n1 in (-2 * T)..(2 * T),
        step in 0i64..D,
        decay in 1i64..D,
    ) {
        let n2 = n1.saturating_add(step); // n2 >= n1
        prop_assert!(bar(paid, lease_end, n1, decay) >= bar(paid, lease_end, n2, decay));
    }

    /// Bonus: strictly inside the ramp, the bar is a faithful linear interp
    /// (bounded above by paid and, one step later, not larger).
    #[test]
    fn ramp_is_linear_and_bounded(
        paid in 0u64..u64::MAX / 2,
        lease_end in -T..T,
        elapsed in 1i64..(D - 1),
        decay in 2i64..D,
    ) {
        prop_assume!(elapsed < decay);
        let now = lease_end + elapsed;
        let expected = ((paid as u128) * ((decay - elapsed) as u128) / decay as u128) as u64;
        prop_assert_eq!(bar(paid, lease_end, now, decay), expected);
    }
}

// ---- Exact-boundary edge cases -------------------------------------------

#[test]
fn edge_at_lease_end_is_full() {
    // now == lease_end exactly -> elapsed 0 -> FULL bar (documented, intended).
    assert_eq!(bar(1_000, 5_000, 5_000, DECAY_SECONDS), 1_000);
}

#[test]
fn edge_one_second_into_decay_is_below_full() {
    let b = bar(1_000, 5_000, 5_001, DECAY_SECONDS);
    assert!(b < 1_000, "bar should drop immediately after lease_end");
    assert!(b > 0, "bar should not be zero one second in");
}

#[test]
fn edge_exact_decay_end_is_zero() {
    assert_eq!(bar(1_000, 5_000, 5_000 + DECAY_SECONDS, DECAY_SECONDS), 0);
}

#[test]
fn edge_one_before_decay_end_is_nonzero() {
    let b = bar(DECAY_SECONDS as u64, 0, DECAY_SECONDS - 1, DECAY_SECONDS);
    assert_eq!(b, 1); // paid=decay, remaining=1 -> decay*1/decay = 1
}

#[test]
fn edge_zero_snapshot_is_zero_everywhere() {
    assert_eq!(bar(0, 0, -10, DECAY_SECONDS), 0);
    assert_eq!(bar(0, 0, 10, DECAY_SECONDS), 0);
}

#[test]
fn edge_degenerate_decay_window() {
    // decay <= 0: full while now <= lease_end, else zero (no div-by-zero).
    assert_eq!(bar(500, 100, 100, 0), 500); // now == lease_end -> full
    assert_eq!(bar(500, 100, 101, 0), 0); // past lease_end -> zero
    assert_eq!(bar(500, 100, 101, -5), 0);
}

#[test]
fn lease_and_decay_constants_are_sane() {
    assert!(LEASE_SECONDS > 0 && DECAY_SECONDS > 0);
}
