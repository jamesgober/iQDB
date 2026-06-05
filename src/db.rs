// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Public handle for the `iqdb` embedded vector database.
//!
//! The top-level [`Iqdb`] type is the only structure most callers will
//! ever construct. It owns the active backend (in-memory or
//! file-backed) and brokers every read / write through a typed API:
//! `open_in_memory` / `open` for lifecycle, `upsert` / `get` /
//! `delete` for record management, `search` / `search_with` /
//! `search_batch` / `search_batch_with` for top-`k` similarity search,
//! and `flush` / `close` for shutdown.
//!
//! As of v0.4.0, every method on the public surface is load-bearing —
//! `Iqdb::open(path)` returns a directory-backed
//! [`FileStore`](crate::file_store) and `Iqdb::flush` /
//! `Iqdb::close` drive the WAL through `full_sync` and the
//! compaction path respectively.

use std::path::Path;

use crate::backend::Backend;
use crate::error::Result;
use crate::file_store::FileStore;
use crate::record::{Record, RecordId};
use crate::search::{flat_search, SearchResult};
use crate::store::MemoryStore;
use crate::vector::{DistanceMetric, Vector};

/// Top-level handle for an open `iqdb` database.
///
/// Construct with [`Iqdb::open_in_memory`] for an ephemeral instance
/// or [`Iqdb::open`] for a directory-backed durable store. `Iqdb` is
/// `Send + Sync` and can be shared across threads via `Arc<Iqdb>`.
/// Concurrent reads share an internal `RwLock` read guard; writes
/// serialise on the same lock and — for the file-backed store —
/// additionally serialise on a WAL `Mutex`.
///
/// # Examples
///
/// In-memory instance:
///
/// ```
/// use iqdb::{Iqdb, Record, RecordId, Vector};
///
/// let db = Iqdb::open_in_memory();
/// db.upsert(Record::new(
///     RecordId::new(1),
///     Vector::new(vec![0.1, 0.2, 0.3]).unwrap(),
/// )).unwrap();
///
/// let got = db.get(RecordId::new(1)).unwrap().expect("record present");
/// assert_eq!(got.vector().as_slice(), &[0.1, 0.2, 0.3]);
/// ```
///
/// Directory-backed durable store:
///
/// ```no_run
/// use iqdb::{Iqdb, Record, RecordId, Vector};
///
/// let db = Iqdb::open("/var/lib/myapp/vectors").unwrap();
/// db.upsert(Record::new(
///     RecordId::new(1),
///     Vector::new(vec![0.1, 0.2, 0.3]).unwrap(),
/// )).unwrap();
/// db.flush().unwrap(); // sync the WAL to durable storage
/// db.close().unwrap(); // compacts the snapshot and truncates the WAL
/// ```
#[derive(Debug)]
pub struct Iqdb {
    backend: Backend,
}

impl Iqdb {
    /// Open or create a directory-backed database at `path`.
    ///
    /// The path is treated as a **directory**. If it does not exist,
    /// it is created with `create_dir_all` semantics. If it exists
    /// but is not a directory, the call fails with
    /// [`Error::InvalidConfig`](crate::Error::InvalidConfig).
    ///
    /// On open, the snapshot file (`<path>/snap`) is loaded into the
    /// in-memory map, then the WAL file (`<path>/wal`) is replayed on
    /// top. Replay stops at the first corrupt frame; records before
    /// the corruption are recovered, anything after is discarded and
    /// the WAL is truncated to the last known-good offset so future
    /// writes are contiguous with the recovered history.
    ///
    /// # Errors
    ///
    /// - [`Error::InvalidConfig`](crate::Error::InvalidConfig) — path
    ///   exists but is not a directory.
    /// - [`Error::Io`](crate::Error::Io) — directory creation,
    ///   snapshot load, or WAL open failed at the OS layer.
    /// - [`Error::Corrupt`](crate::Error::Corrupt) — the snapshot
    ///   file failed an integrity check (bad magic, unknown version,
    ///   CRC mismatch, truncated header).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use iqdb::Iqdb;
    ///
    /// let db = Iqdb::open("./data/my-db")?;
    /// assert!(db.is_empty()); // freshly created
    /// # Ok::<(), iqdb::Error>(())
    /// ```
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let store = FileStore::open(path.as_ref())?;
        Ok(Self {
            backend: Backend::File(store),
        })
    }

    /// Open an ephemeral, in-memory instance.
    ///
    /// The returned handle never touches the filesystem. All records
    /// live in a `RwLock<HashMap<…>>` and are dropped when the handle
    /// is closed (or goes out of scope without being closed).
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Iqdb;
    ///
    /// let db = Iqdb::open_in_memory();
    /// assert!(db.is_empty());
    /// ```
    #[must_use]
    pub fn open_in_memory() -> Self {
        Self {
            backend: Backend::Memory(MemoryStore::new()),
        }
    }

    /// Insert or replace a record.
    ///
    /// If `record.id()` is already present, the existing record is
    /// replaced; if not, it is inserted.
    ///
    /// On the file-backed store, the change is appended to the WAL
    /// first and only then applied to the in-memory map. If the WAL
    /// append fails, the in-memory map is not touched and the error
    /// is propagated. The WAL append is not synced to durable storage
    /// until [`Iqdb::flush`] or [`Iqdb::close`] is called — see
    /// `flush` for the durability contract.
    ///
    /// # Errors
    ///
    /// In-memory backend: infallible.
    /// File-backed backend: [`Error::Io`](crate::Error::Io) on
    /// underlying write failures.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{Iqdb, Record, RecordId, Vector};
    ///
    /// let db = Iqdb::open_in_memory();
    /// db.upsert(Record::new(
    ///     RecordId::new(1),
    ///     Vector::new(vec![1.0, 0.0]).unwrap(),
    /// )).unwrap();
    /// assert_eq!(db.len(), 1);
    ///
    /// // Replacing the same id keeps len at 1.
    /// db.upsert(Record::new(
    ///     RecordId::new(1),
    ///     Vector::new(vec![0.0, 1.0]).unwrap(),
    /// )).unwrap();
    /// assert_eq!(db.len(), 1);
    /// ```
    pub fn upsert(&self, record: Record) -> Result<()> {
        self.backend.upsert(record)
    }

    /// Look up a record by id.
    ///
    /// Returns `Ok(None)` when the id is absent. The returned record
    /// is cloned out of the backend so the internal read lock is
    /// released before the value reaches the caller — carrying a
    /// `RwLockReadGuard<'_, …>` across `?` boundaries is a deadlock
    /// vector that `iqdb` deliberately avoids.
    ///
    /// # Errors
    ///
    /// Both backends are infallible for reads at v0.4.0.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{Iqdb, Record, RecordId, Vector};
    ///
    /// let db = Iqdb::open_in_memory();
    /// db.upsert(Record::new(
    ///     RecordId::new(1),
    ///     Vector::new(vec![1.0, 2.0]).unwrap(),
    /// )).unwrap();
    ///
    /// let hit = db.get(RecordId::new(1)).unwrap();
    /// assert!(hit.is_some());
    ///
    /// let miss = db.get(RecordId::new(99)).unwrap();
    /// assert!(miss.is_none());
    /// ```
    pub fn get(&self, id: RecordId) -> Result<Option<Record>> {
        self.backend.get(id)
    }

    /// Delete a record by id.
    ///
    /// Returns `Ok(true)` if a record was removed, `Ok(false)` if the
    /// id was already absent. The boolean lets callers reason about
    /// idempotent deletes without a prior `get`.
    ///
    /// On the file-backed store, a delete frame is appended to the
    /// WAL before the in-memory map is touched.
    ///
    /// # Errors
    ///
    /// In-memory backend: infallible.
    /// File-backed backend: [`Error::Io`](crate::Error::Io) on
    /// underlying write failures.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{Iqdb, Record, RecordId, Vector};
    ///
    /// let db = Iqdb::open_in_memory();
    /// db.upsert(Record::new(
    ///     RecordId::new(1),
    ///     Vector::new(vec![1.0]).unwrap(),
    /// )).unwrap();
    /// assert!(db.delete(RecordId::new(1)).unwrap());
    /// assert!(!db.delete(RecordId::new(1)).unwrap());
    /// ```
    pub fn delete(&self, id: RecordId) -> Result<bool> {
        self.backend.delete(id)
    }

    /// Number of records currently stored.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Iqdb;
    ///
    /// let db = Iqdb::open_in_memory();
    /// assert_eq!(db.len(), 0);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.backend.len()
    }

    /// `true` if no records are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.backend.is_empty()
    }

    /// Top-`k` similarity search.
    ///
    /// Returns up to `k` records ordered by `score` ascending — the
    /// smaller, the closer under the given [`DistanceMetric`]. The
    /// `payload` field on each [`SearchResult`] is cloned at search
    /// time so callers do not need a follow-up [`Iqdb::get`].
    ///
    /// This is the no-filter convenience wrapper around
    /// [`Iqdb::search_with`]. Use the `_with` form to pre-filter
    /// candidates by payload (or any other [`Record`] predicate).
    ///
    /// The implementation is an exact, brute-force scan — every
    /// stored record is scored. Approximate indices (IVF, HNSW)
    /// land in v0.5.0 and will sit alongside the flat kernel rather
    /// than replacing it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DimensionMismatch`](crate::Error::DimensionMismatch)
    /// if `query.dim()` differs from any stored record's vector.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{DistanceMetric, Iqdb, Record, RecordId, Vector};
    ///
    /// let db = Iqdb::open_in_memory();
    /// db.upsert(Record::new(
    ///     RecordId::new(1),
    ///     Vector::new(vec![1.0, 0.0]).unwrap(),
    /// )).unwrap();
    /// db.upsert(Record::new(
    ///     RecordId::new(2),
    ///     Vector::new(vec![0.0, 1.0]).unwrap(),
    /// )).unwrap();
    ///
    /// let q = Vector::new(vec![1.0, 0.0]).unwrap();
    /// let hits = db.search(&q, 1, DistanceMetric::Cosine).unwrap();
    /// assert_eq!(hits.len(), 1);
    /// assert_eq!(hits[0].id, RecordId::new(1));
    /// ```
    pub fn search(
        &self,
        query: &Vector,
        k: usize,
        metric: DistanceMetric,
    ) -> Result<Vec<SearchResult>> {
        flat_search(&self.backend, query, k, metric, |_| true)
    }

    /// Top-`k` similarity search with a payload (or any-record) filter.
    ///
    /// The `filter` predicate is monomorphised into the search loop —
    /// there is no per-record dynamic dispatch. Records for which the
    /// predicate returns `false` are excluded from the candidate set
    /// before the top-`k` heap admit decision.
    ///
    /// The filter runs while the backend's read lock is held. **Do
    /// not call back into the same [`Iqdb`] handle from inside the
    /// filter** — doing so risks a re-entrant lock acquisition.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DimensionMismatch`](crate::Error::DimensionMismatch)
    /// under the same conditions as [`Iqdb::search`].
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{DistanceMetric, Iqdb, Payload, PayloadValue, Record, RecordId, Vector};
    ///
    /// let db = Iqdb::open_in_memory();
    ///
    /// let mut doc = Payload::new();
    /// doc.insert("kind", "doc");
    /// db.upsert(Record::with_payload(
    ///     RecordId::new(1),
    ///     Vector::new(vec![1.0, 0.0]).unwrap(),
    ///     doc,
    /// )).unwrap();
    ///
    /// let mut other = Payload::new();
    /// other.insert("kind", "image");
    /// db.upsert(Record::with_payload(
    ///     RecordId::new(2),
    ///     Vector::new(vec![1.0, 0.01]).unwrap(),
    ///     other,
    /// )).unwrap();
    ///
    /// let q = Vector::new(vec![1.0, 0.0]).unwrap();
    /// let hits = db.search_with(&q, 5, DistanceMetric::Cosine, |rec| {
    ///     rec.payload()
    ///         .and_then(|p| p.get("kind"))
    ///         .and_then(PayloadValue::as_text)
    ///         == Some("doc")
    /// }).unwrap();
    ///
    /// assert_eq!(hits.len(), 1);
    /// assert_eq!(hits[0].id, RecordId::new(1));
    /// ```
    pub fn search_with<F>(
        &self,
        query: &Vector,
        k: usize,
        metric: DistanceMetric,
        filter: F,
    ) -> Result<Vec<SearchResult>>
    where
        F: Fn(&Record) -> bool,
    {
        flat_search(&self.backend, query, k, metric, filter)
    }

    /// Sequential batch search — one top-`k` result list per query.
    ///
    /// Equivalent to calling [`Iqdb::search`] once per query, but
    /// folded into a single API for ergonomic batch ingest pipelines.
    /// The result vector preserves input order: `output[i]` is the
    /// top-`k` for `queries[i]`. Each batch element acquires the
    /// backend's read lock independently.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DimensionMismatch`](crate::Error::DimensionMismatch)
    /// on the first query whose dimensionality does not match the
    /// stored schema. Subsequent queries are not attempted.
    pub fn search_batch(
        &self,
        queries: &[Vector],
        k: usize,
        metric: DistanceMetric,
    ) -> Result<Vec<Vec<SearchResult>>> {
        self.search_batch_with(queries, k, metric, |_| true)
    }

    /// Batch search with a shared filter applied to every query.
    ///
    /// See [`Iqdb::search_with`] for the filter semantics. The
    /// predicate is reused across every query in the batch.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DimensionMismatch`](crate::Error::DimensionMismatch)
    /// on the first query whose dimensionality does not match the
    /// stored schema. Subsequent queries are not attempted.
    pub fn search_batch_with<F>(
        &self,
        queries: &[Vector],
        k: usize,
        metric: DistanceMetric,
        filter: F,
    ) -> Result<Vec<Vec<SearchResult>>>
    where
        F: Fn(&Record) -> bool,
    {
        let mut out = Vec::with_capacity(queries.len());
        for query in queries {
            out.push(flat_search(&self.backend, query, k, metric, &filter)?);
        }
        Ok(out)
    }

    /// Flush all pending writes to durable storage.
    ///
    /// **File-backed store:** runs the strongest available sync
    /// primitive against the WAL — `F_FULLFSYNC` on macOS, `fsync(2)`
    /// on other Unix, `FlushFileBuffers` on Windows. Returns once the
    /// OS reports completion.
    ///
    /// **In-memory store:** returns `Ok(())` immediately — the
    /// in-memory map is already as durable as a memory-only backend
    /// can be, so flush is a no-op.
    ///
    /// The default durability contract is: a successful `upsert`
    /// followed by a successful `flush` is durable across a power
    /// cut. An `upsert` whose `flush` has not yet returned may be
    /// lost on a crash. Per-write sync (every `upsert` is durable
    /// before returning) is reserved for a later milestone.
    ///
    /// # Errors
    ///
    /// File-backed store: [`Error::Io`](crate::Error::Io) on
    /// underlying sync failures.
    pub fn flush(&self) -> Result<()> {
        self.backend.flush()
    }

    /// Close the database handle, releasing any held resources.
    ///
    /// Consumes `self`. **File-backed store:** runs a compaction —
    /// writes a fresh snapshot, atomically replaces the old one,
    /// truncates the WAL. After `close` returns, the on-disk state is
    /// the snapshot alone and the next open is a single-file load
    /// with no WAL replay. **In-memory store:** drops the in-memory
    /// map.
    ///
    /// `close` is the cleanest shutdown path; dropping a handle
    /// without calling `close` works (the file backend's WAL still
    /// represents a recoverable state) but leaves the next open with
    /// a replay pass instead of a snapshot-only load.
    ///
    /// # Errors
    ///
    /// File-backed store: [`Error::Io`](crate::Error::Io) on
    /// underlying snapshot / rename / truncate failures during
    /// compaction.
    pub fn close(self) -> Result<()> {
        self.backend.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::Payload;
    use crate::vector::Vector;

    fn vec3(a: f32, b: f32, c: f32) -> Vector {
        Vector::new(vec![a, b, c]).unwrap()
    }

    #[test]
    fn open_in_memory_returns_empty_handle() {
        let db = Iqdb::open_in_memory();
        assert!(db.is_empty());
        assert_eq!(db.len(), 0);
    }

    #[test]
    fn flush_on_in_memory_is_noop_ok() {
        let db = Iqdb::open_in_memory();
        assert!(db.flush().is_ok());
    }

    #[test]
    fn close_on_in_memory_succeeds() {
        let db = Iqdb::open_in_memory();
        assert!(db.close().is_ok());
    }

    #[test]
    fn upsert_get_round_trip() {
        let db = Iqdb::open_in_memory();
        db.upsert(Record::new(RecordId::new(1), vec3(0.1, 0.2, 0.3)))
            .unwrap();
        let hit = db.get(RecordId::new(1)).unwrap().expect("present");
        assert_eq!(hit.id().get(), 1);
        assert_eq!(hit.vector().as_slice(), &[0.1, 0.2, 0.3]);
    }

    #[test]
    fn upsert_replaces_existing_record() {
        let db = Iqdb::open_in_memory();
        db.upsert(Record::new(RecordId::new(1), vec3(1.0, 0.0, 0.0)))
            .unwrap();
        db.upsert(Record::new(RecordId::new(1), vec3(0.0, 1.0, 0.0)))
            .unwrap();
        assert_eq!(db.len(), 1);
        let hit = db.get(RecordId::new(1)).unwrap().expect("present");
        assert_eq!(hit.vector().as_slice(), &[0.0, 1.0, 0.0]);
    }

    #[test]
    fn get_returns_none_for_missing_id() {
        let db = Iqdb::open_in_memory();
        assert!(db.get(RecordId::new(99)).unwrap().is_none());
    }

    #[test]
    fn delete_returns_true_only_when_removed() {
        let db = Iqdb::open_in_memory();
        db.upsert(Record::new(RecordId::new(1), vec3(1.0, 0.0, 0.0)))
            .unwrap();
        assert!(db.delete(RecordId::new(1)).unwrap());
        assert!(!db.delete(RecordId::new(1)).unwrap());
    }

    #[test]
    fn payload_round_trips_through_upsert_and_get() {
        let db = Iqdb::open_in_memory();
        let mut payload = Payload::new();
        payload.insert("kind", "doc");
        payload.insert("score", 0.97_f64);

        let record = Record::with_payload(RecordId::new(7), vec3(1.0, 2.0, 3.0), payload);
        db.upsert(record).unwrap();

        let hit = db.get(RecordId::new(7)).unwrap().expect("present");
        let payload = hit.payload().expect("payload present");
        assert_eq!(
            payload
                .get("kind")
                .and_then(crate::payload::PayloadValue::as_text),
            Some("doc")
        );
        assert!(payload
            .get("score")
            .and_then(crate::payload::PayloadValue::as_float)
            .map(|f| (f - 0.97).abs() < 1e-9)
            .unwrap_or(false));
    }

    #[test]
    fn handle_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Iqdb>();
    }
}
