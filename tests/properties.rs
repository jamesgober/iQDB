// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Property-based tests for the defining invariants.
//!
//! Two guarantees from the engineering directives are checked here against
//! arbitrary input rather than hand-picked cases: deterministic ranking on
//! the exact flat path (results sorted nearest-first, bounded by `k`, no
//! duplicate ids) and the durable round-trip (an arbitrary record set
//! survives `open → upsert → close → reopen` unchanged).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use iqdb::{DistanceMetric, Iqdb, Vector, VectorId};
use proptest::collection::vec as pvec;
use proptest::prelude::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDb {
    dir: PathBuf,
}

impl TempDb {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("iqdb-prop-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self { dir }
    }

    fn path(&self) -> PathBuf {
        self.dir.join("db.iqdb")
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Flat top-`k` results are sorted by distance ascending, never exceed
    /// `k` (or the corpus size), and contain no duplicate ids.
    #[test]
    fn flat_search_is_sorted_bounded_and_unique(
        rows in pvec((0u64..200, pvec(-50.0f32..50.0, 4..=4)), 1..40),
        query in pvec(-50.0f32..50.0, 4..=4),
        k in 1usize..12,
    ) {
        let db = Iqdb::open_in_memory(4, DistanceMetric::Euclidean).unwrap();
        for (id, comps) in &rows {
            db.upsert(VectorId::from(*id), Vector::new(comps.clone()).unwrap(), None).unwrap();
        }
        let hits = db.search(&Vector::new(query).unwrap(), k).unwrap();

        prop_assert!(hits.len() <= k);
        prop_assert!(hits.len() <= db.len());
        for window in hits.windows(2) {
            prop_assert!(window[0].distance <= window[1].distance);
        }
        let mut ids: Vec<u64> = hits
            .iter()
            .map(|h| match h.id {
                VectorId::U64(n) => n,
                VectorId::Bytes(_) => unreachable!("u64 ids only in this test"),
            })
            .collect();
        let unique = ids.len();
        ids.sort_unstable();
        ids.dedup();
        prop_assert_eq!(ids.len(), unique);
    }
}

proptest! {
    // File I/O per case: keep the case count modest.
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// An arbitrary record set survives a close + reopen unchanged. Repeated
    /// ids resolve last-write-wins, matching `upsert` semantics.
    #[test]
    fn durable_round_trip_preserves_records(
        rows in pvec((0u64..40, pvec(-10.0f32..10.0, 3..=3)), 0..16),
    ) {
        let tmp = TempDb::new();
        let path = tmp.path();

        let mut expected: HashMap<u64, Vec<f32>> = HashMap::new();
        {
            let db = Iqdb::open(&path, 3, DistanceMetric::Cosine).unwrap();
            for (id, comps) in &rows {
                db.upsert(VectorId::from(*id), Vector::new(comps.clone()).unwrap(), None).unwrap();
                let _ = expected.insert(*id, comps.clone());
            }
            db.close().unwrap();
        }

        let db = Iqdb::open(&path, 3, DistanceMetric::Cosine).unwrap();
        prop_assert_eq!(db.len(), expected.len());
        for (id, comps) in &expected {
            let (got, _) = db.get(&VectorId::from(*id)).unwrap().expect("present");
            prop_assert_eq!(got.as_slice(), comps.as_slice());
        }
    }
}
