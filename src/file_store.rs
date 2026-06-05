// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Directory-backed durable store for `iqdb`.
//!
//! A `FileStore` lives in a directory and owns three files:
//!
//! ```text
//! <path>/
//! ├── snap   — most recent durable snapshot
//! └── wal    — write-ahead log of changes since the snapshot
//! ```
//!
//! ## Read path
//!
//! Every record is mirrored in an in-memory `HashMap<RecordId, Record>`
//! (the same [`MemoryStore`] that backs `Iqdb::open_in_memory`).
//! Reads — `get`, `with_records` (the search kernel's entry point) —
//! go straight to memory; the snapshot and WAL are only consulted at
//! open time and at compaction.
//!
//! ## Write path
//!
//! `upsert` and `delete`:
//!
//! 1. Encode the op into a framed WAL entry (length + body + CRC32).
//! 2. Append the entry to the WAL file (no `fsync` yet — caller calls
//!    [`Iqdb::flush`](crate::Iqdb::flush) on a cadence that matches
//!    the application's durability budget).
//! 3. Apply the change to the in-memory map.
//!
//! If the WAL append fails (disk full, I/O error), the in-memory map
//! is left untouched and the error is propagated. This means the
//! in-memory map and the durable log can briefly diverge between the
//! WAL append and the map update — but only in the direction of
//! "WAL has it, memory does not", which the next open reconciles
//! by replaying the WAL on top of the snapshot.
//!
//! ## Open / recover
//!
//! `FileStore::open` rebuilds the in-memory map by:
//!
//! 1. Reading the snapshot (if it exists) into the map.
//! 2. Replaying the WAL (if it exists) on top of the map.
//!
//! Replay stops at the first corrupt frame — anything written after
//! the corruption is discarded as un-replayable, but every record
//! that was durably written before is recovered.
//!
//! ## Close / compact
//!
//! [`Iqdb::close`](crate::Iqdb::close) on a file-backed handle calls
//! `compact`:
//!
//! 1. Acquire the write lock so the in-memory map cannot change.
//! 2. Write a fresh snapshot to `<path>/snap.tmp`.
//! 3. `full_sync` the snapshot file.
//! 4. Atomically rename `snap.tmp` → `snap` (`MoveFileExW` /
//!    `rename(2)`).
//! 5. Truncate the WAL to zero bytes.
//! 6. `full_sync` the WAL.
//!
//! After this point, the snapshot is the durable source of truth and
//! the WAL is empty. If the process crashes during compaction, the
//! next open sees the previous snapshot and the un-truncated WAL —
//! recovery is idempotent.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use crate::codec::{self, Op};
use crate::error::{Error, Result};
use crate::platform::full_sync;
use crate::record::{Record, RecordId};

const SNAP_FILE: &str = "snap";
const WAL_FILE: &str = "wal";
const SNAP_TMP_FILE: &str = "snap.tmp";

/// Directory-backed durable store.
///
/// Holds the in-memory map mirroring the durable state and the open
/// WAL handle. The struct itself is `Send + Sync`; concurrent reads
/// share the `RwLock` read guard, concurrent writes serialise on the
/// WAL `Mutex` *and* the map `RwLock`.
#[derive(Debug)]
pub(crate) struct FileStore {
    root: PathBuf,
    /// In-memory mirror of the durable state. The `RwLock` here is
    /// the same `std::sync::RwLock<HashMap<…>>` shape as
    /// [`crate::store::MemoryStore`] uses, so the search kernel's
    /// `with_records` closure works against both backends with the
    /// same borrow semantics.
    records: RwLock<HashMap<RecordId, Record>>,
    /// Serialised access to the WAL file. Held only for the duration
    /// of a single `write_all` + `flush` pair, so concurrent readers
    /// are not blocked by writers.
    wal: Mutex<WalHandle>,
}

#[derive(Debug)]
struct WalHandle {
    file: File,
    /// Reusable encode buffer. Lives on the heap once so the
    /// steady-state upsert path is allocation-free above the
    /// `Vec::extend_from_slice` that the codec uses internally.
    scratch: Vec<u8>,
}

impl FileStore {
    /// Open or create a directory-backed store at `path`.
    ///
    /// Behaviour:
    ///
    /// - If `path` does not exist, it is created as a directory.
    /// - If `path` exists and is not a directory,
    ///   [`Error::InvalidConfig`] is returned.
    /// - If `<path>/snap` exists, it is loaded into the in-memory map.
    /// - If `<path>/wal` exists, it is replayed on top of the map.
    /// - The WAL is opened in append mode and kept open for
    ///   subsequent writes.
    ///
    /// Replay stops at the first corrupt WAL frame — records up to
    /// that point are recovered; the corrupt frame and everything
    /// after it are dropped. The WAL is then truncated to the last
    /// known-good offset so future writes overwrite the corruption.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        ensure_directory(path)?;

        let snap_path = path.join(SNAP_FILE);
        let wal_path = path.join(WAL_FILE);

        let mut records: HashMap<RecordId, Record> = HashMap::new();

        if snap_path.exists() {
            load_snapshot(&snap_path, &mut records)?;
        }

        let valid_wal_len = if wal_path.exists() {
            replay_wal(&wal_path, &mut records)?
        } else {
            0
        };

        // Truncate any trailing corruption so the next append is
        // contiguous with the recovered history.
        if wal_path.exists() {
            truncate_to(&wal_path, valid_wal_len)?;
        }

        let wal_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)?;

        Ok(Self {
            root: path.to_path_buf(),
            records: RwLock::new(records),
            wal: Mutex::new(WalHandle {
                file: wal_file,
                scratch: Vec::with_capacity(1024),
            }),
        })
    }

    /// Append an upsert frame to the WAL, then update the in-memory map.
    pub(crate) fn upsert(&self, record: Record) -> Result<()> {
        {
            let mut wal = self.wal_lock();
            // Split the borrow so `scratch` (immutable) and `file`
            // (mutable) can be borrowed simultaneously through the
            // same guard.
            let WalHandle {
                ref mut file,
                ref mut scratch,
            } = *wal;
            scratch.clear();
            codec::write_frame(scratch, &Op::Upsert(record.clone()));
            file.write_all(scratch)?;
        }

        let mut guard = self.write_records();
        let _previous = guard.insert(record.id(), record);
        Ok(())
    }

    /// Append a delete frame to the WAL, then remove from the in-memory map.
    ///
    /// Returns whether the id was present before the call.
    pub(crate) fn delete(&self, id: RecordId) -> Result<bool> {
        {
            let mut wal = self.wal_lock();
            let WalHandle {
                ref mut file,
                ref mut scratch,
            } = *wal;
            scratch.clear();
            codec::write_frame(scratch, &Op::Delete(id));
            file.write_all(scratch)?;
        }

        let mut guard = self.write_records();
        Ok(guard.remove(&id).is_some())
    }

    /// Look up by id. Cloned out so the read lock is released before
    /// the value reaches the caller.
    pub(crate) fn get(&self, id: RecordId) -> Result<Option<Record>> {
        let guard = self.read_records();
        Ok(guard.get(&id).cloned())
    }

    /// Number of records currently held.
    pub(crate) fn len(&self) -> usize {
        self.read_records().len()
    }

    /// `true` if no records are stored.
    pub(crate) fn is_empty(&self) -> bool {
        self.read_records().is_empty()
    }

    /// Run `f` against the in-memory record map under the read lock.
    ///
    /// Same shape as [`crate::store::MemoryStore::with_records`] — the
    /// search kernel binds to both via a single enum-dispatch match
    /// in [`crate::db`].
    pub(crate) fn with_records<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&HashMap<RecordId, Record>) -> R,
    {
        let guard = self.read_records();
        f(&guard)
    }

    /// Drive the underlying WAL file to durable storage.
    ///
    /// Calls [`full_sync`] — the strongest available primitive on the
    /// active platform (`F_FULLFSYNC` on macOS, `fsync(2)` on other
    /// Unix, `FlushFileBuffers` on Windows). Returns once the OS
    /// reports completion.
    pub(crate) fn flush(&self) -> Result<()> {
        let wal = self.wal_lock();
        full_sync(&wal.file)?;
        Ok(())
    }

    /// Compact the store: write a fresh snapshot, atomically replace
    /// the old snapshot, truncate the WAL.
    ///
    /// Holds the records write lock for the duration so the snapshot
    /// reflects a coherent state. On success the WAL is empty and the
    /// snapshot carries every record. On failure (I/O error mid-way)
    /// the previous snapshot and the un-truncated WAL are still
    /// recoverable — the next open replays the WAL on top of the
    /// older snapshot.
    pub(crate) fn compact(&self) -> Result<()> {
        let records = self.write_records();
        let snap_tmp = self.root.join(SNAP_TMP_FILE);
        let snap_final = self.root.join(SNAP_FILE);
        let wal_path = self.root.join(WAL_FILE);

        write_snapshot(&snap_tmp, &records)?;

        // Atomic rename — on both Unix (`rename(2)`) and Windows
        // (`MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`),
        // `std::fs::rename` to an existing destination on the same
        // volume is a single atomic step.
        std::fs::rename(&snap_tmp, &snap_final)?;

        // Now that the snapshot is durable and visible, truncate the
        // WAL. A crash between the rename and the truncate is
        // recoverable: next open replays a fully-empty-effect WAL
        // (everything in it is already in the snapshot).
        let mut wal = self.wal_lock();
        wal.file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&wal_path)?;
        full_sync(&wal.file)?;
        wal.scratch.clear();
        Ok(())
    }

    fn wal_lock(&self) -> std::sync::MutexGuard<'_, WalHandle> {
        match self.wal.lock() {
            Ok(guard) => guard,
            Err(poison) => poison.into_inner(),
        }
    }

    fn read_records(&self) -> std::sync::RwLockReadGuard<'_, HashMap<RecordId, Record>> {
        match self.records.read() {
            Ok(guard) => guard,
            Err(poison) => poison.into_inner(),
        }
    }

    fn write_records(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<RecordId, Record>> {
        match self.records.write() {
            Ok(guard) => guard,
            Err(poison) => poison.into_inner(),
        }
    }
}

/// Ensure `path` exists and is a directory.
///
/// Creates the directory if missing; returns [`Error::InvalidConfig`]
/// if the path exists but is a file or other non-directory.
fn ensure_directory(path: &Path) -> Result<()> {
    if path.exists() {
        if !path.is_dir() {
            return Err(Error::InvalidConfig(
                "iqdb path exists but is not a directory",
            ));
        }
    } else {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

/// Load every record from a snapshot file into `records`.
///
/// The snapshot is the same framed format as the WAL — a header
/// followed by zero or more upsert frames. A snapshot never contains
/// delete frames; deletions are realised by the absence of a record
/// rather than a tombstone, so the file is always smaller than the
/// equivalent WAL replay.
fn load_snapshot(path: &Path, records: &mut HashMap<RecordId, Record>) -> Result<()> {
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() {
        // Empty snapshot is treated as an empty store. This handles
        // the case where a previous compaction created the file but
        // crashed before writing the header.
        return Ok(());
    }
    let mut offset = codec::read_header(&bytes)?;

    while let Some((op, consumed)) = codec::read_frame(&bytes[offset..])? {
        offset += consumed;
        match op {
            Op::Upsert(record) => {
                let _previous = records.insert(record.id(), record);
            }
            // Snapshots are written exclusively from a coherent
            // in-memory state, so a delete frame at this layer is a
            // corrupt file. We fail loudly rather than silently
            // dropping the entry.
            Op::Delete(_) => return Err(Error::corrupt("delete frame in snapshot")),
        }
    }
    Ok(())
}

/// Replay the WAL on top of `records`.
///
/// Returns the byte offset up to which replay succeeded — the caller
/// uses this to truncate trailing corruption. A corrupt frame stops
/// replay; everything before the corruption is preserved.
fn replay_wal(path: &Path, records: &mut HashMap<RecordId, Record>) -> Result<u64> {
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() {
        return Ok(0);
    }

    // The WAL has no header — its frames stand alone. (The snapshot
    // is the only file that carries the magic + version pair; the
    // WAL is implicitly versioned with the snapshot it accompanies,
    // because both are written by the same crate version.)
    let mut offset: usize = 0;
    loop {
        match codec::read_frame(&bytes[offset..]) {
            Ok(None) => return Ok(offset as u64),
            Ok(Some((op, consumed))) => {
                offset += consumed;
                match op {
                    Op::Upsert(record) => {
                        let _previous = records.insert(record.id(), record);
                    }
                    Op::Delete(id) => {
                        let _previous = records.remove(&id);
                    }
                }
            }
            // First corruption: stop replay. Return the offset up to
            // which we made it; the caller truncates from here.
            Err(_) => return Ok(offset as u64),
        }
    }
}

/// Truncate `path` to `len` bytes.
fn truncate_to(path: &Path, len: u64) -> Result<()> {
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_len(len)?;
    full_sync(&file)?;
    Ok(())
}

/// Write a coherent snapshot of `records` to `path`.
///
/// Iteration order over a `HashMap` is unspecified — the snapshot is
/// not byte-for-byte reproducible across machines or runs. That is
/// fine for durability (the recovered state is byte-equivalent to the
/// in-memory state) but means that any test asserting against a
/// snapshot's byte content should re-read it through `load_snapshot`
/// rather than diffing bytes.
fn write_snapshot(path: &Path, records: &HashMap<RecordId, Record>) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;

    let mut buf = Vec::with_capacity(8 + records.len() * 64);
    codec::write_header(&mut buf);
    for record in records.values() {
        codec::write_frame(&mut buf, &Op::Upsert(record.clone()));
    }
    file.write_all(&buf)?;
    full_sync(&file)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::Payload;
    use crate::vector::Vector;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tempdir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("iqdb-fs-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_dir_all(path);
    }

    fn record(id: u64, components: Vec<f32>, payload: Option<Payload>) -> Record {
        let v = Vector::new(components).unwrap();
        match payload {
            None => Record::new(RecordId::new(id), v),
            Some(p) => Record::with_payload(RecordId::new(id), v, p),
        }
    }

    #[test]
    fn open_creates_directory_if_absent() {
        let dir = tempdir();
        let child = dir.join("nested-db");
        let store = FileStore::open(&child).unwrap();
        assert!(store.is_empty());
        assert!(child.is_dir());
        cleanup(&dir);
    }

    #[test]
    fn open_rejects_existing_file_path() {
        let dir = tempdir();
        let file_path = dir.join("not-a-dir");
        std::fs::write(&file_path, b"hi").unwrap();
        let err = FileStore::open(&file_path).unwrap_err();
        assert!(matches!(err, Error::InvalidConfig(_)));
        cleanup(&dir);
    }

    #[test]
    fn upsert_persists_across_reopen() {
        let dir = tempdir();
        {
            let store = FileStore::open(&dir).unwrap();
            store.upsert(record(1, vec![0.1, 0.2, 0.3], None)).unwrap();
            store.flush().unwrap();
        }

        let store = FileStore::open(&dir).unwrap();
        let hit = store.get(RecordId::new(1)).unwrap().expect("present");
        assert_eq!(hit.vector().as_slice(), &[0.1, 0.2, 0.3]);
        cleanup(&dir);
    }

    #[test]
    fn delete_persists_across_reopen() {
        let dir = tempdir();
        {
            let store = FileStore::open(&dir).unwrap();
            store.upsert(record(1, vec![1.0, 0.0], None)).unwrap();
            store.upsert(record(2, vec![0.0, 1.0], None)).unwrap();
            assert!(store.delete(RecordId::new(1)).unwrap());
            store.flush().unwrap();
        }

        let store = FileStore::open(&dir).unwrap();
        assert!(store.get(RecordId::new(1)).unwrap().is_none());
        assert!(store.get(RecordId::new(2)).unwrap().is_some());
        cleanup(&dir);
    }

    #[test]
    fn compact_truncates_wal_and_preserves_state() {
        let dir = tempdir();
        let store = FileStore::open(&dir).unwrap();
        store.upsert(record(1, vec![1.0, 2.0], None)).unwrap();
        store.upsert(record(2, vec![3.0, 4.0], None)).unwrap();
        store.compact().unwrap();
        // WAL should be 0 bytes after compaction.
        let wal_len = std::fs::metadata(dir.join(WAL_FILE)).unwrap().len();
        assert_eq!(wal_len, 0);
        drop(store);

        let store = FileStore::open(&dir).unwrap();
        assert_eq!(store.len(), 2);
        assert!(store.get(RecordId::new(1)).unwrap().is_some());
        cleanup(&dir);
    }

    #[test]
    fn payload_round_trips_through_persistence() {
        let dir = tempdir();
        {
            let store = FileStore::open(&dir).unwrap();
            let mut p = Payload::new();
            let _ = p.insert("kind", "doc");
            let _ = p.insert("year", 2026_i64);
            store
                .upsert(record(7, vec![0.5, 0.5, 0.5], Some(p)))
                .unwrap();
            store.compact().unwrap();
        }

        let store = FileStore::open(&dir).unwrap();
        let hit = store.get(RecordId::new(7)).unwrap().expect("present");
        let payload = hit.payload().expect("payload preserved");
        assert!(payload.contains_key("kind"));
        assert!(payload.contains_key("year"));
        cleanup(&dir);
    }

    #[test]
    fn corrupt_wal_tail_truncates_to_last_good_offset() {
        let dir = tempdir();
        {
            let store = FileStore::open(&dir).unwrap();
            store.upsert(record(1, vec![1.0], None)).unwrap();
            store.flush().unwrap();
        }
        // Append random bytes to the WAL — they will form a corrupt
        // frame that fails the CRC check.
        let wal_path = dir.join(WAL_FILE);
        let mut wal = OpenOptions::new().append(true).open(&wal_path).unwrap();
        wal.write_all(&[0xFFu8; 32]).unwrap();
        wal.sync_all().unwrap();
        drop(wal);

        let store = FileStore::open(&dir).unwrap();
        assert!(store.get(RecordId::new(1)).unwrap().is_some());
        // After open, the WAL should be truncated to the last good
        // offset. Subsequent writes succeed.
        store.upsert(record(2, vec![2.0], None)).unwrap();
        store.flush().unwrap();
        cleanup(&dir);
    }

    #[test]
    fn snapshot_with_bad_magic_is_rejected() {
        let dir = tempdir();
        let _store = FileStore::open(&dir).unwrap();
        // Overwrite the snapshot file with bad magic, then close.
        let snap_path = dir.join(SNAP_FILE);
        std::fs::write(&snap_path, b"XXXX\x01\x00\x00\x00").unwrap();
        let err = FileStore::open(&dir).unwrap_err();
        assert!(matches!(err, Error::Corrupt { .. }));
        cleanup(&dir);
    }
}
