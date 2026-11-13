// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! The durable core engine.
//!
//! [`IqdbCore`] is the type `iqdb` owns and `iqdb-persist` wraps. It holds
//! the authoritative [`RowStore`] (the single source of truth for `len` and
//! for rebuilds) and a derived search index ([`AnyIndex`], optionally behind
//! an [`iqdb_cache::CachedIndex`]). It implements
//! [`Index`](iqdb_index::Index) + [`IndexCore`](iqdb_index::IndexCore) so it
//! can be searched, and [`Persistable`] so it can be wrapped in
//! [`iqdb_persist::PersistedIndex`] for snapshot + write-ahead-log
//! durability — which calls `IndexCore::insert` / `delete` for live writes
//! and replay, so those methods are the complete authoritative operation.
//!
//! ## Two index lifecycles
//!
//! Flat and HNSW accept incremental inserts directly, so a write updates
//! both the store and the index and a following search is immediate. IVF
//! must be trained before it accepts data: inserts only touch the store and
//! set a dirty flag, and the index is (re)built from the store the first
//! time a search needs it ([`IqdbCore::ensure_ready`]). `len()` reads the
//! store, never the index, so it is correct regardless of whether the IVF
//! index has been materialized yet — which is what keeps
//! [`PersistedIndex::load`](iqdb_persist::PersistedIndex::load)'s
//! `len == header.n_vectors` cross-check satisfied.

pub(crate) mod codec;
pub(crate) mod index;
pub(crate) mod store;

use std::io::{Read, Write};
use std::sync::Arc;

use iqdb_build::build_into;
use iqdb_cache::{CacheConfig, CacheStats, CachedIndex};
use iqdb_flat::{FlatConfig, FlatIndex};
use iqdb_index::{Index, IndexCore, IndexStats};
use iqdb_persist::{PersistError, Persistable, Result as PersistResult};
use iqdb_types::{DistanceMetric, Hit, IqdbError, Metadata, Result, SearchParams, VectorId};

use crate::config::{CoreConfig, IndexKind};
use index::AnyIndex;
use store::RowStore;

/// Fewest rows an IVF-PQ codebook trains on (one per centroid of its
/// 256-entry codebooks).
const PQ_MIN_TRAINING_ROWS: usize = 256;

/// The search index, optionally wrapped in a result cache.
///
/// Both variants implement [`IndexCore`], so the rest of the engine treats
/// them uniformly. The cache invalidates on every mutation, so it never
/// serves a stale result.
enum CacheLayer {
    // Both variants are boxed: `AnyIndex` carries the (large) inline IVF /
    // HNSW state, so boxing keeps `CacheLayer` — and the `IqdbCore` that
    // holds it — pointer-sized.
    Plain(Box<AnyIndex>),
    Cached(Box<CachedIndex<AnyIndex>>),
}

impl CacheLayer {
    fn wrap(idx: AnyIndex, cache: &Option<CacheConfig>) -> Self {
        match cache {
            Some(cfg) => Self::Cached(Box::new(CachedIndex::with_config(idx, cfg.clone()))),
            None => Self::Plain(Box::new(idx)),
        }
    }

    fn into_inner(self) -> AnyIndex {
        match self {
            Self::Plain(idx) => *idx,
            Self::Cached(cached) => (*cached).into_inner(),
        }
    }

    fn needs_training(&self) -> bool {
        match self {
            Self::Plain(idx) => idx.needs_training(),
            Self::Cached(cached) => cached.get_ref().needs_training(),
        }
    }

    fn tombstones(&self) -> usize {
        match self {
            Self::Plain(idx) => idx.tombstones(),
            Self::Cached(cached) => cached.get_ref().tombstones(),
        }
    }

    fn wants_compaction(&self) -> bool {
        match self {
            Self::Plain(idx) => idx.wants_compaction(),
            Self::Cached(cached) => cached.get_ref().wants_compaction(),
        }
    }

    fn cache_stats(&self) -> Option<CacheStats> {
        match self {
            Self::Plain(_) => None,
            Self::Cached(cached) => Some(cached.cache_stats()),
        }
    }
}

impl IndexCore for CacheLayer {
    fn insert(
        &mut self,
        id: VectorId,
        vector: Arc<[f32]>,
        metadata: Option<Metadata>,
    ) -> Result<()> {
        match self {
            Self::Plain(i) => i.insert(id, vector, metadata),
            Self::Cached(c) => c.insert(id, vector, metadata),
        }
    }

    fn delete(&mut self, id: &VectorId) -> Result<()> {
        match self {
            Self::Plain(i) => i.delete(id),
            Self::Cached(c) => c.delete(id),
        }
    }

    fn search(&self, query: &[f32], params: &SearchParams) -> Result<Vec<Hit>> {
        match self {
            Self::Plain(i) => i.search(query, params),
            Self::Cached(c) => c.search(query, params),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Plain(i) => i.len(),
            Self::Cached(c) => c.len(),
        }
    }

    fn dim(&self) -> usize {
        match self {
            Self::Plain(i) => i.dim(),
            Self::Cached(c) => c.dim(),
        }
    }

    fn metric(&self) -> DistanceMetric {
        match self {
            Self::Plain(i) => i.metric(),
            Self::Cached(c) => c.metric(),
        }
    }

    fn flush(&mut self) -> Result<()> {
        match self {
            Self::Plain(i) => i.flush(),
            Self::Cached(c) => c.flush(),
        }
    }

    fn stats(&self) -> IndexStats {
        match self {
            Self::Plain(i) => i.stats(),
            Self::Cached(c) => c.stats(),
        }
    }
}

/// The authoritative, durable core engine for one `iqdb` database.
pub(crate) struct IqdbCore {
    dim: usize,
    metric: DistanceMetric,
    kind: IndexKind,
    store: RowStore,
    index: CacheLayer,
    cache_cfg: Option<CacheConfig>,
    /// `true` when the IVF index must be (re)built from the store before the
    /// next search. Always `false` for the flat and HNSW kinds.
    ivf_dirty: bool,
}

impl IqdbCore {
    /// Number of stored rows (authoritative).
    pub(crate) fn len(&self) -> usize {
        self.store.len()
    }

    /// `true` if the store is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    /// The fixed dimensionality.
    pub(crate) fn dim(&self) -> usize {
        self.dim
    }

    /// The fixed distance metric.
    pub(crate) fn metric(&self) -> DistanceMetric {
        self.metric
    }

    /// `true` if a search would first need to materialize the IVF index.
    /// Below the training threshold searches scan the store instead, so
    /// there is nothing to materialize.
    pub(crate) fn needs_materialization(&self) -> bool {
        self.ivf_dirty && !self.below_training_threshold()
    }

    /// `true` when this is an IVF database holding fewer rows than its
    /// clusters can be trained on: `n_clusters`, or 256 for IVF-PQ, whose
    /// codebooks need that many.
    fn below_training_threshold(&self) -> bool {
        match self.kind {
            IndexKind::Ivf(cfg) => {
                let min_rows = if cfg.use_pq {
                    cfg.n_clusters.max(PQ_MIN_TRAINING_ROWS)
                } else {
                    cfg.n_clusters
                };
                self.store.len() < min_rows
            }
            _ => false,
        }
    }

    /// `true` if `id` is currently stored.
    pub(crate) fn contains(&self, id: &VectorId) -> bool {
        self.store.contains(id)
    }

    /// Clone out the stored payload and metadata for `id`, if present.
    pub(crate) fn get_row(&self, id: &VectorId) -> Option<(Arc<[f32]>, Option<Metadata>)> {
        self.store
            .get(id)
            .map(|row| (Arc::clone(&row.vector), row.meta.clone()))
    }

    /// Cache statistics, if a cache is configured.
    pub(crate) fn cache_stats(&self) -> Option<CacheStats> {
        self.index.cache_stats()
    }

    /// Replace the cache configuration, re-wrapping the live index. Used by
    /// the handle to re-apply a cache after a `load` (the cache is a runtime
    /// choice, not persisted state).
    pub(crate) fn set_cache(&mut self, cache: Option<CacheConfig>) -> Result<()> {
        let placeholder = CacheLayer::wrap(
            AnyIndex::new(IndexKind::Flat, self.dim, self.metric)?,
            &None,
        );
        let current = std::mem::replace(&mut self.index, placeholder);
        self.cache_cfg = cache;
        self.index = CacheLayer::wrap(current.into_inner(), &self.cache_cfg);
        Ok(())
    }

    /// Materialize the IVF index from the store if a write left it stale.
    /// A no-op for the flat and HNSW kinds, and for an empty store.
    pub(crate) fn ensure_ready(&mut self) -> Result<()> {
        // Too few rows to train on: searches scan the store exactly until
        // enough rows arrive, so leave the index stale rather than fail.
        if !self.ivf_dirty || self.store.is_empty() || self.below_training_threshold() {
            return Ok(());
        }
        let mut idx = AnyIndex::new(self.kind, self.dim, self.metric)?;
        if idx.needs_training() {
            let sample: Vec<&[f32]> = self.store.iter().map(|row| row.vector.as_ref()).collect();
            idx.train(&sample)?;
        }
        let items: Vec<(VectorId, Arc<[f32]>, Option<Metadata>)> = self
            .store
            .iter()
            .map(|row| (row.id.clone(), Arc::clone(&row.vector), row.meta.clone()))
            .collect();
        let _inserted = build_into(&mut idx, items)?;
        self.index = CacheLayer::wrap(idx, &self.cache_cfg);
        self.ivf_dirty = false;
        Ok(())
    }

    /// Rebuild the approximate index from the current vectors. For IVF this
    /// retrains centroids (and PQ codebooks); for HNSW it drops deleted
    /// nodes from the graph; for flat it is a no-op.
    pub(crate) fn optimize(&mut self) -> Result<()> {
        if matches!(self.kind, IndexKind::Ivf(_)) {
            self.ivf_dirty = true;
        }
        if self.index.tombstones() > 0 {
            self.compact_index()?;
        }
        self.ensure_ready()
    }

    /// Search the materialized index. The caller (the handle) must have
    /// called [`ensure_ready`](Self::ensure_ready) first when
    /// [`needs_materialization`](Self::needs_materialization) is set.
    ///
    /// `k` is clamped to the row count, so a huge `k` never sizes an index
    /// buffer. An IVF database below its training threshold is searched
    /// exactly over the store.
    pub(crate) fn search(&self, query: &[f32], params: &SearchParams) -> Result<Vec<Hit>> {
        let clamped;
        let params = if params.k > self.store.len() {
            clamped = SearchParams {
                k: self.store.len(),
                ..params.clone()
            };
            &clamped
        } else {
            params
        };
        if self.ivf_dirty && self.below_training_threshold() {
            return self.exact_scan(query, params);
        }
        self.index.search(query, params)
    }

    /// Exact search over every stored row, for an IVF database too small
    /// to train. The store holds fewer rows than `n_clusters` here, so
    /// the scan is small.
    fn exact_scan(&self, query: &[f32], params: &SearchParams) -> Result<Vec<Hit>> {
        let mut flat = FlatIndex::new(self.dim, self.metric, FlatConfig)?;
        for row in self.store.iter() {
            flat.insert(row.id.clone(), Arc::clone(&row.vector), row.meta.clone())?;
        }
        flat.search(query, params)
    }

    /// Rebuild the HNSW graph without its deleted nodes. The index is
    /// unwrapped from its cache layer for the rebuild and re-wrapped
    /// after, which also clears any cached results.
    fn compact_index(&mut self) -> Result<()> {
        let placeholder = CacheLayer::wrap(
            AnyIndex::new(IndexKind::Flat, self.dim, self.metric)?,
            &None,
        );
        let mut idx = std::mem::replace(&mut self.index, placeholder).into_inner();
        let result = idx.compact();
        self.index = CacheLayer::wrap(idx, &self.cache_cfg);
        result
    }

    /// Compact the HNSW graph once deleted nodes outnumber live ones.
    fn maybe_compact(&mut self) -> Result<()> {
        if self.index.wants_compaction() {
            self.compact_index()
        } else {
            Ok(())
        }
    }

    /// Drop a stale index entry (ignoring an absent id) before reinserting.
    fn index_replace_delete(&mut self, id: &VectorId) -> Result<()> {
        match self.index.delete(id) {
            Ok(()) | Err(IqdbError::NotFound) => Ok(()),
            Err(other) => Err(other),
        }
    }
}

impl IndexCore for IqdbCore {
    fn insert(
        &mut self,
        id: VectorId,
        vector: Arc<[f32]>,
        metadata: Option<Metadata>,
    ) -> Result<()> {
        let newly = self
            .store
            .upsert(id.clone(), Arc::clone(&vector), metadata.clone());
        // Untrained IVF: keep the data in the store only; the index is built
        // from the store on the next search.
        if self.index.needs_training() {
            self.ivf_dirty = true;
            return Ok(());
        }
        // Flat / HNSW / trained IVF: keep the index in step with the store.
        if !newly {
            self.index_replace_delete(&id)?;
        }
        self.index.insert(id, vector, metadata)?;
        if !newly {
            // An update left a deleted HNSW node behind.
            self.maybe_compact()?;
        }
        Ok(())
    }

    fn delete(&mut self, id: &VectorId) -> Result<()> {
        let _existed = self.store.remove(id);
        if self.index.needs_training() {
            // Untrained IVF holds no data; the rebuild reads the store.
            self.ivf_dirty = true;
            return Ok(());
        }
        match self.index.delete(id) {
            Ok(()) | Err(IqdbError::NotFound) => {}
            Err(other) => return Err(other),
        }
        self.maybe_compact()
    }

    fn search(&self, query: &[f32], params: &SearchParams) -> Result<Vec<Hit>> {
        IqdbCore::search(self, query, params)
    }

    fn len(&self) -> usize {
        self.store.len()
    }

    fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn metric(&self) -> DistanceMetric {
        self.metric
    }

    fn flush(&mut self) -> Result<()> {
        self.index.flush()
    }

    fn stats(&self) -> IndexStats {
        let mut stats = self.index.stats();
        stats.n_vectors = self.store.len();
        stats
    }
}

impl Index for IqdbCore {
    type Config = CoreConfig;

    fn new(dim: usize, metric: DistanceMetric, config: Self::Config) -> Result<Self> {
        let idx = AnyIndex::new(config.index, dim, metric)?;
        let ivf_dirty = idx.needs_training();
        let index = CacheLayer::wrap(idx, &config.cache);
        Ok(Self {
            dim,
            metric,
            kind: config.index,
            store: RowStore::new(),
            index,
            cache_cfg: config.cache,
            ivf_dirty,
        })
    }
}

impl Persistable for IqdbCore {
    const INDEX_TYPE: &'static str = "iqdb-core";

    fn save_to(&self, writer: &mut dyn Write) -> PersistResult<()> {
        codec::encode(writer, self.kind, self.dim, self.metric, &self.store)
    }

    fn load_from(reader: &mut dyn Read) -> PersistResult<Self> {
        let decoded = codec::decode(reader)?;

        let mut store = RowStore::with_capacity(decoded.rows.len());
        for row in decoded.rows {
            let _ = store.upsert(row.id, row.vector, row.meta);
        }

        let mut idx =
            AnyIndex::new(decoded.kind, decoded.dim, decoded.metric).map_err(PersistError::from)?;
        let ivf_dirty = if idx.needs_training() {
            // IVF: defer training to the first search. Do NOT train here —
            // the WAL replay re-inserts these rows through `insert`, and an
            // untrained IVF defers them to the store, avoiding a double count.
            true
        } else {
            // Flat / HNSW: bulk-load now so `len()` matches the header.
            let items: Vec<(VectorId, Arc<[f32]>, Option<Metadata>)> = store
                .iter()
                .map(|row| (row.id.clone(), Arc::clone(&row.vector), row.meta.clone()))
                .collect();
            let _inserted = build_into(&mut idx, items).map_err(PersistError::from)?;
            false
        };

        // The cache is a runtime choice, not persisted; the handle re-applies
        // it after load via `set_cache`.
        let index = CacheLayer::wrap(idx, &None);
        Ok(Self {
            dim: decoded.dim,
            metric: decoded.metric,
            kind: decoded.kind,
            store,
            index,
            cache_cfg: None,
            ivf_dirty,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HnswConfig, IvfConfig};

    fn v(xs: &[f32]) -> Arc<[f32]> {
        Arc::from(xs)
    }

    fn core(kind: IndexKind, dim: usize, metric: DistanceMetric) -> IqdbCore {
        IqdbCore::new(
            dim,
            metric,
            CoreConfig {
                index: kind,
                cache: None,
            },
        )
        .unwrap()
    }

    fn save_load(c: &IqdbCore) -> IqdbCore {
        let mut bytes = Vec::new();
        c.save_to(&mut bytes).unwrap();
        IqdbCore::load_from(&mut &bytes[..]).unwrap()
    }

    #[test]
    fn flat_insert_search_and_round_trip() {
        let mut c = core(IndexKind::Flat, 2, DistanceMetric::Euclidean);
        c.insert(VectorId::from(1u64), v(&[0.0, 0.0]), None)
            .unwrap();
        c.insert(VectorId::from(2u64), v(&[3.0, 4.0]), None)
            .unwrap();
        assert_eq!(c.len(), 2);
        let hits = c
            .search(
                &[0.0, 0.0],
                &SearchParams::new(1, DistanceMetric::Euclidean),
            )
            .unwrap();
        assert_eq!(hits[0].id, VectorId::from(1u64));

        let restored = save_load(&c);
        assert_eq!(restored.len(), 2);
        assert_eq!(restored.dim(), 2);
        assert_eq!(restored.metric(), DistanceMetric::Euclidean);
        let hits = restored
            .search(
                &[0.0, 0.0],
                &SearchParams::new(1, DistanceMetric::Euclidean),
            )
            .unwrap();
        assert_eq!(hits[0].id, VectorId::from(1u64));
    }

    #[test]
    fn upsert_replaces_in_index_not_duplicates() {
        let mut c = core(IndexKind::Flat, 2, DistanceMetric::Euclidean);
        c.insert(VectorId::from(1u64), v(&[0.0, 0.0]), None)
            .unwrap();
        c.insert(VectorId::from(1u64), v(&[5.0, 5.0]), None)
            .unwrap();
        assert_eq!(c.len(), 1);
        let (got, _) = c.get_row(&VectorId::from(1u64)).unwrap();
        assert_eq!(got.as_ref(), &[5.0, 5.0]);
    }

    #[test]
    fn delete_is_idempotent() {
        let mut c = core(IndexKind::Flat, 1, DistanceMetric::Euclidean);
        c.insert(VectorId::from(1u64), v(&[1.0]), None).unwrap();
        assert!(c.contains(&VectorId::from(1u64)));
        c.delete(&VectorId::from(1u64)).unwrap();
        c.delete(&VectorId::from(1u64)).unwrap(); // no error on absent
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn hnsw_round_trip_rebuilds_index() {
        let mut c = core(
            IndexKind::Hnsw(HnswConfig::default()),
            2,
            DistanceMetric::Euclidean,
        );
        for i in 0..20u64 {
            c.insert(VectorId::from(i), v(&[i as f32, 0.0]), None)
                .unwrap();
        }
        let restored = save_load(&c);
        assert_eq!(restored.len(), 20);
        let hits = restored
            .search(
                &[0.0, 0.0],
                &SearchParams::new(1, DistanceMetric::Euclidean),
            )
            .unwrap();
        assert_eq!(hits[0].id, VectorId::from(0u64));
    }

    #[test]
    fn ivf_materializes_lazily_on_search() {
        let cfg = IvfConfig::default()
            .with_n_clusters(2)
            .with_n_probes(2)
            .with_training_sample_size(64)
            .with_seed(7);
        let mut c = core(IndexKind::Ivf(cfg), 2, DistanceMetric::Euclidean);
        // A cluster each side of the origin.
        let pts = [
            [0.0, 0.0],
            [0.1, -0.1],
            [-0.1, 0.1],
            [10.0, 10.0],
            [10.1, 9.9],
            [9.9, 10.1],
        ];
        for (i, p) in pts.iter().enumerate() {
            c.insert(VectorId::from(i as u64), v(p), None).unwrap();
        }
        assert!(c.needs_materialization());
        assert_eq!(c.len(), 6); // authoritative even before materialization

        c.ensure_ready().unwrap();
        assert!(!c.needs_materialization());
        let hits = c
            .search(
                &[0.0, 0.0],
                &SearchParams::new(1, DistanceMetric::Euclidean),
            )
            .unwrap();
        assert_eq!(hits[0].id, VectorId::from(0u64));

        // Round-trip: IVF rebuilds and stays correct.
        let mut restored = save_load(&c);
        assert_eq!(restored.len(), 6);
        assert!(restored.needs_materialization());
        restored.ensure_ready().unwrap();
        let hits = restored
            .search(
                &[10.0, 10.0],
                &SearchParams::new(1, DistanceMetric::Euclidean),
            )
            .unwrap();
        assert_eq!(hits[0].id, VectorId::from(3u64));
    }

    #[test]
    fn set_cache_preserves_data_and_enables_stats() {
        let mut c = core(IndexKind::Flat, 2, DistanceMetric::Cosine);
        c.insert(VectorId::from(1u64), v(&[1.0, 0.0]), None)
            .unwrap();
        assert!(c.cache_stats().is_none());

        c.set_cache(Some(CacheConfig::new().capacity(16))).unwrap();
        assert!(c.cache_stats().is_some());
        let params = SearchParams::new(1, DistanceMetric::Cosine);
        let _ = c.search(&[1.0, 0.0], &params).unwrap();
        let _ = c.search(&[1.0, 0.0], &params).unwrap();
        assert_eq!(c.cache_stats().unwrap().hits, 1);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn snapshot_with_oversized_index_config_fails_to_load() {
        // A snapshot whose CRC is valid but whose stored config asks for an
        // impossible graph or cluster table must fail the load, not size
        // allocations from those fields.
        let mut c = core(
            IndexKind::Hnsw(HnswConfig::default()),
            2,
            DistanceMetric::Euclidean,
        );
        c.insert(VectorId::from(1u64), v(&[1.0, 0.0]), None)
            .unwrap();
        c.kind = IndexKind::Hnsw(
            HnswConfig::default()
                .with_m(1 << 40)
                .with_ef_construction(1 << 41),
        );
        let mut bytes = Vec::new();
        c.save_to(&mut bytes).unwrap();
        assert!(IqdbCore::load_from(&mut &bytes[..]).is_err());

        c.kind = IndexKind::Ivf(IvfConfig::default().with_n_clusters(1 << 40));
        let mut bytes = Vec::new();
        c.save_to(&mut bytes).unwrap();
        assert!(IqdbCore::load_from(&mut &bytes[..]).is_err());
    }

    #[test]
    fn huge_k_is_clamped_to_the_row_count() {
        for kind in [IndexKind::Flat, IndexKind::Hnsw(HnswConfig::default())] {
            let mut c = core(kind, 2, DistanceMetric::Euclidean);
            for i in 0..5u64 {
                c.insert(VectorId::from(i), v(&[i as f32, 0.0]), None)
                    .unwrap();
            }
            let hits = c
                .search(
                    &[0.0, 0.0],
                    &SearchParams::new(usize::MAX / 2, DistanceMetric::Euclidean),
                )
                .unwrap();
            assert_eq!(hits.len(), 5);
        }
    }

    #[test]
    fn small_ivf_database_is_searched_exactly() {
        // Default config: 256 clusters, far more than the rows stored.
        let mut c = core(
            IndexKind::Ivf(IvfConfig::default()),
            2,
            DistanceMetric::Euclidean,
        );
        for i in 0..100u64 {
            c.insert(VectorId::from(i), v(&[i as f32, 1.0]), None)
                .unwrap();
        }
        assert!(!c.needs_materialization());
        c.ensure_ready().unwrap();
        let hits = c
            .search(
                &[10.2, 1.0],
                &SearchParams::new(3, DistanceMetric::Euclidean),
            )
            .unwrap();
        let ids: Vec<VectorId> = hits.into_iter().map(|h| h.id).collect();
        assert_eq!(
            ids,
            vec![
                VectorId::from(10u64),
                VectorId::from(11u64),
                VectorId::from(9u64)
            ]
        );
        // Once enough rows arrive, the index trains on the next search.
        let cfg = IvfConfig::default().with_n_clusters(4).with_n_probes(4);
        let mut c = core(IndexKind::Ivf(cfg), 2, DistanceMetric::Euclidean);
        for i in 0..3u64 {
            c.insert(VectorId::from(i), v(&[i as f32, 0.0]), None)
                .unwrap();
        }
        assert!(!c.needs_materialization());
        c.insert(VectorId::from(3u64), v(&[3.0, 0.0]), None)
            .unwrap();
        assert!(c.needs_materialization());
        c.ensure_ready().unwrap();
        assert!(!c.needs_materialization());
    }

    #[test]
    fn hnsw_updates_do_not_grow_the_graph_without_bound() {
        let mut c = core(
            IndexKind::Hnsw(HnswConfig::default()),
            8,
            DistanceMetric::Euclidean,
        );
        // 100 rows, each rewritten 50 times: 4,900 replaced nodes.
        for round in 0..50u64 {
            for i in 0..100u64 {
                let x: Vec<f32> = (0..8).map(|j| (i * 8 + j + round) as f32).collect();
                c.insert(VectorId::from(i), v(&x), None).unwrap();
            }
        }
        assert_eq!(c.len(), 100);
        let dead = c.index.tombstones();
        assert!(
            dead <= index::COMPACT_MIN_TOMBSTONES,
            "{dead} deleted nodes still held"
        );

        // `optimize` drops whatever is left.
        c.optimize().unwrap();
        assert_eq!(c.index.tombstones(), 0);
        let hits = c
            .search(&[0.0; 8], &SearchParams::new(1, DistanceMetric::Euclidean))
            .unwrap();
        assert_eq!(hits[0].id, VectorId::from(0u64));
    }
}
