// Copyright 2026 James Gober.
// Licensed under Apache-2.0 OR MIT.

//! # iqdb — embedded vector database for Rust
//!
//! `iqdb` is a single-process, in-application similarity-search engine for
//! high-dimensional workloads. It targets the operational shape of
//! [`sqlite`] or [`redb`]: no daemon, no network hop, no separate runtime.
//! Open a handle, write vectors, query nearest neighbours — all from inside
//! your binary.
//!
//! The `0.5.0` release re-platforms `iqdb` onto the **iqdb crate family**:
//! it is now the integration layer that composes the family's shared
//! vocabulary ([`iqdb_types`]), its index seam ([`iqdb_index`]), the exact
//! and approximate index implementations ([`iqdb_flat`], [`iqdb_hnsw`],
//! [`iqdb_ivf`]), durable storage ([`iqdb_persist`]), and an optional result
//! cache ([`iqdb_cache`]). The public vocabulary — [`Vector`], [`VectorId`],
//! [`Metadata`], [`Value`], [`Hit`], [`Filter`], [`DistanceMetric`] — is
//! re-exported from the family so it agrees across every `iqdb-*` crate.
//!
//! ## The handle
//!
//! [`Iqdb`] is the one type most callers construct. A database fixes its
//! dimensionality and [`DistanceMetric`] at open time, then exposes a small
//! surface: `upsert` / `get` / `delete` for record management, `search` /
//! `search_with` (plus batch variants) for top-`k` similarity search, and
//! `flush` / `optimize` / `close` for maintenance.
//!
//! ## Choosing an index
//!
//! Tier 1 — [`Iqdb::open_in_memory`] and [`Iqdb::open`] default to the exact
//! flat index. Tier 2 — pass an [`IqdbConfig`] to
//! [`Iqdb::open_in_memory_with`] / [`Iqdb::open_with`] to select an
//! approximate index ([`IndexKind::Hnsw`] / [`IndexKind::Ivf`]) and tune it,
//! or to attach a [`CacheConfig`]. Flat is the recall ground truth that the
//! approximate indices are measured against.
//!
//! ## Async
//!
//! Enable the `async` feature for `AsyncIqdb`, a Tokio adapter that mirrors
//! the [`Iqdb`] surface and offloads each blocking call onto Tokio's blocking
//! pool via `spawn_blocking`. The synchronous API and the default build are
//! unchanged; no Tokio is pulled unless `async` is enabled.
//!
//! [`sqlite`]: https://www.sqlite.org/
//! [`redb`]: https://crates.io/crates/redb
//!
//! # Examples
//!
//! Open an in-memory database, upsert a record, look it up, and search:
//!
//! ```
//! use iqdb::{DistanceMetric, Iqdb, Result, Vector, VectorId};
//!
//! fn run() -> Result<()> {
//!     let db = Iqdb::open_in_memory(3, DistanceMetric::Cosine)?;
//!
//!     db.upsert(VectorId::from(1u64), Vector::new(vec![0.1, 0.2, 0.3])?, None)?;
//!     db.upsert(VectorId::from(2u64), Vector::new(vec![0.9, 0.1, 0.0])?, None)?;
//!
//!     let hits = db.search(&Vector::new(vec![0.1, 0.2, 0.3])?, 1)?;
//!     assert_eq!(hits[0].id, VectorId::from(1u64));
//!     Ok(())
//! }
//! # run().unwrap();
//! ```
//!
//! Select an HNSW index and a result cache through [`IqdbConfig`]:
//!
//! ```
//! use iqdb::{CacheConfig, DistanceMetric, HnswConfig, IndexKind, Iqdb, IqdbConfig};
//!
//! # fn run() -> iqdb::Result<()> {
//! let cfg = IqdbConfig::new(128, DistanceMetric::Cosine)
//!     .index(IndexKind::Hnsw(HnswConfig::default().with_ef_search(96)))
//!     .cache(CacheConfig::new().capacity(10_000));
//! let db = Iqdb::open_in_memory_with(cfg)?;
//! assert!(db.is_empty());
//! # Ok(())
//! # }
//! # run().unwrap();
//! ```

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]
#![deny(unused_results)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::unreachable)]
#![deny(clippy::undocumented_unsafe_blocks)]
// Test code is allowed to use the convenience panickers — the strict lint
// profile above is for production library code, not assertion scaffolding
// inside `#[cfg(test)]` modules.
#![cfg_attr(
    test,
    allow(
        unused_results,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::print_stdout,
        clippy::print_stderr
    )
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "async")]
mod async_db;
mod config;
mod engine;
mod error;
mod handle;

#[cfg(feature = "async")]
#[cfg_attr(docsrs, doc(cfg(feature = "async")))]
pub use async_db::AsyncIqdb;
pub use config::{
    CacheConfig, Compression, EvictionPolicy, FsyncPolicy, HnswConfig, IndexKind, IqdbConfig,
    IvfConfig,
};
pub use error::{Error, Result};
pub use handle::Iqdb;

// Re-export the cache-statistics type so `Iqdb::cache_stats` is usable
// without a second dependency.
pub use iqdb_cache::CacheStats;

// The shared vocabulary, re-exported from `iqdb-types` so it is identical
// across the whole iqdb family.
pub use iqdb_types::{
    DistanceMetric, Filter, Hit, Metadata, SearchParams, Value, Vector, VectorId,
};
