// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Crash-recovery integration tests for the durable, file-backed store.
//!
//! These corrupt the on-disk files directly and drive recovery through the
//! public `Iqdb` handle, pinning the contract from the engineering directives:
//! a corrupt write-ahead-log tail is truncated to the last known-good offset
//! (every record before the corruption survives, the log is left write-ready),
//! while a corrupt snapshot fails the open with `Error::Persist` rather than
//! loading partial state.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use iqdb::{DistanceMetric, Error, Iqdb, Vector, VectorId};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempDb {
    dir: PathBuf,
}

impl TempDb {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("iqdb-rec-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self { dir }
    }

    fn snapshot(&self) -> PathBuf {
        self.dir.join("db.iqdb")
    }

    /// The write-ahead log lives at `<snapshot>.wal`.
    fn wal(&self) -> PathBuf {
        let mut s: OsString = self.snapshot().into_os_string();
        s.push(".wal");
        PathBuf::from(s)
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
fn corrupt_wal_tail_is_truncated_and_prior_records_survive() {
    let tmp = TempDb::new();
    let snap = tmp.snapshot();

    // Two acknowledged writes (fsynced under the default Always policy), then
    // drop the handle without closing — so the records live in the WAL on top
    // of the empty initial snapshot.
    {
        let db = Iqdb::open(&snap, 2, DistanceMetric::Cosine).unwrap();
        db.upsert(VectorId::from(1u64), v(&[0.1, 0.2]), None)
            .unwrap();
        db.upsert(VectorId::from(2u64), v(&[0.3, 0.4]), None)
            .unwrap();
    }

    // Simulate a torn write: append junk after the last good frame.
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(tmp.wal())
            .expect("wal exists after acknowledged writes");
        f.write_all(&[0xAB; 13]).unwrap();
        f.sync_all().unwrap();
    }

    // Reopen: the torn tail is discarded, both prior records are recovered.
    let db = Iqdb::open(&snap, 2, DistanceMetric::Cosine).unwrap();
    assert_eq!(db.len(), 2);
    assert!(db.get(&VectorId::from(1u64)).unwrap().is_some());
    assert!(db.get(&VectorId::from(2u64)).unwrap().is_some());

    // The log is write-ready again: a further write + clean close persists.
    db.upsert(VectorId::from(3u64), v(&[0.5, 0.6]), None)
        .unwrap();
    db.close().unwrap();

    let db = Iqdb::open(&snap, 2, DistanceMetric::Cosine).unwrap();
    assert_eq!(db.len(), 3);
}

#[test]
fn corrupt_snapshot_fails_the_open() {
    let tmp = TempDb::new();
    let snap = tmp.snapshot();

    // Write and compact so the data lives in the snapshot, then drop.
    {
        let db = Iqdb::open(&snap, 2, DistanceMetric::Cosine).unwrap();
        db.upsert(VectorId::from(1u64), v(&[1.0, 0.0]), None)
            .unwrap();
        db.close().unwrap();
    }

    // Flip a byte deep in the payload region — past the header, so the CRC32
    // over the payload no longer matches.
    let mut bytes = std::fs::read(&snap).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    std::fs::write(&snap, &bytes).unwrap();

    let err = Iqdb::open(&snap, 2, DistanceMetric::Cosine).unwrap_err();
    assert!(matches!(err, Error::Persist(_)), "got {err:?}");
}

#[test]
fn garbage_file_is_not_a_database() {
    let tmp = TempDb::new();
    let snap = tmp.snapshot();
    std::fs::write(&snap, b"this is not an iqdb snapshot file at all").unwrap();

    let err = Iqdb::open(&snap, 2, DistanceMetric::Cosine).unwrap_err();
    assert!(matches!(err, Error::Persist(_)), "got {err:?}");
}
