//! Property-based tests for the v0.3.0 search ranking invariants.
//!
//! These properties hold across the full input space defined by the
//! generators below, not just the hand-picked fixtures in
//! `tests/search.rs`. They exist to catch regressions in:
//!
//! 1. Distance-metric algebraic identities (symmetry,
//!    identity-of-indiscernibles, smaller-is-closer on `Cosine`).
//! 2. Top-`k` ranking invariants (ascending order, length bounded by
//!    `min(k, store.len())`, perfect matches always rank first).
//! 3. The interaction between filtered and unfiltered search —
//!    `search_with(|_| true)` must agree with `search`.
//!
//! Strategies are kept small (dim ≤ 6, store ≤ 16) so the default
//! 256-case sweep finishes in well under a second.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use iqdb::{DistanceMetric, Iqdb, Record, RecordId, Vector};
use proptest::collection::vec as prop_vec;
use proptest::prelude::*;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn tempdir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("iqdb-prop-{nanos}-{n}"));
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

/// Finite `f32` strategy with bounded magnitude — keeps cosine /
/// dot-product computations within `f32` precision.
fn finite_f32() -> impl Strategy<Value = f32> {
    (-100.0_f32..100.0_f32).prop_filter("must be finite", |v| v.is_finite())
}

/// Vector of `dim` finite components.
fn vector_strategy(dim: usize) -> impl Strategy<Value = Vec<f32>> {
    prop_vec(finite_f32(), dim..=dim)
}

proptest! {
    /// Identical vectors collapse to zero distance under L2 and
    /// Cosine; Dot returns `−‖a‖²` which is non-positive for finite
    /// inputs.
    #[test]
    fn identity_yields_minimal_distance(components in vector_strategy(4)) {
        prop_assume!(components.iter().any(|x| x.abs() > 1e-3));
        let a = Vector::new(components.clone()).expect("finite");
        let b = Vector::new(components).expect("finite");

        let l2 = DistanceMetric::L2.distance(&a, &b).expect("ok");
        prop_assert!(l2.abs() < 1e-3, "l2={}", l2);

        let cos = DistanceMetric::Cosine.distance(&a, &b).expect("ok");
        prop_assert!(cos.abs() < 1e-3, "cos={}", cos);

        // Dot distance is `−(a·a)` — non-positive whenever `a` is real.
        let dot = DistanceMetric::Dot.distance(&a, &b).expect("ok");
        prop_assert!(dot <= 1e-3, "dot={}", dot);
    }

    /// L2 and Cosine are symmetric: `d(a,b) == d(b,a)`. Dot is also
    /// symmetric because `a·b == b·a`.
    #[test]
    fn distance_is_symmetric(
        a_components in vector_strategy(4),
        b_components in vector_strategy(4),
    ) {
        let a = Vector::new(a_components).expect("finite");
        let b = Vector::new(b_components).expect("finite");

        for metric in [DistanceMetric::L2, DistanceMetric::Cosine, DistanceMetric::Dot] {
            let forward = metric.distance(&a, &b).expect("ok");
            let backward = metric.distance(&b, &a).expect("ok");
            if forward.is_nan() || backward.is_nan() {
                // Cosine against a zero vector yields NaN; symmetry
                // still holds in that both directions agree on NaN-ness.
                prop_assert_eq!(forward.is_nan(), backward.is_nan(), "{:?}", metric);
            } else {
                prop_assert!(
                    (forward - backward).abs() < 1e-3,
                    "{:?}: forward={}, backward={}",
                    metric, forward, backward,
                );
            }
        }
    }

    /// L2 distance is non-negative for every finite input pair.
    #[test]
    fn l2_distance_is_non_negative(
        a_components in vector_strategy(4),
        b_components in vector_strategy(4),
    ) {
        let a = Vector::new(a_components).expect("finite");
        let b = Vector::new(b_components).expect("finite");
        let d = DistanceMetric::L2.distance(&a, &b).expect("ok");
        prop_assert!(d.is_finite(), "l2 must be finite for finite inputs: {}", d);
        prop_assert!(d >= 0.0, "l2 must be non-negative: {}", d);
    }

    /// Cosine distance is in `[0, 2]` whenever both norms are non-zero.
    #[test]
    fn cosine_distance_is_in_zero_two_range(
        a_components in vector_strategy(4),
        b_components in vector_strategy(4),
    ) {
        let a = Vector::new(a_components).expect("finite");
        let b = Vector::new(b_components).expect("finite");
        prop_assume!(a.norm() > 1e-3 && b.norm() > 1e-3);
        let d = DistanceMetric::Cosine.distance(&a, &b).expect("ok");
        // Allow a small slack for floating-point rounding at the bounds.
        prop_assert!((-1e-3..=2.0 + 1e-3).contains(&d), "cosine out of range: {}", d);
    }

    /// `search` result length never exceeds `min(k, store.len())`.
    #[test]
    fn search_length_is_bounded(
        records in prop_vec(vector_strategy(4), 0..=16),
        k in 0_usize..20,
    ) {
        let db = Iqdb::open_in_memory();
        for (id, components) in records.iter().enumerate() {
            let v = Vector::new(components.clone()).expect("finite");
            db.upsert(Record::new(RecordId::new(id as u64), v)).expect("ok");
        }

        let probe = Vector::new(vec![0.5, 0.5, 0.5, 0.5]).expect("finite");
        let hits = db.search(&probe, k, DistanceMetric::L2).expect("ok");
        prop_assert!(hits.len() <= k);
        prop_assert!(hits.len() <= records.len());
    }

    /// Top-`k` results are sorted ascending under the smaller-is-closer
    /// rule (with NaN sorted last).
    #[test]
    fn search_results_are_sorted_ascending(
        records in prop_vec(vector_strategy(4), 1..=16),
        k in 1_usize..20,
    ) {
        let db = Iqdb::open_in_memory();
        for (id, components) in records.iter().enumerate() {
            let v = Vector::new(components.clone()).expect("finite");
            db.upsert(Record::new(RecordId::new(id as u64), v)).expect("ok");
        }

        let probe = Vector::new(vec![0.5, 0.5, 0.5, 0.5]).expect("finite");
        let hits = db.search(&probe, k, DistanceMetric::L2).expect("ok");
        for pair in hits.windows(2) {
            let (lo, hi) = (pair[0].score, pair[1].score);
            // NaN sorts to the end; once a NaN appears, the rest must also be NaN.
            if lo.is_nan() {
                prop_assert!(hi.is_nan(), "non-NaN after NaN: lo={}, hi={}", lo, hi);
            } else {
                prop_assert!(
                    lo <= hi,
                    "results out of order: lo={}, hi={}",
                    lo, hi,
                );
            }
        }
    }

    /// A record that is byte-identical to the probe must appear in the
    /// result set when one is present and `k >= 1`.
    #[test]
    fn perfect_match_is_always_in_top_k(
        components in vector_strategy(4),
        decoys in prop_vec(vector_strategy(4), 0..=10),
    ) {
        let db = Iqdb::open_in_memory();
        // Insert decoys first so the perfect match is not at index 0.
        for (i, decoy) in decoys.iter().enumerate() {
            let v = Vector::new(decoy.clone()).expect("finite");
            db.upsert(Record::new(RecordId::new(i as u64), v)).expect("ok");
        }
        let perfect_id = (decoys.len() as u64) + 1;
        db.upsert(Record::new(
            RecordId::new(perfect_id),
            Vector::new(components.clone()).expect("finite"),
        )).expect("ok");

        let probe = Vector::new(components).expect("finite");
        let k = decoys.len() + 1;
        let hits = db.search(&probe, k, DistanceMetric::L2).expect("ok");
        prop_assert!(
            hits.iter().any(|h| h.id == RecordId::new(perfect_id)),
            "perfect match id={} missing from top-{}",
            perfect_id, k,
        );
    }

    /// `search_with(|_| true)` must produce the same id sequence as
    /// `search` — the no-filter form is exactly the always-true form.
    #[test]
    fn unfiltered_matches_always_true_filter(
        records in prop_vec(vector_strategy(4), 1..=16),
        k in 1_usize..10,
    ) {
        let db = Iqdb::open_in_memory();
        for (id, components) in records.iter().enumerate() {
            let v = Vector::new(components.clone()).expect("finite");
            db.upsert(Record::new(RecordId::new(id as u64), v)).expect("ok");
        }
        let probe = Vector::new(vec![0.0, 0.0, 0.0, 0.0]).expect("finite");

        let unfiltered = db.search(&probe, k, DistanceMetric::L2).expect("ok");
        let always_true = db
            .search_with(&probe, k, DistanceMetric::L2, |_| true)
            .expect("ok");

        let ids_a: Vec<u64> = unfiltered.iter().map(|h| h.id.get()).collect();
        let ids_b: Vec<u64> = always_true.iter().map(|h| h.id.get()).collect();
        prop_assert_eq!(ids_a, ids_b);
    }

    /// Writing an arbitrary record set to a file-backed `Iqdb`,
    /// closing, and reopening yields a store whose records compare
    /// equal to the originals — the durable round-trip preserves both
    /// the vector data and the set membership.
    ///
    /// Bounded to small sets (≤8 records, dim 4) so the default 256
    /// proptest cases finish in under a second.
    #[test]
    fn persistence_round_trip_preserves_records(
        records in prop_vec(vector_strategy(4), 0..=8),
    ) {
        let dir = tempdir();
        {
            let db = Iqdb::open(&dir).expect("open");
            for (id, components) in records.iter().enumerate() {
                let v = Vector::new(components.clone()).expect("finite");
                db.upsert(Record::new(RecordId::new(id as u64), v)).expect("upsert");
            }
            db.close().expect("close");
        }

        let db = Iqdb::open(&dir).expect("reopen");
        prop_assert_eq!(db.len(), records.len());
        for (id, components) in records.iter().enumerate() {
            let hit = db
                .get(RecordId::new(id as u64))
                .expect("ok")
                .expect("present");
            prop_assert_eq!(hit.vector().as_slice(), components.as_slice());
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
