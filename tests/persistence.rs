//! Integration tests for the v0.4.0 file-backed persistence surface.
//!
//! These tests exercise the durable lifecycle through the public
//! `Iqdb::open(path)` entry point — they validate the full handle,
//! not just the internal `FileStore` (which has its own unit tests
//! inside the crate). Tempdirs are created per-test with monotonic
//! nanosecond names so a parallel test run does not stomp itself.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use iqdb::{DistanceMetric, Error, Iqdb, Payload, PayloadValue, Record, RecordId, Vector};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn tempdir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("iqdb-int-{nanos}-{n}"));
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

fn cleanup(path: &PathBuf) {
    let _ = std::fs::remove_dir_all(path);
}

fn record(id: u64, components: Vec<f32>) -> Record {
    Record::new(RecordId::new(id), Vector::new(components).unwrap())
}

#[test]
fn open_on_fresh_directory_creates_empty_database() {
    let dir = tempdir();
    let child = dir.join("nested");
    {
        let db = Iqdb::open(&child).expect("open");
        assert!(db.is_empty());
        assert_eq!(db.len(), 0);
    }
    assert!(child.is_dir());
    cleanup(&dir);
}

#[test]
fn upserted_records_survive_close_and_reopen() {
    let dir = tempdir();
    {
        let db = Iqdb::open(&dir).unwrap();
        db.upsert(record(1, vec![0.1, 0.2, 0.3])).unwrap();
        db.upsert(record(2, vec![1.0, 0.0, 0.0])).unwrap();
        db.upsert(record(3, vec![0.0, 1.0, 0.0])).unwrap();
        db.close().unwrap();
    }
    let db = Iqdb::open(&dir).unwrap();
    assert_eq!(db.len(), 3);
    let hit = db.get(RecordId::new(2)).unwrap().expect("present");
    assert_eq!(hit.vector().as_slice(), &[1.0, 0.0, 0.0]);
    cleanup(&dir);
}

#[test]
fn deletes_survive_close_and_reopen() {
    let dir = tempdir();
    {
        let db = Iqdb::open(&dir).unwrap();
        db.upsert(record(1, vec![1.0, 0.0])).unwrap();
        db.upsert(record(2, vec![0.0, 1.0])).unwrap();
        let _ = db.delete(RecordId::new(1)).unwrap();
        db.close().unwrap();
    }
    let db = Iqdb::open(&dir).unwrap();
    assert_eq!(db.len(), 1);
    assert!(db.get(RecordId::new(1)).unwrap().is_none());
    assert!(db.get(RecordId::new(2)).unwrap().is_some());
    cleanup(&dir);
}

#[test]
fn payload_round_trips_through_persistence() {
    let dir = tempdir();
    {
        let db = Iqdb::open(&dir).unwrap();
        let mut p = Payload::new();
        let _ = p.insert("kind", "doc");
        let _ = p.insert("year", 2026_i64);
        let _ = p.insert("score", 0.97_f64);
        let _ = p.insert("verified", true);
        let _ = p.insert("blob", PayloadValue::Bytes(vec![1, 2, 3, 4]));

        db.upsert(Record::with_payload(
            RecordId::new(7),
            Vector::new(vec![0.5, 0.5, 0.5]).unwrap(),
            p,
        ))
        .unwrap();
        db.close().unwrap();
    }
    let db = Iqdb::open(&dir).unwrap();
    let hit = db.get(RecordId::new(7)).unwrap().expect("present");
    let payload = hit.payload().expect("payload preserved");
    assert_eq!(
        payload.get("kind").and_then(PayloadValue::as_text),
        Some("doc")
    );
    assert_eq!(
        payload.get("year").and_then(PayloadValue::as_int),
        Some(2026)
    );
    assert!(payload
        .get("score")
        .and_then(PayloadValue::as_float)
        .map(|f| (f - 0.97).abs() < 1e-9)
        .unwrap_or(false));
    assert_eq!(
        payload.get("verified").and_then(PayloadValue::as_bool),
        Some(true)
    );
    assert_eq!(
        payload
            .get("blob")
            .and_then(PayloadValue::as_bytes)
            .map(<[u8]>::to_vec),
        Some(vec![1, 2, 3, 4])
    );
    cleanup(&dir);
}

#[test]
fn flush_without_close_recovers_via_wal_replay() {
    let dir = tempdir();
    {
        let db = Iqdb::open(&dir).unwrap();
        db.upsert(record(1, vec![1.0, 0.0])).unwrap();
        db.upsert(record(2, vec![0.0, 1.0])).unwrap();
        db.flush().unwrap();
        // No `close()` — the handle is dropped, the WAL is left
        // un-truncated. Recovery on next open replays it on top of
        // the (empty, default) snapshot.
    }
    let db = Iqdb::open(&dir).unwrap();
    assert_eq!(db.len(), 2);
    assert!(db.get(RecordId::new(1)).unwrap().is_some());
    assert!(db.get(RecordId::new(2)).unwrap().is_some());
    cleanup(&dir);
}

#[test]
fn upsert_without_flush_or_close_is_still_replayed_on_reopen() {
    // The OS page cache typically keeps the WAL append visible to a
    // subsequent open even without an fsync — durability against a
    // power cut requires `flush`, but a clean process exit followed
    // by a new process opening the same path sees the appended data.
    let dir = tempdir();
    {
        let db = Iqdb::open(&dir).unwrap();
        db.upsert(record(1, vec![1.0, 0.0])).unwrap();
        // Drop without flush or close.
    }
    let db = Iqdb::open(&dir).unwrap();
    assert_eq!(db.len(), 1);
    assert_eq!(
        db.get(RecordId::new(1)).unwrap().map(|r| r.id().get()),
        Some(1)
    );
    cleanup(&dir);
}

#[test]
fn open_rejects_existing_file_path() {
    let dir = tempdir();
    let file_path = dir.join("not-a-directory");
    std::fs::write(&file_path, b"data").unwrap();
    let err = Iqdb::open(&file_path).unwrap_err();
    assert!(matches!(err, Error::InvalidConfig(_)));
    cleanup(&dir);
}

#[test]
fn search_works_against_recovered_data() {
    let dir = tempdir();
    {
        let db = Iqdb::open(&dir).unwrap();
        db.upsert(record(1, vec![1.0, 0.0, 0.0])).unwrap();
        db.upsert(record(2, vec![0.0, 1.0, 0.0])).unwrap();
        db.upsert(record(3, vec![0.0, 0.0, 1.0])).unwrap();
        db.close().unwrap();
    }
    let db = Iqdb::open(&dir).unwrap();
    let probe = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
    let hits = db.search(&probe, 2, DistanceMetric::Cosine).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].id, RecordId::new(1));
    cleanup(&dir);
}

#[test]
fn multiple_reopen_cycles_preserve_state() {
    let dir = tempdir();
    for round in 0..5_u64 {
        let db = Iqdb::open(&dir).unwrap();
        db.upsert(record(round, vec![round as f32, 0.0, 0.0]))
            .unwrap();
        db.close().unwrap();
    }
    let db = Iqdb::open(&dir).unwrap();
    assert_eq!(db.len(), 5);
    for round in 0..5_u64 {
        assert!(db.get(RecordId::new(round)).unwrap().is_some());
    }
    cleanup(&dir);
}

#[test]
fn close_truncates_wal_to_zero_bytes() {
    let dir = tempdir();
    {
        let db = Iqdb::open(&dir).unwrap();
        for id in 0..50_u64 {
            db.upsert(record(id, vec![id as f32, 0.0])).unwrap();
        }
        db.close().unwrap();
    }
    let wal_len = std::fs::metadata(dir.join("wal")).unwrap().len();
    assert_eq!(wal_len, 0, "WAL must be empty after compaction");
    let snap_len = std::fs::metadata(dir.join("snap")).unwrap().len();
    assert!(snap_len > 0, "snapshot must contain the records");
    cleanup(&dir);
}

#[test]
fn corrupt_snapshot_surfaces_corrupt_error() {
    let dir = tempdir();
    {
        let db = Iqdb::open(&dir).unwrap();
        db.upsert(record(1, vec![1.0, 0.0])).unwrap();
        db.close().unwrap();
    }
    // Overwrite the snapshot with bytes that fail the magic check.
    std::fs::write(dir.join("snap"), b"NOPE\x01\x00\x00\x00").unwrap();
    let err = Iqdb::open(&dir).unwrap_err();
    assert!(matches!(err, Error::Corrupt { .. }));
    cleanup(&dir);
}

#[test]
fn corrupt_wal_tail_is_truncated_silently() {
    let dir = tempdir();
    {
        let db = Iqdb::open(&dir).unwrap();
        db.upsert(record(1, vec![1.0, 0.0])).unwrap();
        db.flush().unwrap();
    }
    // Append junk to the WAL. The next open should recover the
    // single committed record and truncate the garbage.
    use std::io::Write;
    let mut wal = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("wal"))
        .unwrap();
    wal.write_all(&[0xFFu8; 64]).unwrap();
    wal.sync_all().unwrap();
    drop(wal);

    let db = Iqdb::open(&dir).unwrap();
    assert_eq!(db.len(), 1);
    // After open, the WAL is back to a clean state and new writes
    // succeed.
    db.upsert(record(2, vec![0.0, 1.0])).unwrap();
    db.close().unwrap();
    cleanup(&dir);
}
