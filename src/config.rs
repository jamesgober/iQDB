// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Construction-time configuration for an [`Iqdb`](crate::Iqdb) handle.
//!
//! A database is opened with a fixed dimensionality and distance metric — an
//! HNSW graph and an IVF clustering are both built around a specific `dim`
//! and `metric`, so neither can change without rebuilding. [`IqdbConfig`]
//! captures those two required values plus two optional choices: which index
//! backs the search ([`IndexKind`]), and whether searches are cached
//! ([`CacheConfig`]).
//!
//! The config is **fluent**, mirroring the family's own
//! [`iqdb_cache::CacheConfig`] idiom — start from [`IqdbConfig::new`] and
//! chain the overrides you want:
//!
//! ```
//! use iqdb::{DistanceMetric, IndexKind, IqdbConfig};
//! use iqdb::HnswConfig;
//!
//! let cfg = IqdbConfig::new(128, DistanceMetric::Cosine)
//!     .index(IndexKind::Hnsw(HnswConfig::default().with_ef_search(96)));
//!
//! assert_eq!(cfg.dim(), 128);
//! ```
//!
//! Per-index tuning lives inside the [`IndexKind`] variants (each carries the
//! family's own parameter struct), so [`IqdbConfig`] stays small and never
//! grows into a god-config.

use iqdb_types::DistanceMetric;

pub use iqdb_cache::{CacheConfig, EvictionPolicy};
pub use iqdb_hnsw::HnswConfig;
pub use iqdb_ivf::IvfConfig;
pub use iqdb_persist::{Compression, FsyncPolicy};

/// Which index implementation backs an [`Iqdb`](crate::Iqdb) handle.
///
/// `Flat` is the exact, brute-force ground truth (no tuning, no training).
/// `Hnsw` and `Ivf` are approximate: each carries the family's own
/// parameter struct so its knobs are tuned in one place.
///
/// The default is [`IndexKind::Flat`] — exact search with zero ceremony.
///
/// # Examples
///
/// ```
/// use iqdb::{IndexKind, IvfConfig};
///
/// // Exact search (the default).
/// let exact = IndexKind::default();
/// assert!(matches!(exact, IndexKind::Flat));
///
/// // IVF with a tuned probe count.
/// let ivf = IndexKind::Ivf(IvfConfig::default().with_n_probes(16));
/// assert!(matches!(ivf, IndexKind::Ivf(_)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndexKind {
    /// Exact brute-force search. The recall ground truth; no tuning needed.
    #[default]
    Flat,
    /// Hierarchical Navigable Small World graph index (approximate). The
    /// payload tunes graph degree and beam width — see [`HnswConfig`].
    Hnsw(HnswConfig),
    /// Inverted-file clustered index (approximate), IVF-Flat or IVF-PQ. The
    /// payload tunes cluster count, probe count, and quantization — see
    /// [`IvfConfig`].
    Ivf(IvfConfig),
}

impl IndexKind {
    /// A short, stable identifier for the variant — used in diagnostics and
    /// in the on-disk payload's index-kind tag.
    #[must_use]
    pub(crate) const fn tag(&self) -> u8 {
        match self {
            Self::Flat => 0,
            Self::Hnsw(_) => 1,
            Self::Ivf(_) => 2,
        }
    }
}

/// The index + cache half of [`IqdbConfig`], used as the
/// [`Index::Config`](iqdb_index::Index::Config) for the internal core. Kept
/// `pub(crate)`: callers configure through [`IqdbConfig`], never this.
#[derive(Debug, Clone, Default)]
pub(crate) struct CoreConfig {
    pub(crate) index: IndexKind,
    pub(crate) cache: Option<CacheConfig>,
}

/// Durable-storage tuning for the file-backed path. Ignored by the in-memory
/// backend (there is nothing to sync or compress). Defaults to the safest
/// point: fsync every acknowledged write, no compression.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Durability {
    pub(crate) fsync: FsyncPolicy,
    pub(crate) compression: Compression,
}

impl Default for Durability {
    fn default() -> Self {
        Self {
            fsync: FsyncPolicy::Always,
            compression: Compression::None,
        }
    }
}

/// Construction-time configuration for an [`Iqdb`](crate::Iqdb) handle.
///
/// Build one with [`IqdbConfig::new`] and chain the optional overrides. Pass
/// it to [`Iqdb::open_in_memory_with`](crate::Iqdb::open_in_memory_with) or
/// [`Iqdb::open_with`](crate::Iqdb::open_with).
///
/// # Examples
///
/// ```
/// use iqdb::{CacheConfig, DistanceMetric, IndexKind, IqdbConfig, IvfConfig};
///
/// let cfg = IqdbConfig::new(64, DistanceMetric::Euclidean)
///     .index(IndexKind::Ivf(IvfConfig::default().with_n_clusters(256)))
///     .cache(CacheConfig::new().capacity(10_000));
///
/// assert_eq!(cfg.dim(), 64);
/// assert_eq!(cfg.metric(), DistanceMetric::Euclidean);
/// ```
#[derive(Debug, Clone)]
pub struct IqdbConfig {
    dim: usize,
    metric: DistanceMetric,
    core: CoreConfig,
    durability: Durability,
}

impl IqdbConfig {
    /// Start a config for a `dim`-dimensional database compared under
    /// `metric`. Defaults to an exact [`IndexKind::Flat`] index with no
    /// cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{DistanceMetric, IqdbConfig};
    ///
    /// let cfg = IqdbConfig::new(3, DistanceMetric::Cosine);
    /// assert_eq!(cfg.dim(), 3);
    /// ```
    #[must_use]
    pub fn new(dim: usize, metric: DistanceMetric) -> Self {
        Self {
            dim,
            metric,
            core: CoreConfig::default(),
            durability: Durability::default(),
        }
    }

    /// Select the index implementation (and its tuning).
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{DistanceMetric, HnswConfig, IndexKind, IqdbConfig};
    ///
    /// let cfg = IqdbConfig::new(128, DistanceMetric::Cosine)
    ///     .index(IndexKind::Hnsw(HnswConfig::default().with_m(32)));
    /// assert!(matches!(cfg.index_kind(), IndexKind::Hnsw(_)));
    /// ```
    #[must_use]
    pub fn index(mut self, kind: IndexKind) -> Self {
        self.core.index = kind;
        self
    }

    /// Wrap searches in a result cache with the given configuration. Without
    /// this call the database is uncached (correct, just not memoized).
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{CacheConfig, DistanceMetric, IqdbConfig};
    ///
    /// let cfg = IqdbConfig::new(16, DistanceMetric::Cosine)
    ///     .cache(CacheConfig::new().capacity(4_096));
    /// assert!(cfg.is_cached());
    /// ```
    #[must_use]
    pub fn cache(mut self, cache: CacheConfig) -> Self {
        self.core.cache = Some(cache);
        self
    }

    /// Set the write-ahead-log fsync cadence for the durable, file-backed
    /// path. Ignored by an in-memory database.
    ///
    /// The default, [`FsyncPolicy::Always`], makes every acknowledged write
    /// durable before it returns. [`FsyncPolicy::Periodic`] bounds the
    /// un-synced window for throughput; [`FsyncPolicy::Never`] is for tests
    /// and tmpfs-backed paths only.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{DistanceMetric, FsyncPolicy, IqdbConfig};
    /// use std::time::Duration;
    ///
    /// let cfg = IqdbConfig::new(16, DistanceMetric::Cosine)
    ///     .fsync(FsyncPolicy::Periodic(Duration::from_millis(50)));
    /// # let _ = cfg;
    /// ```
    #[must_use]
    pub fn fsync(mut self, policy: FsyncPolicy) -> Self {
        self.durability.fsync = policy;
        self
    }

    /// Set the snapshot compression for the durable, file-backed path.
    /// Ignored by an in-memory database.
    ///
    /// [`Compression::Zstd`] and [`Compression::Lz4`] require the matching
    /// `zstd` / `lz4` cargo feature; opening with one whose feature is not
    /// compiled in fails with [`Error::Persist`](crate::Error::Persist).
    /// The default is [`Compression::None`].
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{Compression, DistanceMetric, IqdbConfig};
    ///
    /// let cfg = IqdbConfig::new(16, DistanceMetric::Cosine)
    ///     .compression(Compression::Zstd { level: 3 });
    /// # let _ = cfg;
    /// ```
    #[must_use]
    pub fn compression(mut self, compression: Compression) -> Self {
        self.durability.compression = compression;
        self
    }

    /// The configured dimensionality.
    #[must_use]
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// The configured distance metric.
    #[must_use]
    pub fn metric(&self) -> DistanceMetric {
        self.metric
    }

    /// The configured index kind.
    #[must_use]
    pub fn index_kind(&self) -> IndexKind {
        self.core.index
    }

    /// `true` if a result cache is configured.
    #[must_use]
    pub fn is_cached(&self) -> bool {
        self.core.cache.is_some()
    }

    /// Decompose into the parts the handle hands to the core constructor and
    /// the durable-storage layer.
    pub(crate) fn into_parts(self) -> (usize, DistanceMetric, CoreConfig, Durability) {
        (self.dim, self.metric, self.core, self.durability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_index_kind_is_flat() {
        assert_eq!(IndexKind::default(), IndexKind::Flat);
        assert_eq!(IndexKind::Flat.tag(), 0);
    }

    #[test]
    fn index_kind_tags_are_stable() {
        assert_eq!(IndexKind::Hnsw(HnswConfig::default()).tag(), 1);
        assert_eq!(IndexKind::Ivf(IvfConfig::default()).tag(), 2);
    }

    #[test]
    fn fluent_builder_threads_choices() {
        let cfg = IqdbConfig::new(8, DistanceMetric::Cosine)
            .index(IndexKind::Ivf(IvfConfig::default().with_n_probes(4)))
            .cache(CacheConfig::new().capacity(512));
        assert_eq!(cfg.dim(), 8);
        assert_eq!(cfg.metric(), DistanceMetric::Cosine);
        assert!(cfg.is_cached());
        assert!(matches!(cfg.index_kind(), IndexKind::Ivf(_)));
    }

    #[test]
    fn defaults_are_flat_uncached_and_safely_durable() {
        let cfg = IqdbConfig::new(4, DistanceMetric::Euclidean);
        assert!(!cfg.is_cached());
        assert_eq!(cfg.index_kind(), IndexKind::Flat);
        let (dim, metric, core, durability) = cfg.into_parts();
        assert_eq!(dim, 4);
        assert_eq!(metric, DistanceMetric::Euclidean);
        assert!(core.cache.is_none());
        assert_eq!(durability.fsync, FsyncPolicy::Always);
        assert_eq!(durability.compression, Compression::None);
    }

    #[test]
    fn durability_knobs_thread_through() {
        let cfg = IqdbConfig::new(4, DistanceMetric::Cosine)
            .fsync(FsyncPolicy::Never)
            .compression(Compression::Lz4);
        let (_, _, _, durability) = cfg.into_parts();
        assert_eq!(durability.fsync, FsyncPolicy::Never);
        assert_eq!(durability.compression, Compression::Lz4);
    }
}
