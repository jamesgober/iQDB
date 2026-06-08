// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Integration tests for the durable, file-backed store.
//!
//! These exercise the contract iqdb inherits from `iqdb-persist`: a write
//! that is acknowledged (and flushed, or merely logged under the default
//! always-fsync policy) survives a process restart, the snapshot + WAL pair
//! reconstructs the in-memory state on reopen, and a reopen that disagrees
//! with the stored shape fails loudly rather than loading wrong state.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use iqdb::{
    DistanceMetric, IndexKind, Iqdb, IqdbConfig, IvfConfig, Metadata, Value, Vector, VectorId,
};

/// A unique temp directory holding one database file, removed on drop.
struct TempDb {
    dir: PathBuf,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

impl TempDb {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("iqdb-it-{}-{n}", std::process::id()));
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

fn v(xs: &[f32]) -> Vector {
    Vector::new(xs.to_vec()).unwrap()
}

#[test]
fn create_upsert_close_reopen_round_trip() {
    let tmp = TempDb::new();
    let path = tmp.path();

    {
        let db = Iqdb::open(&path, 3, DistanceMetric::Cosine).unwrap();
        db.upsert(VectorId::from(1u64), v(&[0.1, 0.2, 0.3]), None)
            .unwrap();
        db.upsert(VectorId::from(2u64), v(&[0.9, 0.0, 0.1]), None)
            .unwrap();
        assert_eq!(db.len(), 2);
        db.close().unwrap();
    }

    let db = Iqdb::open(&path, 3, DistanceMetric::Cosine).unwrap();
    assert_eq!(db.len(), 2);
    let (got, _) = db.get(&VectorId::from(1u64)).unwrap().expect("present");
    assert_eq!(got.as_slice(), &[0.1, 0.2, 0.3]);
}

#[test]
fn recovery_without_close_replays_the_wal() {
    let tmp = TempDb::new();
    let path = tmp.path();

    {
        // No flush, no close — just drop the handle. The default always-fsync
        // WAL policy means each acknowledged upsert is already durable.
        let db = Iqdb::open(&path, 2, DistanceMetric::Euclidean).unwrap();
        db.upsert(VectorId::from(7u64), v(&[1.0, 2.0]), None)
            .unwrap();
        db.upsert(VectorId::from(8u64), v(&[3.0, 4.0]), None)
            .unwrap();
    }

    let db = Iqdb::open(&path, 2, DistanceMetric::Euclidean).unwrap();
    assert_eq!(db.len(), 2);
    assert!(db.get(&VectorId::from(7u64)).unwrap().is_some());
    assert!(db.get(&VectorId::from(8u64)).unwrap().is_some());
}

#[test]
fn delete_persists_across_reopen() {
    let tmp = TempDb::new();
    let path = tmp.path();

    {
        let db = Iqdb::open(&path, 1, DistanceMetric::Euclidean).unwrap();
        db.upsert(VectorId::from(1u64), v(&[1.0]), None).unwrap();
        db.upsert(VectorId::from(2u64), v(&[2.0]), None).unwrap();
        assert!(db.delete(&VectorId::from(1u64)).unwrap());
        db.close().unwrap();
    }

    let db = Iqdb::open(&path, 1, DistanceMetric::Euclidean).unwrap();
    assert_eq!(db.len(), 1);
    assert!(db.get(&VectorId::from(1u64)).unwrap().is_none());
    assert!(db.get(&VectorId::from(2u64)).unwrap().is_some());
}

#[test]
fn metadata_survives_round_trip() {
    let tmp = TempDb::new();
    let path = tmp.path();
    let meta: Metadata = [
        ("kind".to_string(), Value::String("doc".into())),
        ("year".to_string(), Value::Int(2026)),
        ("score".to_string(), Value::Float(0.875)),
        ("ok".to_string(), Value::Bool(true)),
    ]
    .into_iter()
    .collect();

    {
        let db = Iqdb::open(&path, 2, DistanceMetric::Cosine).unwrap();
        db.upsert(VectorId::from(1u64), v(&[1.0, 0.0]), Some(meta.clone()))
            .unwrap();
        db.close().unwrap();
    }

    let db = Iqdb::open(&path, 2, DistanceMetric::Cosine).unwrap();
    let (_, got_meta) = db.get(&VectorId::from(1u64)).unwrap().expect("present");
    assert_eq!(got_meta.as_ref(), Some(&meta));
}

#[test]
fn search_against_recovered_data() {
    let tmp = TempDb::new();
    let path = tmp.path();

    {
        let db = Iqdb::open(&path, 2, DistanceMetric::Euclidean).unwrap();
        db.upsert(VectorId::from(1u64), v(&[0.0, 0.0]), None)
            .unwrap();
        db.upsert(VectorId::from(2u64), v(&[10.0, 10.0]), None)
            .unwrap();
        db.close().unwrap();
    }

    let db = Iqdb::open(&path, 2, DistanceMetric::Euclidean).unwrap();
    let hits = db.search(&v(&[0.1, 0.1]), 2).unwrap();
    assert_eq!(hits[0].id, VectorId::from(1u64));
    assert_eq!(hits[1].id, VectorId::from(2u64));
}

#[test]
fn reopen_with_wrong_dim_is_rejected() {
    let tmp = TempDb::new();
    let path = tmp.path();

    {
        let db = Iqdb::open(&path, 3, DistanceMetric::Cosine).unwrap();
        db.upsert(VectorId::from(1u64), v(&[1.0, 0.0, 0.0]), None)
            .unwrap();
        db.close().unwrap();
    }

    let err = Iqdb::open(&path, 4, DistanceMetric::Cosine).unwrap_err();
    assert!(matches!(err, iqdb::Error::Config(_)), "got {err:?}");
}

#[test]
fn reopen_with_wrong_metric_is_rejected() {
    let tmp = TempDb::new();
    let path = tmp.path();

    {
        let db = Iqdb::open(&path, 2, DistanceMetric::Cosine).unwrap();
        db.upsert(VectorId::from(1u64), v(&[1.0, 0.0]), None)
            .unwrap();
        db.close().unwrap();
    }

    let err = Iqdb::open(&path, 2, DistanceMetric::Euclidean).unwrap_err();
    assert!(matches!(err, iqdb::Error::Config(_)), "got {err:?}");
}

#[test]
fn ivf_database_persists_index_kind_and_searches_after_reopen() {
    let tmp = TempDb::new();
    let path = tmp.path();
    let cfg = IqdbConfig::new(2, DistanceMetric::Euclidean).index(IndexKind::Ivf(
        IvfConfig::default()
            .with_n_clusters(2)
            .with_n_probes(2)
            .with_training_sample_size(64)
            .with_seed(7),
    ));

    {
        let db = Iqdb::open_with(&path, cfg).unwrap();
        for (i, p) in [[0.0, 0.0], [0.1, -0.1], [10.0, 10.0], [9.9, 10.1]]
            .iter()
            .enumerate()
        {
            db.upsert(VectorId::from(i as u64), v(p), None).unwrap();
        }
        // Search materializes + trains the IVF index, then close compacts it.
        let hits = db.search(&v(&[0.0, 0.0]), 1).unwrap();
        assert_eq!(hits[0].id, VectorId::from(0u64));
        db.close().unwrap();
    }

    // Reopen with a plain Flat request: the stored IVF kind wins (it is part
    // of the database identity), and search still works after the rebuild.
    let db = Iqdb::open(&path, 2, DistanceMetric::Euclidean).unwrap();
    assert_eq!(db.len(), 4);
    let hits = db.search(&v(&[10.0, 10.0]), 1).unwrap();
    assert_eq!(hits[0].id, VectorId::from(2u64));
}

#[test]
fn multiple_sessions_accumulate() {
    let tmp = TempDb::new();
    let path = tmp.path();

    for i in 0..5u64 {
        let db = Iqdb::open(&path, 1, DistanceMetric::Euclidean).unwrap();
        db.upsert(VectorId::from(i), v(&[i as f32]), None).unwrap();
        db.close().unwrap();
    }

    let db = Iqdb::open(&path, 1, DistanceMetric::Euclidean).unwrap();
    assert_eq!(db.len(), 5);
}

#[test]
fn path_helper_is_used() {
    // Touch the helper so the lint profile sees it exercised.
    let tmp = TempDb::new();
    let _p: &Path = &tmp.dir;
    assert!(tmp.path().ends_with("db.iqdb"));
}
