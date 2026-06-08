<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br><b>iqdb</b><br>
    <sub><sup>API REFERENCE</sup></sub>
</h1>
<div align="center">
    <sup>
        <a href="../README.md" title="Project Home"><b>HOME</b></a>
        <span>&nbsp;│&nbsp;</span>
        <span>API</span>
        <span>&nbsp;│&nbsp;</span>
        <a href="../CHANGELOG.md" title="Changelog"><b>CHANGELOG</b></a>
        <span>&nbsp;│&nbsp;</span>
        <a href="../REPS.md" title="Standards"><b>STANDARDS</b></a>
    </sup>
</div>
<br>

This document is the canonical API reference for **iqdb v0.7.0**. Every public type, method, error variant, and feature flag is recorded here with parameter descriptions and at least one runnable example. The narrative companion is the [README](../README.md); the per-release notes live under [`docs/release/`](./release/).

As of v0.5.0, `iqdb` is the integration layer over the **iqdb crate family**. The vector vocabulary it exposes (`Vector`, `VectorId`, `Metadata`, `Value`, `Hit`, `Filter`, `DistanceMetric`, `SearchParams`) is re-exported from [`iqdb-types`](https://docs.rs/iqdb-types); the tuning structs `HnswConfig`, `IvfConfig`, and `CacheConfig` are re-exported from the respective family crates. They are documented here as part of iqdb's public surface, with links to the originating crate for the exhaustive reference.

## Table of Contents

- [Installation](#installation)
- [Quick Start](#quick-start)
- [Error Handling](#error-handling)
- [Public APIs](#public-apis)
  - [`Iqdb`](#iqdb)
  - [`AsyncIqdb`](#asynciqdb)
  - [`IqdbConfig`](#iqdbconfig)
  - [`IndexKind`](#indexkind)
  - [`HnswConfig` / `IvfConfig`](#hnswconfig--ivfconfig)
  - [`CacheConfig` / `CacheStats`](#cacheconfig--cachestats)
  - [`Vector`](#vector)
  - [`VectorId`](#vectorid)
  - [`Metadata` / `Value`](#metadata--value)
  - [`Filter`](#filter)
  - [`DistanceMetric`](#distancemetric)
  - [`Hit`](#hit)
  - [`Error` / `Result`](#error--result)
- [Feature Flags](#feature-flags)
- [Behavioural Notes](#behavioural-notes)

## Installation

```toml
[dependencies]
iqdb = "0.5"
```

With optional features:

```toml
[dependencies]
iqdb = { version = "0.5", features = ["serde", "parallel", "zstd"] }
```

MSRV is Rust **1.87**.

## Quick Start

```rust
use iqdb::{DistanceMetric, Iqdb, Result, Vector, VectorId};

fn main() -> Result<()> {
    let db = Iqdb::open_in_memory(3, DistanceMetric::Cosine)?;
    db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0, 0.0])?, None)?;
    db.upsert(VectorId::from(2u64), Vector::new(vec![0.0, 1.0, 0.0])?, None)?;

    let hits = db.search(&Vector::new(vec![1.0, 0.0, 0.0])?, 1)?;
    assert_eq!(hits[0].id, VectorId::from(1u64));
    db.close()
}
```

## Error Handling

Every fallible call returns [`Result<T>`](#error--result) whose error is the single [`Error`](#error--result) enum. It wraps the family's two error vocabularies plus a handle-level configuration variant. The enum is `#[non_exhaustive]`; a `match` must include a wildcard `_` arm.

```rust
use iqdb::{DistanceMetric, Error, Iqdb, Vector, VectorId};

# fn main() {
let db = Iqdb::open_in_memory(3, DistanceMetric::Cosine).unwrap();
// Wrong dimensionality is rejected at the boundary.
let err = db
    .upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0]).unwrap(), None)
    .unwrap_err();
match err {
    Error::Index(_) => { /* a vocabulary / index failure */ }
    Error::Persist(_) => { /* a durable-storage failure */ }
    Error::Config(_) => { /* a handle-level consistency failure */ }
    _ => {}
}
# }
```

## Public APIs

### `Iqdb`

The database handle. A database fixes its dimensionality and distance metric at open time and is `Send + Sync` (share it behind an `Arc<Iqdb>`). Concurrent searches share a read guard; writes and the lazy IVF build take the write guard. Lock poisoning is recovered rather than propagated as a panic.

#### Constructors

| Method | Signature | Description |
|--------|-----------|-------------|
| `open_in_memory` | `(dim: usize, metric: DistanceMetric) -> Result<Iqdb>` | Ephemeral, exact-flat database. Errors on `dim == 0`. |
| `open_in_memory_with` | `(config: IqdbConfig) -> Result<Iqdb>` | In-memory database from a full [`IqdbConfig`](#iqdbconfig) (index kind + optional cache). |
| `open` | `<P: AsRef<Path>>(path: P, dim: usize, metric: DistanceMetric) -> Result<Iqdb>` | Durable, file-backed database. Creates the file if absent, loads it if present. |
| `open_with` | `<P: AsRef<Path>>(path: P, config: IqdbConfig) -> Result<Iqdb>` | Durable database from a full [`IqdbConfig`](#iqdbconfig). |

`dim` is the dimensionality every vector must have; `metric` is the distance used by every search. On a reopen, the stored `dim` / `metric` must match the requested values or the call fails with `Error::Config`.

```rust
use iqdb::{DistanceMetric, HnswConfig, IndexKind, Iqdb, IqdbConfig};

# fn main() -> iqdb::Result<()> {
// Tier 1: exact flat, in memory.
let flat = Iqdb::open_in_memory(64, DistanceMetric::Cosine)?;

// Tier 2: an HNSW graph, configured.
let cfg = IqdbConfig::new(64, DistanceMetric::Cosine)
    .index(IndexKind::Hnsw(HnswConfig::default().with_ef_search(96)));
let hnsw = Iqdb::open_in_memory_with(cfg)?;

assert!(flat.is_empty() && hnsw.is_empty());
# Ok(())
# }
```

#### Records

| Method | Signature | Description |
|--------|-----------|-------------|
| `upsert` | `(id: VectorId, vector: Vector, metadata: Option<Metadata>) -> Result<()>` | Insert or replace the record under `id`. Replacing keeps the count unchanged. Rejects a wrong-dimension vector. |
| `get` | `(id: &VectorId) -> Result<Option<(Vector, Option<Metadata>)>>` | The stored vector and metadata, or `None` if absent. |
| `delete` | `(id: &VectorId) -> Result<bool>` | Remove by id; `true` if it was present (idempotent). |
| `len` | `() -> usize` | Number of stored vectors. |
| `is_empty` | `() -> bool` | `true` if empty. |

```rust
use iqdb::{DistanceMetric, Iqdb, Metadata, Value, Vector, VectorId};

# fn main() -> iqdb::Result<()> {
let db = Iqdb::open_in_memory(2, DistanceMetric::Cosine)?;

let meta: Metadata = [("kind".to_string(), Value::String("doc".into()))]
    .into_iter().collect();
db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0])?, Some(meta))?;

// Replacing the same id keeps len at 1.
db.upsert(VectorId::from(1u64), Vector::new(vec![0.0, 1.0])?, None)?;
assert_eq!(db.len(), 1);

let (vector, _meta) = db.get(&VectorId::from(1u64))?.expect("present");
assert_eq!(vector.as_slice(), &[0.0, 1.0]);

assert!(db.delete(&VectorId::from(1u64))?);
assert!(!db.delete(&VectorId::from(1u64))?);
# Ok(())
# }
```

#### Search

| Method | Signature | Description |
|--------|-----------|-------------|
| `search` | `(query: &Vector, k: usize) -> Result<Vec<Hit>>` | Top-`k`, nearest-first. |
| `search_with` | `(query: &Vector, k: usize, filter: Filter) -> Result<Vec<Hit>>` | Top-`k` restricted to records matching `filter`. |
| `search_batch` | `(queries: &[Vector], k: usize) -> Result<Vec<Vec<Hit>>>` | One result list per query, in order. |
| `search_batch_with` | `(queries: &[Vector], k: usize, filter: Filter) -> Result<Vec<Vec<Hit>>>` | Batch with a shared filter. |

Results sort by `Hit::distance` ascending (smaller is nearer). On the exact flat index the filter is applied before scoring; on HNSW / IVF it is a post-filter, so a highly selective filter can return fewer than `k` hits.

```rust
use iqdb::{DistanceMetric, Filter, Iqdb, Metadata, Value, Vector, VectorId};

# fn main() -> iqdb::Result<()> {
let db = Iqdb::open_in_memory(2, DistanceMetric::Cosine)?;
let doc: Metadata = [("kind".to_string(), Value::String("doc".into()))].into_iter().collect();
let img: Metadata = [("kind".to_string(), Value::String("image".into()))].into_iter().collect();
db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0])?, Some(doc))?;
db.upsert(VectorId::from(2u64), Vector::new(vec![0.99, 0.10])?, Some(img))?;

// Unfiltered top-2.
let all = db.search(&Vector::new(vec![1.0, 0.0])?, 2)?;
assert_eq!(all.len(), 2);

// Documents only.
let docs = db.search_with(
    &Vector::new(vec![1.0, 0.0])?,
    5,
    Filter::eq("kind", Value::String("doc".into())),
)?;
assert_eq!(docs.len(), 1);
assert_eq!(docs[0].id, VectorId::from(1u64));
# Ok(())
# }
```

#### Maintenance & lifecycle

| Method | Signature | Description |
|--------|-----------|-------------|
| `optimize` | `() -> Result<()>` | Rebuild / retrain the approximate index over the current vectors (IVF centroids). No-op for flat / HNSW. |
| `cache_stats` | `() -> Option<CacheStats>` | Cache hit/miss statistics, or `None` when uncached. |
| `flush` | `() -> Result<()>` | Compact a file-backed store (fold the WAL into a fresh snapshot); no-op in memory. |
| `close` | `(self) -> Result<()>` | Final compaction, then release the handle. |
| `dim` | `() -> usize` | The fixed dimensionality. |
| `metric` | `() -> DistanceMetric` | The fixed distance metric. |

```rust,no_run
use iqdb::{DistanceMetric, Iqdb, Vector, VectorId};

# fn main() -> iqdb::Result<()> {
let db = Iqdb::open("./data/vectors.iqdb", 3, DistanceMetric::Cosine)?;
db.upsert(VectorId::from(1u64), Vector::new(vec![0.1, 0.2, 0.3])?, None)?;
db.flush()?;
db.close()
# }
```

### `AsyncIqdb`

*Available with the `async` feature.* A Tokio adapter over [`Iqdb`](#iqdb). It holds an `Arc<Iqdb>` and runs each blocking operation on Tokio's blocking pool via `tokio::task::spawn_blocking`, so awaiting a search or a write never stalls the executor. The family is synchronous by design — a search is CPU-bound, a durable write is a blocking `fsync` — so `AsyncIqdb` is a thin adapter, not a re-implementation; the sync `Iqdb` remains the source of truth. It is `Clone` (shares the handle through the `Arc`), `Send`, and `Sync`. A panic in a blocking closure is re-raised on the awaiting task.

The async methods mirror the sync surface; search and batch methods take their query **by value** (the work runs on another thread). Cheap accessors stay synchronous.

| Method | Signature | Description |
|--------|-----------|-------------|
| `open_in_memory` | `async (dim, metric) -> Result<AsyncIqdb>` | In-memory, exact flat. |
| `open_in_memory_with` | `async (config: IqdbConfig) -> Result<AsyncIqdb>` | In-memory from a config. |
| `open` | `async <P: AsRef<Path>>(path, dim, metric) -> Result<AsyncIqdb>` | Durable; open runs on the blocking pool. |
| `open_with` | `async <P: AsRef<Path>>(path, config) -> Result<AsyncIqdb>` | Durable from a config. |
| `upsert` | `async (id: VectorId, vector: Vector, metadata: Option<Metadata>) -> Result<()>` | Insert or replace. |
| `get` | `async (id: VectorId) -> Result<Option<(Vector, Option<Metadata>)>>` | Look up by id (taken by value). |
| `delete` | `async (id: VectorId) -> Result<bool>` | Remove by id. |
| `search` | `async (query: Vector, k: usize) -> Result<Vec<Hit>>` | Top-`k`. |
| `search_with` | `async (query: Vector, k: usize, filter: Filter) -> Result<Vec<Hit>>` | Filtered top-`k`. |
| `search_batch` / `search_batch_with` | `async (queries: Vec<Vector>, k, [filter]) -> Result<Vec<Vec<Hit>>>` | Batch variants. |
| `optimize` | `async () -> Result<()>` | Rebuild / retrain the approximate index. |
| `flush` | `async () -> Result<()>` | Compact a file-backed store. |
| `close` | `async (self) -> Result<()>` | Final compaction, then release. |
| `len` / `is_empty` / `dim` / `metric` / `cache_stats` | sync `(&self)` | Cheap accessors — no offload. |

```rust
# tokio::runtime::Runtime::new().unwrap().block_on(async {
use iqdb::{AsyncIqdb, DistanceMetric, Vector, VectorId};

let db = AsyncIqdb::open_in_memory(3, DistanceMetric::Cosine).await?;
db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0, 0.0])?, None).await?;

let hits = db.search(Vector::new(vec![1.0, 0.0, 0.0])?, 1).await?;
assert_eq!(hits[0].id, VectorId::from(1u64));
db.close().await?;
# Ok::<(), iqdb::Error>(())
# }).unwrap();
```

### `IqdbConfig`

The fluent, construction-time configuration consumed by `open_in_memory_with` / `open_with`. Small by design: dimensionality, metric, an [`IndexKind`](#indexkind), and an optional [`CacheConfig`](#cacheconfig--cachestats). Per-index tuning lives inside the `IndexKind` variants.

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `(dim: usize, metric: DistanceMetric) -> IqdbConfig` | Start a config; defaults to `IndexKind::Flat`, no cache. |
| `index` | `(self, kind: IndexKind) -> IqdbConfig` | Select the index implementation. |
| `cache` | `(self, cache: CacheConfig) -> IqdbConfig` | Attach a result cache. |
| `fsync` | `(self, policy: FsyncPolicy) -> IqdbConfig` | WAL fsync cadence for the file-backed path (`Always` default / `Periodic` / `Never`). Ignored in memory. |
| `compression` | `(self, compression: Compression) -> IqdbConfig` | Snapshot compression (`None` default / `Zstd { level }` / `Lz4`); the latter two require the `zstd` / `lz4` feature. Ignored in memory. |
| `dim` | `(&self) -> usize` | The configured dimensionality. |
| `metric` | `(&self) -> DistanceMetric` | The configured metric. |
| `index_kind` | `(&self) -> IndexKind` | The configured index kind. |
| `is_cached` | `(&self) -> bool` | Whether a cache is configured. |

```rust
use iqdb::{CacheConfig, Compression, DistanceMetric, FsyncPolicy, IndexKind, IqdbConfig, IvfConfig};

let cfg = IqdbConfig::new(128, DistanceMetric::Euclidean)
    .index(IndexKind::Ivf(IvfConfig::default().with_n_probes(16)))
    .cache(CacheConfig::new().capacity(10_000))
    .fsync(FsyncPolicy::Always)
    .compression(Compression::None);

assert_eq!(cfg.dim(), 128);
assert!(cfg.is_cached());
assert!(matches!(cfg.index_kind(), IndexKind::Ivf(_)));
```

`FsyncPolicy` and `Compression` are re-exported from [`iqdb-persist`](https://docs.rs/iqdb-persist); they tune only the durable, file-backed path and are ignored by an in-memory database.

### `IndexKind`

Selects which index backs the database. `Copy`, `#[derive(Default)]` (defaults to `Flat`).

| Variant | Payload | Description |
|---------|---------|-------------|
| `Flat` | — | Exact brute-force search; the recall ground truth. No tuning. |
| `Hnsw` | `HnswConfig` | Hierarchical Navigable Small World graph (approximate). |
| `Ivf` | `IvfConfig` | Inverted-file clustered index (approximate), IVF-Flat or IVF-PQ. |

```rust
use iqdb::{IndexKind, IvfConfig};

let exact = IndexKind::default();
assert!(matches!(exact, IndexKind::Flat));

let ivf = IndexKind::Ivf(IvfConfig::default().with_n_clusters(256));
assert!(matches!(ivf, IndexKind::Ivf(_)));
```

### `HnswConfig` / `IvfConfig`

Re-exported tuning structs for the approximate indices (see [`iqdb-hnsw`](https://docs.rs/iqdb-hnsw) and [`iqdb-ivf`](https://docs.rs/iqdb-ivf) for the full reference). Both follow the `with_*` builder idiom.

`HnswConfig` knobs: `m` (graph degree), `ef_construction` (build beam), `ef_search` (query beam), `filter_widen` (beam multiplier under a filter), `seed`.

`IvfConfig` knobs: `n_clusters`, `n_probes` (clusters visited per query), `training_sample_size`, `use_pq` / `pq_subvectors` / `pq_refine_factor` (IVF-PQ), `seed`.

```rust
use iqdb::{HnswConfig, IvfConfig};

let hnsw = HnswConfig::default().with_m(32).with_ef_search(128);
assert_eq!(hnsw.m, 32);

let ivf = IvfConfig::default().with_n_clusters(64).with_n_probes(8);
assert_eq!(ivf.n_probes, 8);
```

IVF is trained lazily from the stored vectors on the first search; `Iqdb::optimize` retrains it. IVF-PQ (`use_pq = true`) supports only `Euclidean`, `DotProduct`, and `Manhattan`; constructing it under `Cosine` or `Hamming` fails with `Error::Index`.

### `CacheConfig` / `CacheStats`

Re-exported from [`iqdb-cache`](https://docs.rs/iqdb-cache). `CacheConfig` sizes a result cache (`capacity`, `policy`, `ttl`); `CacheStats` reports `hits`, `misses`, `evictions`, `len`, `capacity`, with `hit_rate()` and `lookups()` helpers. The cache invalidates on every mutation, so it never serves a stale result.

```rust
use iqdb::{CacheConfig, DistanceMetric, Iqdb, IqdbConfig, Vector, VectorId};

# fn main() -> iqdb::Result<()> {
let db = Iqdb::open_in_memory_with(
    IqdbConfig::new(2, DistanceMetric::Cosine).cache(CacheConfig::new().capacity(1024)),
)?;
db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0])?, None)?;

let q = Vector::new(vec![1.0, 0.0])?;
let _ = db.search(&q, 1)?;
let _ = db.search(&q, 1)?; // served from cache
assert_eq!(db.cache_stats().expect("cached").hits, 1);
# Ok(())
# }
```

### `Vector`

An owned, validated, immutable dense embedding (re-exported from [`iqdb-types`](https://docs.rs/iqdb-types)). `Vector::new(Vec<f32>)` rejects empty and non-finite input; `as_slice`, `dim`, and `into_inner` are non-allocating accessors.

```rust
use iqdb::Vector;

let v = Vector::new(vec![0.1, 0.2, 0.3])?;
assert_eq!(v.dim(), 3);
assert_eq!(v.as_slice(), &[0.1, 0.2, 0.3]);

// Empty and non-finite input is rejected.
assert!(Vector::new(vec![]).is_err());
assert!(Vector::new(vec![f32::NAN]).is_err());
# Ok::<(), iqdb::Error>(())
```

### `VectorId`

A stable identifier (re-exported from [`iqdb-types`](https://docs.rs/iqdb-types)): either a compact `U64` or an opaque non-empty `Bytes` key.

```rust
use iqdb::VectorId;

let numeric = VectorId::from(7u64);
assert_eq!(numeric, VectorId::U64(7));

let key = VectorId::try_from(vec![0xde, 0xad]).expect("non-empty");
assert_eq!(key.to_string(), "dead");
# Ok::<(), iqdb::Error>(())
```

### `Metadata` / `Value`

`Metadata` is an immutable, ordered map of string keys to scalar `Value`s (re-exported from [`iqdb-types`](https://docs.rs/iqdb-types)). `Value` is a closed set of scalars: `String`, `Int(i64)`, `Float(f64)`, `Bool`, `Null`. Build `Metadata` from an iterator or a `BTreeMap`.

```rust
use iqdb::{Metadata, Value};

let meta: Metadata = [
    ("title".to_string(), Value::String("intro".into())),
    ("year".to_string(), Value::Int(2026)),
]
.into_iter()
.collect();

assert_eq!(meta.get("year"), Some(&Value::Int(2026)));
assert_eq!(meta.get("missing"), None);
```

### `Filter`

A declarative metadata predicate (re-exported from [`iqdb-types`](https://docs.rs/iqdb-types)) passed to `search_with`. Leaf comparisons (`eq`, `neq`, `lt`, `lte`, `gt`, `gte`, `is_in`) compose with `and`, `or`, and `not`. Absent fields and type mismatches evaluate to `false` (closed-world).

```rust
use iqdb::{Filter, Value};

let published_recent = Filter::and(vec![
    Filter::eq("published", Value::Bool(true)),
    Filter::gte("year", Value::Int(2026)),
]);
let _ = published_recent;
```

### `DistanceMetric`

The distance used by a database (re-exported from [`iqdb-types`](https://docs.rs/iqdb-types), `#[non_exhaustive]`): `Cosine`, `DotProduct`, `Euclidean`, `Manhattan`, `Hamming`. All five sort smaller-is-nearer; `Hit::distance` for `DotProduct` is negated so one ordering rule holds everywhere.

```rust
use iqdb::DistanceMetric;

let metric = DistanceMetric::Cosine;
assert_eq!(metric, DistanceMetric::Cosine);
```

### `Hit`

A search result (re-exported from [`iqdb-types`](https://docs.rs/iqdb-types)): `id: VectorId`, `distance: f32` (smaller is nearer), `metadata: Option<Metadata>`.

```rust
use iqdb::{DistanceMetric, Iqdb, Vector, VectorId};

# fn main() -> iqdb::Result<()> {
let db = Iqdb::open_in_memory(2, DistanceMetric::Euclidean)?;
db.upsert(VectorId::from(1u64), Vector::new(vec![0.0, 0.0])?, None)?;
let hit = &db.search(&Vector::new(vec![0.1, 0.1])?, 1)?[0];
assert_eq!(hit.id, VectorId::from(1u64));
assert!(hit.distance >= 0.0);
# Ok(())
# }
```

### `Error` / `Result`

`Error` is the single `#[non_exhaustive]` error type; `Result<T>` aliases `core::result::Result<T, Error>`. Every variant's `Display` carries only static text or library-produced values, so it is safe to log.

| Variant | Meaning |
|---------|---------|
| `Error::Index(IqdbError)` | An index / vocabulary failure — dimension mismatch, absent id, invalid metric for the chosen index, malformed filter. |
| `Error::Persist(PersistError)` | A durable-storage failure — snapshot / WAL I/O, a corrupt or truncated file, a checksum mismatch, an unsupported compression feature. |
| `Error::Config(&'static str)` | A handle-level consistency failure — most often a reopen whose `dim` / `metric` does not match the stored database. |

```rust
use iqdb::{DistanceMetric, Error, Iqdb, Vector, VectorId};

# fn main() {
let db = Iqdb::open_in_memory(3, DistanceMetric::Cosine).unwrap();
let err = db
    .upsert(VectorId::from(1u64), Vector::new(vec![1.0]).unwrap(), None)
    .unwrap_err();
assert!(matches!(err, Error::Index(_)));
# }
```

## Feature Flags

All feature flags are additive.

| Feature | Default | Description |
|---------|---------|-------------|
| `serde` | off | Derive `Serialize` / `Deserialize` on the public data types (forwards to the family `serde` features). |
| `parallel` | off | Rayon-backed parallel distance scan on the flat index (forwards to `iqdb-flat`). |
| `zstd` | off | Zstandard snapshot compression (forwards to `iqdb-persist`). |
| `lz4` | off | LZ4 snapshot compression (forwards to `iqdb-persist`). |
| `async` | off | Tokio-driven [`AsyncIqdb`](#asynciqdb) mirror of the public API; pulls `tokio` (only the `rt` feature). |

## Behavioural Notes

- **Construction-time shape.** `dim` and `metric` are fixed at open. A wrong-dimension vector is rejected at `upsert`; a reopen with a mismatched `dim` / `metric` fails with `Error::Config`.
- **Authoritative store.** iqdb owns the vectors in an internal row store; the search index (flat / HNSW / IVF) is a derived structure rebuilt from that store on load. `len()` always reflects the authoritative store.
- **IVF lifecycle.** IVF is trained lazily from the stored vectors on the first search after a write, and rebuilt by `Iqdb::optimize`. Flat and HNSW accept incremental inserts directly, so search-after-write is immediate for them.
- **Approximate filters.** HNSW / IVF apply a metadata `Filter` after traversal; a highly selective filter can return fewer than `k` hits. The flat index applies the filter exactly before scoring. Widen HNSW `filter_widen` or IVF `n_probes` to compensate.
- **Concurrency.** `Iqdb` is `Send + Sync`. Reads share a `RwLock` read guard; writes and the lazy IVF build take the write guard. Lock poisoning is recovered, never propagated as a panic.
- **Durability.** The default file-backed write policy `fsync`s the WAL on every acknowledged write, so a clean process exit (or even a crash) replays to the last acknowledged state on reopen. `flush` / `close` compact the WAL into a snapshot.
- **Cross-platform format.** The on-disk payload is little-endian on every platform; a database written on x86_64 reads back identically on aarch64.
- **Non-exhaustive types.** `Error` and `DistanceMetric` are `#[non_exhaustive]`. Always include a wildcard `_` arm in a `match`.

<br>

<div align="center">
    <sup>
        <a href="../README.md" title="Project Home"><b>HOME</b></a>
        <span>&nbsp;│&nbsp;</span>
        <span>API</span>
        <span>&nbsp;│&nbsp;</span>
        <a href="../CHANGELOG.md" title="Changelog"><b>CHANGELOG</b></a>
        <span>&nbsp;│&nbsp;</span>
        <a href="../REPS.md" title="Standards"><b>STANDARDS</b></a>
    </sup>
</div>
