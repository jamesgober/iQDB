// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! The derived index abstraction.
//!
//! [`AnyIndex`] is a closed enum over the three family index
//! implementations. It dispatches the object-safe
//! [`IndexCore`](iqdb_index::IndexCore) surface arm-by-arm — a `match`
//! the compiler inlines, with no `dyn` indirection on the query hot path —
//! and adds the training hooks the IVF lifecycle needs.
//!
//! IVF training (`train` / `retrain` / `is_trained`) is **not** part of the
//! `IndexCore` or `Index` trait; it lives as inherent methods on
//! [`iqdb_ivf::IvfIndex`]. The enum is what makes those reachable from the
//! core engine: a `Box<dyn IndexCore>` could not see them.

use std::sync::Arc;

use iqdb_flat::{FlatConfig, FlatIndex};
use iqdb_hnsw::HnswIndex;
use iqdb_index::{Index, IndexCore, IndexStats};
use iqdb_ivf::IvfIndex;
use iqdb_types::{DistanceMetric, Hit, Metadata, Result, SearchParams, VectorId};

use crate::config::IndexKind;

/// One of the three family index implementations, behind a single type.
#[derive(Debug)]
pub(crate) enum AnyIndex {
    /// Exact brute-force search.
    Flat(FlatIndex),
    /// HNSW graph index (approximate).
    Hnsw(HnswIndex),
    /// IVF clustered index (approximate); requires training before use.
    Ivf(IvfIndex),
}

impl AnyIndex {
    /// Construct an empty index of the given `kind`, dimensionality, and
    /// metric. An [`AnyIndex::Ivf`] comes back **untrained** — the core
    /// engine trains it lazily from the row store before the first search.
    ///
    /// # Errors
    ///
    /// Propagates the family constructor's [`iqdb_types::IqdbError`] — for
    /// example, `InvalidConfig` for `dim == 0`, or `InvalidMetric` for an
    /// IVF-PQ configuration under a metric it does not support.
    pub(crate) fn new(kind: IndexKind, dim: usize, metric: DistanceMetric) -> Result<Self> {
        Ok(match kind {
            IndexKind::Flat => Self::Flat(FlatIndex::new(dim, metric, FlatConfig)?),
            IndexKind::Hnsw(cfg) => Self::Hnsw(HnswIndex::new(dim, metric, cfg)?),
            IndexKind::Ivf(cfg) => Self::Ivf(IvfIndex::new(dim, metric, cfg)?),
        })
    }

    /// `true` if this index needs a `train` call before it can accept
    /// inserts or answer searches — only an untrained IVF does.
    pub(crate) fn needs_training(&self) -> bool {
        match self {
            Self::Ivf(idx) => !idx.is_trained(),
            Self::Flat(_) | Self::Hnsw(_) => false,
        }
    }

    /// Train the index on `sample` if it is an untrained IVF; a no-op for the
    /// flat and HNSW kinds (which need no training).
    ///
    /// # Errors
    ///
    /// Propagates [`iqdb_ivf::IvfIndex::train`]'s error.
    pub(crate) fn train(&mut self, sample: &[&[f32]]) -> Result<()> {
        match self {
            Self::Ivf(idx) => idx.train(sample),
            Self::Flat(_) | Self::Hnsw(_) => Ok(()),
        }
    }
}

impl IndexCore for AnyIndex {
    fn insert(
        &mut self,
        id: VectorId,
        vector: Arc<[f32]>,
        metadata: Option<Metadata>,
    ) -> Result<()> {
        match self {
            Self::Flat(i) => i.insert(id, vector, metadata),
            Self::Hnsw(i) => i.insert(id, vector, metadata),
            Self::Ivf(i) => i.insert(id, vector, metadata),
        }
    }

    fn delete(&mut self, id: &VectorId) -> Result<()> {
        match self {
            Self::Flat(i) => i.delete(id),
            Self::Hnsw(i) => i.delete(id),
            Self::Ivf(i) => i.delete(id),
        }
    }

    fn search(&self, query: &[f32], params: &SearchParams) -> Result<Vec<Hit>> {
        match self {
            Self::Flat(i) => i.search(query, params),
            Self::Hnsw(i) => i.search(query, params),
            Self::Ivf(i) => i.search(query, params),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Flat(i) => i.len(),
            Self::Hnsw(i) => i.len(),
            Self::Ivf(i) => i.len(),
        }
    }

    fn dim(&self) -> usize {
        match self {
            Self::Flat(i) => i.dim(),
            Self::Hnsw(i) => i.dim(),
            Self::Ivf(i) => i.dim(),
        }
    }

    fn metric(&self) -> DistanceMetric {
        match self {
            Self::Flat(i) => i.metric(),
            Self::Hnsw(i) => i.metric(),
            Self::Ivf(i) => i.metric(),
        }
    }

    fn flush(&mut self) -> Result<()> {
        match self {
            Self::Flat(i) => i.flush(),
            Self::Hnsw(i) => i.flush(),
            Self::Ivf(i) => i.flush(),
        }
    }

    fn stats(&self) -> IndexStats {
        match self {
            Self::Flat(i) => i.stats(),
            Self::Hnsw(i) => i.stats(),
            Self::Ivf(i) => i.stats(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HnswConfig, IvfConfig};

    fn v(xs: &[f32]) -> Arc<[f32]> {
        Arc::from(xs)
    }

    #[test]
    fn flat_inserts_and_searches() {
        let mut idx = AnyIndex::new(IndexKind::Flat, 2, DistanceMetric::Euclidean).unwrap();
        assert!(!idx.needs_training());
        idx.insert(VectorId::from(1u64), v(&[0.0, 0.0]), None)
            .unwrap();
        idx.insert(VectorId::from(2u64), v(&[3.0, 4.0]), None)
            .unwrap();
        let hits = idx
            .search(
                &[0.0, 0.0],
                &SearchParams::new(1, DistanceMetric::Euclidean),
            )
            .unwrap();
        assert_eq!(hits[0].id, VectorId::from(1u64));
    }

    #[test]
    fn hnsw_inserts_without_training() {
        let mut idx = AnyIndex::new(
            IndexKind::Hnsw(HnswConfig::default()),
            2,
            DistanceMetric::Euclidean,
        )
        .unwrap();
        assert!(!idx.needs_training());
        idx.insert(VectorId::from(1u64), v(&[0.0, 0.0]), None)
            .unwrap();
        idx.insert(VectorId::from(2u64), v(&[3.0, 4.0]), None)
            .unwrap();
        let hits = idx
            .search(
                &[0.0, 0.0],
                &SearchParams::new(1, DistanceMetric::Euclidean),
            )
            .unwrap();
        assert_eq!(hits[0].id, VectorId::from(1u64));
    }

    #[test]
    fn ivf_needs_training_then_searches() {
        let cfg = IvfConfig::default()
            .with_n_clusters(2)
            .with_n_probes(2)
            .with_training_sample_size(16)
            .with_seed(7);
        let mut idx = AnyIndex::new(IndexKind::Ivf(cfg), 2, DistanceMetric::Euclidean).unwrap();
        assert!(idx.needs_training());

        let sample = [
            [0.0_f32, 0.0],
            [0.1, -0.1],
            [-0.1, 0.1],
            [10.0, 10.0],
            [10.1, 9.9],
            [9.9, 10.1],
        ];
        let refs: Vec<&[f32]> = sample.iter().map(|s| s.as_slice()).collect();
        idx.train(&refs).unwrap();
        assert!(!idx.needs_training());

        idx.insert(VectorId::from(1u64), v(&[0.0, 0.0]), None)
            .unwrap();
        idx.insert(VectorId::from(2u64), v(&[10.0, 10.0]), None)
            .unwrap();
        let hits = idx
            .search(
                &[0.0, 0.0],
                &SearchParams::new(1, DistanceMetric::Euclidean),
            )
            .unwrap();
        assert_eq!(hits[0].id, VectorId::from(1u64));
    }
}
