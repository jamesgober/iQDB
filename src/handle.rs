// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! The public database handle.
//!
//! [`Iqdb`] is the one type most callers construct. It fixes a
//! dimensionality and a [`DistanceMetric`] at open time — an HNSW graph or
//! an IVF clustering is built around both, so neither changes without a
//! rebuild — and brokers every read and write through a single internal
//! [`RwLock`]. Concurrent searches share a read guard; writes take the write
//! guard; the rare IVF materialization path (building the cluster index from
//! the stored vectors on the first search after a write) takes the write
//! guard too. Lock poisoning is recovered, never propagated as a panic.
//!
//! Two backends sit behind the same surface: an in-memory store and a
//! directory-file-backed store made durable by [`iqdb_persist`]. The public
//! API and its results are identical across both.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use iqdb_cache::CacheStats;
use iqdb_index::{Index, IndexCore};
use iqdb_persist::{PersistConfig, PersistedIndex};
use iqdb_types::{
    DistanceMetric, Filter, Hit, IqdbError, Metadata, SearchParams, Vector, VectorId,
};

use crate::config::IqdbConfig;
use crate::engine::IqdbCore;
use crate::error::{Error, Result};

/// The active backend behind an [`Iqdb`] handle.
enum Storage {
    /// An ephemeral, in-memory database.
    Memory(IqdbCore),
    /// A durable, file-backed database (snapshot + write-ahead log).
    Persisted(PersistedIndex<IqdbCore>),
}

impl Storage {
    fn core(&self) -> &IqdbCore {
        match self {
            Self::Memory(core) => core,
            Self::Persisted(persisted) => persisted.index(),
        }
    }

    fn core_mut(&mut self) -> &mut IqdbCore {
        match self {
            Self::Memory(core) => core,
            Self::Persisted(persisted) => persisted.index_mut(),
        }
    }

    fn upsert(&mut self, id: VectorId, vector: Arc<[f32]>, meta: Option<Metadata>) -> Result<()> {
        match self {
            // In-memory: apply directly. File-backed: log to the WAL first
            // (fsync per policy), then apply — so an acknowledged write
            // survives a crash and replays on the next open.
            Self::Memory(core) => core.insert(id, vector, meta)?,
            Self::Persisted(persisted) => persisted.insert(id, vector, meta)?,
        }
        Ok(())
    }

    fn delete(&mut self, id: &VectorId) -> Result<()> {
        match self {
            Self::Memory(core) => core.delete(id)?,
            Self::Persisted(persisted) => persisted.delete(id)?,
        }
        Ok(())
    }

    /// Compact a file-backed store: fold the WAL into a fresh snapshot and
    /// reset the log. A no-op for the in-memory backend.
    fn checkpoint(&mut self) -> Result<()> {
        if let Self::Persisted(persisted) = self {
            persisted.checkpoint()?;
        }
        Ok(())
    }
}

/// An open `iqdb` database.
///
/// Construct with [`Iqdb::open_in_memory`] for an ephemeral instance or
/// [`Iqdb::open`] for a durable, file-backed one. `Iqdb` is `Send + Sync`
/// and is meant to be shared across threads behind an `Arc<Iqdb>`.
///
/// # Examples
///
/// ```
/// use iqdb::{DistanceMetric, Iqdb, Vector, VectorId};
///
/// # fn run() -> iqdb::Result<()> {
/// let db = Iqdb::open_in_memory(3, DistanceMetric::Cosine)?;
/// db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0, 0.0])?, None)?;
/// db.upsert(VectorId::from(2u64), Vector::new(vec![0.0, 1.0, 0.0])?, None)?;
///
/// let hits = db.search(&Vector::new(vec![1.0, 0.0, 0.0])?, 1)?;
/// assert_eq!(hits[0].id, VectorId::from(1u64));
/// # Ok(())
/// # }
/// # run().unwrap();
/// ```
pub struct Iqdb {
    dim: usize,
    metric: DistanceMetric,
    inner: RwLock<Storage>,
}

impl Iqdb {
    /// Open an ephemeral, in-memory database of `dim`-dimensional vectors
    /// compared under `metric`, backed by the exact flat index.
    ///
    /// # Errors
    ///
    /// [`Error::Config`] if `dim` is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{DistanceMetric, Iqdb};
    ///
    /// let db = Iqdb::open_in_memory(8, DistanceMetric::Euclidean).unwrap();
    /// assert!(db.is_empty());
    /// ```
    pub fn open_in_memory(dim: usize, metric: DistanceMetric) -> Result<Self> {
        Self::open_in_memory_with(IqdbConfig::new(dim, metric))
    }

    /// Open an in-memory database from a full [`IqdbConfig`] — choosing the
    /// index implementation ([`IndexKind`](crate::IndexKind)) and an optional
    /// result cache.
    ///
    /// # Errors
    ///
    /// - [`Error::Config`] if `dim` is zero.
    /// - [`Error::Index`] if the chosen index rejects the configuration (for
    ///   example an IVF-PQ setup under an unsupported metric).
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{DistanceMetric, HnswConfig, IndexKind, IqdbConfig, Iqdb};
    ///
    /// let cfg = IqdbConfig::new(16, DistanceMetric::Cosine)
    ///     .index(IndexKind::Hnsw(HnswConfig::default()));
    /// let db = Iqdb::open_in_memory_with(cfg).unwrap();
    /// assert!(db.is_empty());
    /// ```
    pub fn open_in_memory_with(config: IqdbConfig) -> Result<Self> {
        let (dim, metric, core_cfg) = config.into_parts();
        Self::require_nonzero_dim(dim)?;
        let core = IqdbCore::new(dim, metric, core_cfg)?;
        Ok(Self {
            dim,
            metric,
            inner: RwLock::new(Storage::Memory(core)),
        })
    }

    /// Open or create a durable, file-backed database at `path`.
    ///
    /// `path` is the snapshot file; the write-ahead log lives beside it. If
    /// the file exists it is loaded (and the WAL replayed); otherwise a fresh
    /// database is created. A reopened database's stored `dim` / `metric`
    /// must match the values passed here.
    ///
    /// # Errors
    ///
    /// - [`Error::Persist`] on I/O failure, a corrupt snapshot, or a checksum
    ///   mismatch.
    /// - [`Error::Config`] if `dim` is zero, or if a reopened database's
    ///   `dim` / `metric` does not match the requested values.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use iqdb::{DistanceMetric, Iqdb, Vector, VectorId};
    ///
    /// # fn run() -> iqdb::Result<()> {
    /// let db = Iqdb::open("./data/vectors.iqdb", 3, DistanceMetric::Cosine)?;
    /// db.upsert(VectorId::from(1u64), Vector::new(vec![0.1, 0.2, 0.3])?, None)?;
    /// db.flush()?; // compact: fold the WAL into a fresh snapshot
    /// db.close()
    /// # }
    /// # run().unwrap();
    /// ```
    pub fn open<P: AsRef<Path>>(path: P, dim: usize, metric: DistanceMetric) -> Result<Self> {
        Self::open_with(path, IqdbConfig::new(dim, metric))
    }

    /// Open or create a durable, file-backed database from a full
    /// [`IqdbConfig`].
    ///
    /// # Errors
    ///
    /// See [`Iqdb::open`].
    pub fn open_with<P: AsRef<Path>>(path: P, config: IqdbConfig) -> Result<Self> {
        let (dim, metric, core_cfg) = config.into_parts();
        Self::require_nonzero_dim(dim)?;
        let cache = core_cfg.cache.clone();
        let path = path.as_ref().to_path_buf();
        let mut persist_cfg = PersistConfig::new(path.clone());
        persist_cfg.wal_enabled = true;

        let storage = if path.exists() {
            let mut persisted = PersistedIndex::<IqdbCore>::load(persist_cfg)?;
            {
                let core = persisted.index();
                if core.dim() != dim || core.metric() != metric {
                    return Err(Error::Config(
                        "reopened database dim/metric does not match the requested values",
                    ));
                }
            }
            // The cache is a runtime choice, not persisted; re-apply it here.
            if cache.is_some() {
                persisted.index_mut().set_cache(cache)?;
            }
            Storage::Persisted(persisted)
        } else {
            let core = IqdbCore::new(dim, metric, core_cfg)?;
            Storage::Persisted(PersistedIndex::open_with(core, persist_cfg)?)
        };

        Ok(Self {
            dim,
            metric,
            inner: RwLock::new(storage),
        })
    }

    /// The fixed dimensionality of every vector in this database.
    #[must_use]
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// The fixed distance metric.
    #[must_use]
    pub fn metric(&self) -> DistanceMetric {
        self.metric
    }

    /// Number of stored vectors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.read().core().len()
    }

    /// `true` if no vectors are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.read().core().is_empty()
    }

    /// Insert or replace the vector (and optional metadata) stored under
    /// `id`. Replacing an existing id keeps the count unchanged.
    ///
    /// # Errors
    ///
    /// [`Error::Index`] with [`IqdbError::DimensionMismatch`] if
    /// `vector.dim()` differs from the database dimensionality;
    /// [`Error::Persist`] on a durable-store write failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{DistanceMetric, Iqdb, Metadata, Value, Vector, VectorId};
    ///
    /// # fn run() -> iqdb::Result<()> {
    /// let db = Iqdb::open_in_memory(2, DistanceMetric::Cosine)?;
    /// let meta: Metadata = [("kind".to_string(), Value::String("doc".into()))]
    ///     .into_iter()
    ///     .collect();
    /// db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0])?, Some(meta))?;
    /// assert_eq!(db.len(), 1);
    /// # Ok(())
    /// # }
    /// # run().unwrap();
    /// ```
    pub fn upsert(&self, id: VectorId, vector: Vector, metadata: Option<Metadata>) -> Result<()> {
        self.check_dim(vector.dim())?;
        let arc: Arc<[f32]> = Arc::from(vector.into_inner().into_boxed_slice());
        self.write().upsert(id, arc, metadata)
    }

    /// Look up the vector and metadata stored under `id`.
    ///
    /// # Errors
    ///
    /// Reads do not fail under normal operation; an error indicates a stored
    /// vector that no longer satisfies validation, surfaced as
    /// [`Error::Index`].
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{DistanceMetric, Iqdb, Vector, VectorId};
    ///
    /// # fn run() -> iqdb::Result<()> {
    /// let db = Iqdb::open_in_memory(2, DistanceMetric::Cosine)?;
    /// db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 2.0])?, None)?;
    ///
    /// let (vector, _meta) = db.get(&VectorId::from(1u64))?.expect("present");
    /// assert_eq!(vector.as_slice(), &[1.0, 2.0]);
    /// assert!(db.get(&VectorId::from(99u64))?.is_none());
    /// # Ok(())
    /// # }
    /// # run().unwrap();
    /// ```
    pub fn get(&self, id: &VectorId) -> Result<Option<(Vector, Option<Metadata>)>> {
        match self.read().core().get_row(id) {
            None => Ok(None),
            Some((vector, meta)) => {
                let vector = Vector::new(vector.as_ref().to_vec())?;
                Ok(Some((vector, meta)))
            }
        }
    }

    /// Delete the vector stored under `id`. Returns `true` if a vector was
    /// removed, `false` if `id` was already absent.
    ///
    /// # Errors
    ///
    /// [`Error::Persist`] on a durable-store write failure.
    pub fn delete(&self, id: &VectorId) -> Result<bool> {
        let mut guard = self.write();
        let existed = guard.core().contains(id);
        guard.delete(id)?;
        Ok(existed)
    }

    /// Top-`k` similarity search under the database metric. Returns up to `k`
    /// [`Hit`]s ordered nearest-first (smaller distance is nearer).
    ///
    /// # Errors
    ///
    /// [`Error::Index`] with [`IqdbError::DimensionMismatch`] if
    /// `query.dim()` differs from the database dimensionality.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{DistanceMetric, Iqdb, Vector, VectorId};
    ///
    /// # fn run() -> iqdb::Result<()> {
    /// let db = Iqdb::open_in_memory(2, DistanceMetric::Euclidean)?;
    /// db.upsert(VectorId::from(1u64), Vector::new(vec![0.0, 0.0])?, None)?;
    /// db.upsert(VectorId::from(2u64), Vector::new(vec![5.0, 5.0])?, None)?;
    ///
    /// let hits = db.search(&Vector::new(vec![0.1, 0.1])?, 1)?;
    /// assert_eq!(hits[0].id, VectorId::from(1u64));
    /// # Ok(())
    /// # }
    /// # run().unwrap();
    /// ```
    pub fn search(&self, query: &Vector, k: usize) -> Result<Vec<Hit>> {
        self.search_inner(query, k, None)
    }

    /// Top-`k` similarity search restricted to vectors whose metadata matches
    /// `filter`.
    ///
    /// On the exact flat index the filter is applied before scoring, so the
    /// result is exact. On the approximate HNSW and IVF indices the filter is
    /// applied after traversal, so a highly selective filter can return fewer
    /// than `k` hits; widen the search (HNSW `filter_widen`, IVF `n_probes`)
    /// if that matters.
    ///
    /// # Errors
    ///
    /// As [`Iqdb::search`], plus [`Error::Index`] with
    /// [`IqdbError::InvalidFilter`] for a malformed filter.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{DistanceMetric, Filter, Iqdb, Metadata, Value, Vector, VectorId};
    ///
    /// # fn run() -> iqdb::Result<()> {
    /// let db = Iqdb::open_in_memory(2, DistanceMetric::Cosine)?;
    /// let doc: Metadata = [("kind".to_string(), Value::String("doc".into()))]
    ///     .into_iter().collect();
    /// let img: Metadata = [("kind".to_string(), Value::String("image".into()))]
    ///     .into_iter().collect();
    /// db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0])?, Some(doc))?;
    /// db.upsert(VectorId::from(2u64), Vector::new(vec![1.0, 0.01])?, Some(img))?;
    ///
    /// let filter = Filter::eq("kind", Value::String("doc".into()));
    /// let hits = db.search_with(&Vector::new(vec![1.0, 0.0])?, 5, filter)?;
    /// assert_eq!(hits.len(), 1);
    /// assert_eq!(hits[0].id, VectorId::from(1u64));
    /// # Ok(())
    /// # }
    /// # run().unwrap();
    /// ```
    pub fn search_with(&self, query: &Vector, k: usize, filter: Filter) -> Result<Vec<Hit>> {
        self.search_inner(query, k, Some(filter))
    }

    /// Run [`Iqdb::search`] for each query, preserving input order.
    ///
    /// # Errors
    ///
    /// As [`Iqdb::search`]; fails on the first query whose dimensionality is
    /// wrong.
    pub fn search_batch(&self, queries: &[Vector], k: usize) -> Result<Vec<Vec<Hit>>> {
        queries.iter().map(|q| self.search(q, k)).collect()
    }

    /// Run [`Iqdb::search_with`] for each query with a shared `filter`,
    /// preserving input order.
    ///
    /// # Errors
    ///
    /// As [`Iqdb::search_with`].
    pub fn search_batch_with(
        &self,
        queries: &[Vector],
        k: usize,
        filter: Filter,
    ) -> Result<Vec<Vec<Hit>>> {
        queries
            .iter()
            .map(|q| self.search_with(q, k, filter.clone()))
            .collect()
    }

    /// Rebuild the approximate index over the current vectors. For IVF this
    /// retrains centroids (and PQ codebooks) so recall tracks the data after
    /// many inserts or deletes; for the flat and HNSW kinds it is a no-op.
    ///
    /// # Errors
    ///
    /// [`Error::Index`] if retraining fails.
    pub fn optimize(&self) -> Result<()> {
        self.write().core_mut().optimize().map_err(Error::from)
    }

    /// Cache hit/miss statistics, or `None` if no cache is configured.
    #[must_use]
    pub fn cache_stats(&self) -> Option<CacheStats> {
        self.read().core().cache_stats()
    }

    /// Flush pending writes to durable storage. For a file-backed database
    /// this compacts the write-ahead log into a fresh snapshot; for an
    /// in-memory database it is a no-op.
    ///
    /// # Errors
    ///
    /// [`Error::Persist`] on a durable-store write failure.
    pub fn flush(&self) -> Result<()> {
        self.write().checkpoint()
    }

    /// Close the database, compacting a file-backed store one final time.
    /// Consumes the handle.
    ///
    /// # Errors
    ///
    /// [`Error::Persist`] on a final-compaction write failure.
    pub fn close(self) -> Result<()> {
        let mut storage = self
            .inner
            .into_inner()
            .unwrap_or_else(|poison| poison.into_inner());
        storage.checkpoint()
    }

    // -- internals ------------------------------------------------------

    /// The single construction gate for `dim`. Every constructor funnels
    /// through `open_in_memory_with` / `open_with`, so rejecting a zero
    /// dimension here covers both the direct (`dim` argument) and the
    /// `IqdbConfig` builder paths — a zero-dim config cannot be smuggled in.
    fn require_nonzero_dim(dim: usize) -> Result<()> {
        if dim == 0 {
            Err(Error::Config("dim must be non-zero"))
        } else {
            Ok(())
        }
    }

    fn check_dim(&self, found: usize) -> Result<()> {
        if found == self.dim {
            Ok(())
        } else {
            Err(Error::Index(IqdbError::DimensionMismatch {
                expected: self.dim,
                found,
            }))
        }
    }

    fn params(&self, k: usize, filter: Option<Filter>) -> SearchParams {
        SearchParams {
            filter,
            ..SearchParams::new(k, self.metric)
        }
    }

    fn search_inner(&self, query: &Vector, k: usize, filter: Option<Filter>) -> Result<Vec<Hit>> {
        self.check_dim(query.dim())?;

        // Fast path: a read guard is enough unless the IVF index still needs
        // to be materialized from the store.
        {
            let guard = self.read();
            if guard.core().is_empty() {
                return Ok(Vec::new());
            }
            if !guard.core().needs_materialization() {
                let params = self.params(k, filter);
                return guard
                    .core()
                    .search(query.as_slice(), &params)
                    .map_err(Error::from);
            }
        }

        // Slow path: build the IVF index under the write guard, then search.
        let mut guard = self.write();
        if guard.core().is_empty() {
            return Ok(Vec::new());
        }
        guard.core_mut().ensure_ready()?;
        let params = self.params(k, filter);
        guard
            .core()
            .search(query.as_slice(), &params)
            .map_err(Error::from)
    }

    fn read(&self) -> RwLockReadGuard<'_, Storage> {
        self.inner
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, Storage> {
        self.inner
            .write()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

impl fmt::Debug for Iqdb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Iqdb")
            .field("dim", &self.dim)
            .field("metric", &self.metric)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HnswConfig, IndexKind, IvfConfig};

    fn vec2(a: f32, b: f32) -> Vector {
        Vector::new(vec![a, b]).unwrap()
    }

    #[test]
    fn handle_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Iqdb>();
    }

    #[test]
    fn open_in_memory_rejects_zero_dim() {
        // Both the direct path and the IqdbConfig builder path hit the same
        // gate and surface the same variant.
        let direct = Iqdb::open_in_memory(0, DistanceMetric::Cosine).unwrap_err();
        assert!(matches!(direct, Error::Config(_)), "got {direct:?}");

        let via_config =
            Iqdb::open_in_memory_with(IqdbConfig::new(0, DistanceMetric::Cosine)).unwrap_err();
        assert!(matches!(via_config, Error::Config(_)), "got {via_config:?}");
    }

    #[test]
    fn crud_round_trip_in_memory() {
        let db = Iqdb::open_in_memory(2, DistanceMetric::Euclidean).unwrap();
        assert!(db.is_empty());
        db.upsert(VectorId::from(1u64), vec2(0.0, 0.0), None)
            .unwrap();
        db.upsert(VectorId::from(2u64), vec2(3.0, 4.0), None)
            .unwrap();
        assert_eq!(db.len(), 2);

        let (got, _) = db.get(&VectorId::from(2u64)).unwrap().unwrap();
        assert_eq!(got.as_slice(), &[3.0, 4.0]);

        assert!(db.delete(&VectorId::from(2u64)).unwrap());
        assert!(!db.delete(&VectorId::from(2u64)).unwrap());
        assert_eq!(db.len(), 1);
    }

    #[test]
    fn upsert_rejects_wrong_dimension() {
        let db = Iqdb::open_in_memory(3, DistanceMetric::Cosine).unwrap();
        let err = db
            .upsert(VectorId::from(1u64), vec2(1.0, 0.0), None)
            .unwrap_err();
        assert!(matches!(
            err,
            Error::Index(IqdbError::DimensionMismatch {
                expected: 3,
                found: 2
            })
        ));
    }

    #[test]
    fn search_on_empty_returns_empty() {
        let db = Iqdb::open_in_memory(2, DistanceMetric::Cosine).unwrap();
        assert!(db.search(&vec2(1.0, 0.0), 5).unwrap().is_empty());
    }

    #[test]
    fn search_orders_nearest_first() {
        let db = Iqdb::open_in_memory(2, DistanceMetric::Euclidean).unwrap();
        db.upsert(VectorId::from(1u64), vec2(0.0, 0.0), None)
            .unwrap();
        db.upsert(VectorId::from(2u64), vec2(10.0, 10.0), None)
            .unwrap();
        let hits = db.search(&vec2(0.1, 0.1), 2).unwrap();
        assert_eq!(hits[0].id, VectorId::from(1u64));
        assert_eq!(hits[1].id, VectorId::from(2u64));
    }

    #[test]
    fn ivf_handle_searches_after_lazy_materialization() {
        let cfg = IqdbConfig::new(2, DistanceMetric::Euclidean).index(IndexKind::Ivf(
            IvfConfig::default()
                .with_n_clusters(2)
                .with_n_probes(2)
                .with_training_sample_size(64)
                .with_seed(7),
        ));
        let db = Iqdb::open_in_memory_with(cfg).unwrap();
        for (i, p) in [[0.0, 0.0], [0.1, -0.1], [10.0, 10.0], [9.9, 10.1]]
            .iter()
            .enumerate()
        {
            db.upsert(
                VectorId::from(i as u64),
                Vector::new(p.to_vec()).unwrap(),
                None,
            )
            .unwrap();
        }
        let hits = db.search(&vec2(0.0, 0.0), 1).unwrap();
        assert_eq!(hits[0].id, VectorId::from(0u64));
    }

    #[test]
    fn hnsw_handle_round_trip() {
        let cfg = IqdbConfig::new(2, DistanceMetric::Euclidean)
            .index(IndexKind::Hnsw(HnswConfig::default()));
        let db = Iqdb::open_in_memory_with(cfg).unwrap();
        db.upsert(VectorId::from(1u64), vec2(0.0, 0.0), None)
            .unwrap();
        db.upsert(VectorId::from(2u64), vec2(5.0, 5.0), None)
            .unwrap();
        let hits = db.search(&vec2(0.0, 0.0), 1).unwrap();
        assert_eq!(hits[0].id, VectorId::from(1u64));
    }
}
