// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Public handle for the `iqdb` embedded vector database.
//!
//! The top-level [`Iqdb`] type is the only structure most callers will
//! ever construct. It owns the backing store and brokers every read /
//! write through a typed API: `open_in_memory` / `open` for lifecycle,
//! `upsert` / `get` / `delete` for record management, `search` /
//! `search_with` / `search_batch` / `search_batch_with` for top-`k`
//! similarity search, and `flush` / `close` for shutdown.
//!
//! `open(path)` and `flush()` still return [`Error::NotImplemented`]
//! at v0.3.0 — they light up with the durable storage substrate in
//! v0.4.0. Wiring them now means call sites can be authored against
//! the final shape and gated on the variant; no refactor is required
//! when the engine arrives.

use std::path::Path;

use crate::error::{Error, Result};
use crate::record::{Record, RecordId};
use crate::search::{flat_search, SearchResult};
use crate::store::MemoryStore;
use crate::vector::{DistanceMetric, Vector};

/// Top-level handle for an open `iqdb` database.
///
/// Construct with [`Iqdb::open_in_memory`] for an ephemeral instance,
/// or — once durable storage lands in v0.4.0 — [`Iqdb::open`] for a
/// file-backed store. `Iqdb` is `Send + Sync` and can be shared
/// across threads via `Arc<Iqdb>`; the in-memory backend serialises
/// writers behind an internal `RwLock` and allows concurrent readers.
///
/// # Examples
///
/// Construct an in-memory instance, upsert a few records, and read
/// one back:
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
#[derive(Debug)]
pub struct Iqdb {
    store: MemoryStore,
}

impl Iqdb {
    /// Open the database at the given path.
    ///
    /// The file-backed path lands with the durable-storage substrate
    /// in v0.4.0. Until then, this method always returns
    /// [`Error::NotImplemented`] so call sites can be wired against
    /// the final API shape ahead of the engine arriving.
    ///
    /// # Errors
    ///
    /// Currently always returns [`Error::NotImplemented`].
    pub fn open<P: AsRef<Path>>(_path: P) -> Result<Self> {
        Err(Error::NotImplemented)
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
            store: MemoryStore::new(),
        }
    }

    /// Insert or replace a record.
    ///
    /// If `record.id()` is already present in the store, the existing
    /// record is replaced; if not, it is inserted. Both paths return
    /// `Ok(())` — the durable backend in v0.4.0 will use this signature
    /// to report partial-write failures, so call sites should already
    /// be `?`-propagating the result.
    ///
    /// # Errors
    ///
    /// In v0.2.0 the in-memory path is infallible. The signature
    /// returns `Result<()>` so the durable backend can introduce
    /// failure modes without an API break.
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
        self.store.upsert(record)
    }

    /// Look up a record by id.
    ///
    /// Returns `Ok(None)` when the id is absent. The returned record
    /// is cloned out of the store so the internal read lock is
    /// released before the value reaches the caller — borrowing the
    /// guard across `?` boundaries is a deadlock vector that
    /// `iqdb` deliberately avoids.
    ///
    /// # Errors
    ///
    /// In v0.2.0 the in-memory path is infallible.
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
        self.store.get(id)
    }

    /// Delete a record by id.
    ///
    /// Returns `Ok(true)` if a record was removed, `Ok(false)` if the
    /// id was already absent. The boolean lets callers reason about
    /// idempotent deletes without a prior `get`.
    ///
    /// # Errors
    ///
    /// In v0.2.0 the in-memory path is infallible.
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
        self.store.delete(id)
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
        self.store.len()
    }

    /// `true` if no records are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    /// Top-`k` similarity search against the in-memory store.
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
    /// Returns [`Error::DimensionMismatch`] if `query.dim()` differs
    /// from any stored record's vector. The dimensional schema is
    /// expected to be homogeneous within a single store; v0.7.0
    /// collections will enforce this at upsert time.
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
        flat_search(&self.store, query, k, metric, |_| true)
    }

    /// Top-`k` similarity search with a payload (or any-record) filter.
    ///
    /// The `filter` predicate is monomorphised into the search loop —
    /// there is no per-record dynamic dispatch, so the cost of a
    /// filtered scan is the cost of the filter call plus the
    /// unfiltered scan. Records for which the predicate returns
    /// `false` are excluded from the candidate set before the
    /// top-`k` heap admit decision.
    ///
    /// The filter runs while the store's read lock is held. **Do not
    /// call back into the same [`Iqdb`] handle from inside the
    /// filter** — doing so risks a re-entrant lock acquisition.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DimensionMismatch`] under the same conditions
    /// as [`Iqdb::search`].
    ///
    /// # Examples
    ///
    /// Filter by a payload field:
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
        flat_search(&self.store, query, k, metric, filter)
    }

    /// Sequential batch search — one top-`k` result list per query.
    ///
    /// Equivalent to calling [`Iqdb::search`] once per query, but
    /// folded into a single API for ergonomic batch ingest pipelines.
    /// The result vector preserves input order: `output[i]` is the
    /// top-`k` for `queries[i]`. Each batch acquires the store's
    /// read lock independently — concurrent writers can interleave
    /// between batch elements.
    ///
    /// Parallel batch execution is reserved for a later milestone
    /// (the search-engine work in v0.5.0 will introduce the
    /// per-shard scan that batch parallelism rides on).
    ///
    /// # Errors
    ///
    /// Returns [`Error::DimensionMismatch`] on the first query whose
    /// dimensionality does not match the stored schema. Subsequent
    /// queries are not attempted.
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
    /// predicate is reused — and re-monomorphised — across every
    /// query in the batch.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DimensionMismatch`] on the first query whose
    /// dimensionality does not match the stored schema. Subsequent
    /// queries are not attempted.
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
            out.push(flat_search(&self.store, query, k, metric, &filter)?);
        }
        Ok(out)
    }

    /// Flush all pending writes to durable storage.
    ///
    /// The in-memory backend has no durable substrate to flush to;
    /// the method is retained so call sites can be wired against the
    /// final API shape, and it lights up in v0.4.0 when file-backed
    /// storage lands. Today it returns [`Error::NotImplemented`].
    ///
    /// # Errors
    ///
    /// Currently always returns [`Error::NotImplemented`].
    pub fn flush(&self) -> Result<()> {
        Err(Error::NotImplemented)
    }

    /// Close the database handle, releasing any held resources.
    ///
    /// Consumes `self`. The in-memory backend has no resources beyond
    /// the boxed slices that drop with the handle; the explicit
    /// `close` exists so call sites have a single point where
    /// durable-backend cleanup (sync / lock release / file close)
    /// will land in v0.4.0.
    ///
    /// # Errors
    ///
    /// Currently always returns `Ok(())`. Widen as needed when close
    /// has real work to do.
    pub fn close(self) -> Result<()> {
        Ok(())
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
    fn open_returns_not_implemented() {
        let result = Iqdb::open("/tmp/iqdb-test");
        assert!(matches!(result, Err(Error::NotImplemented)));
    }

    #[test]
    fn flush_returns_not_implemented() {
        let db = Iqdb::open_in_memory();
        let result = db.flush();
        assert!(matches!(result, Err(Error::NotImplemented)));
    }

    #[test]
    fn close_succeeds() {
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
