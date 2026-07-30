//! Integration-level property tests for `bar()` (the decaying claim threshold),
//! adapted to the reference signature `bar(paid_snapshot, lease_end, now, decay: u32)`.
//!
//! P1 full while `now < lease_end` · P2 zero once `now >= lease_end + decay`
//! · P3 monotone non-increasing in `now` · P4 never above `paid_snapshot`.
//! (These mirror the crate-internal `#[cfg(test)] mod tests` in lib.rs and
//! additionally exercise `bar` through the public API.)

use proptest::prelude::*;
use vetrina::{bar, MAX_DECAY_SECS, MIN_DECAY_SECS};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// P4: never above the snapshot, for any inputs.
    #[test]
    fn p4_never_above_snapshot(
        snap in any::<u64>(),
        lease_end in any::<i64>(),
        now in any::<i64>(),
        decay in MIN_DECAY_SECS..=MAX_DECAY_SECS,
    ) {
        prop_assert!(bar(snap, lease_end, now, decay) <= snap);
    }

    /// P1: full while the lease is active (`now < lease_end`).
    #[test]
    fn p1_full_during_lease(
        snap in any::<u64>(),
        lease_end in (i64::MIN / 2)..(i64::MAX / 2),
        delta in 1i64..1_000_000_000,
        decay in MIN_DECAY_SECS..=MAX_DECAY_SECS,
    ) {
        let now = lease_end - delta; // now < lease_end
        prop_assert_eq!(bar(snap, lease_end, now, decay), snap);
    }

    /// P2: zero once decay is complete (`now >= lease_end + decay`).
    #[test]
    fn p2_zero_after_full_decay(
        snap in any::<u64>(),
        lease_end in 0i64..(i64::MAX / 4),
        extra in 0i64..1_000_000_000,
        decay in MIN_DECAY_SECS..=MAX_DECAY_SECS,
    ) {
        let now = lease_end + decay as i64 + extra;
        prop_assert_eq!(bar(snap, lease_end, now, decay), 0);
    }

    /// P3: monotone non-increasing in `now`.
    #[test]
    fn p3_monotone_non_increasing(
        snap in any::<u64>(),
        lease_end in 0i64..1_000_000_000,
        t1 in 0i64..3_000_000_000,
        dt in 0i64..1_000_000,
        decay in MIN_DECAY_SECS..=MAX_DECAY_SECS,
    ) {
        let t2 = t1 + dt;
        prop_assert!(bar(snap, lease_end, t2, decay) <= bar(snap, lease_end, t1, decay));
    }

    /// Inside the ramp the value is the exact integer linear interpolation.
    #[test]
    fn ramp_is_linear(
        snap in 0u64..u64::MAX / 2,
        lease_end in 0i64..1_000_000_000,
        decay in 2u32..=MAX_DECAY_SECS,
        elapsed in 1i64..(MAX_DECAY_SECS as i64),
    ) {
        prop_assume!(elapsed < decay as i64);
        let now = lease_end + elapsed;
        let expected = ((snap as u128) * ((decay as i64 - elapsed) as u128) / decay as u128) as u64;
        prop_assert_eq!(bar(snap, lease_end, now, decay), expected);
    }
}

// ---- Exact-boundary edge cases -------------------------------------------

#[test]
fn edge_at_lease_end_is_full() {
    // now == lease_end: enters the decay branch with elapsed == 0 -> full bar.
    assert_eq!(bar(1000, 500, 500, 100), 1000);
}

#[test]
fn edge_ramps_and_reaches_zero() {
    assert_eq!(bar(1000, 500, 550, 100), 500); // halfway
    assert_eq!(bar(1000, 500, 600, 100), 0); // exactly lease_end + decay
    assert_eq!(bar(1000, 500, 601, 100), 0); // beyond
}

#[test]
fn edge_zero_snapshot_is_zero_everywhere() {
    assert_eq!(bar(0, 0, -10, 100), 0);
    assert_eq!(bar(0, 0, 10, 100), 0);
}
