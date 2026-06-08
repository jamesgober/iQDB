// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Integration tests for the async surface (`async` feature).
//!
//! The whole file is gated on the feature, so it compiles to nothing when
//! `async` is off. It checks that the async durable path is consistent with
//! the sync one: an awaited write survives a close + reopen.

#![cfg(feature = "async")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use iqdb::{AsyncIqdb, DistanceMetric, Vector, VectorId};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDb {
    dir: PathBuf,
}

impl TempDb {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("iqdb-async-{}-{n}", std::process::id()));
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

#[tokio::test]
async fn async_durable_round_trip() {
    let tmp = TempDb::new();
    let path = tmp.path();

    {
        let db = AsyncIqdb::open(&path, 2, DistanceMetric::Cosine)
            .await
            .unwrap();
        db.upsert(VectorId::from(1u64), v(&[0.1, 0.2]), None)
            .await
            .unwrap();
        db.upsert(VectorId::from(2u64), v(&[0.3, 0.4]), None)
            .await
            .unwrap();
        db.flush().await.unwrap();
        db.close().await.unwrap();
    }

    let db = AsyncIqdb::open(&path, 2, DistanceMetric::Cosine)
        .await
        .unwrap();
    assert_eq!(db.len(), 2);
    let (got, _) = db
        .get(VectorId::from(1u64))
        .await
        .unwrap()
        .expect("present");
    assert_eq!(got.as_slice(), &[0.1, 0.2]);
}

#[tokio::test]
async fn async_reopen_dim_mismatch_is_rejected() {
    let tmp = TempDb::new();
    let path = tmp.path();

    {
        let db = AsyncIqdb::open(&path, 3, DistanceMetric::Cosine)
            .await
            .unwrap();
        db.upsert(VectorId::from(1u64), v(&[1.0, 0.0, 0.0]), None)
            .await
            .unwrap();
        db.close().await.unwrap();
    }

    let err = AsyncIqdb::open(&path, 4, DistanceMetric::Cosine)
        .await
        .unwrap_err();
    assert!(matches!(err, iqdb::Error::Config(_)), "got {err:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn async_concurrent_searches() {
    let db = std::sync::Arc::new(
        AsyncIqdb::open_in_memory(2, DistanceMetric::Euclidean)
            .await
            .unwrap(),
    );
    for i in 0..20u64 {
        db.upsert(VectorId::from(i), v(&[i as f32, 0.0]), None)
            .await
            .unwrap();
    }

    let mut handles = Vec::new();
    for target in 0..20u64 {
        let db = std::sync::Arc::clone(&db);
        handles.push(tokio::spawn(async move {
            let hits = db.search(v(&[target as f32, 0.0]), 1).await.unwrap();
            (target, hits[0].id.clone())
        }));
    }
    for handle in handles {
        let (target, id) = handle.await.unwrap();
        assert_eq!(id, VectorId::from(target));
    }
}
