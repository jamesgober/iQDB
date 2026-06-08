<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <strong>iQDB</strong>
    <br>
    <sup><sub>EMBEDDED VECTOR DATABASE FOR RUST</sub></sup>
</h1>

<p align="center">
    <a href="https://crates.io/crates/iqdb"><img alt="crates.io" src="https://img.shields.io/crates/v/iqdb.svg"></a>
    <a href="https://crates.io/crates/iqdb" alt="Download iqdb"><img alt="Crates.io Downloads" src="https://img.shields.io/crates/d/iqdb?color=%230099ff"></a>
    <a href="https://docs.rs/iqdb"><img alt="docs.rs" src="https://docs.rs/iqdb/badge.svg"></a>
    <img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.87%2B-blue.svg?style=flat-square" title="Rust Version">
    <a href="https://github.com/jamesgober/iqdb/actions"><img alt="CI" src="https://github.com/jamesgober/iqdb/actions/workflows/ci.yml/badge.svg"></a>
</p>

<p align="center">
    FASTER - LIGHTER - SMARTER 
    <br>
    Intelligence-grade vector storage for AI-native applications.
    <br>
    Sub-millisecond vector search. Zero network hops. - i<b>QD</b>B
</p>

<br>

<div align="left">
    <p>
        <strong>iQDB</strong> is an <b>embedded vector database</b> for Rust, a single-process, in-application similarity-search engine designed for <em>high-dimensional</em> workloads where every microsecond on the query path matters.
        It targets the same operational shape as <code>sqlite</code> or <code>redb</code>: no daemon, no network hop, no separate runtime. Open a handle, write vectors, query nearest neighbours — all from inside your binary.
    </p>
    <p>
        The engine is built against a <b>lock-free hot path</b>, <b>allocation-free steady state</b>, and a <b>cache-aware</b> on-disk layout. Exact brute-force search, approximate-nearest-neighbour indices (HNSW and IVF), payload metadata, declarative filters, and durable write-ahead-logged storage are all in scope. Indices and storage are pluggable, so workloads can trade recall for latency without rewriting the surrounding application.
    </p>
    <p>
        Built for <b>cross-platform</b> deployment from day one — Linux, macOS, and Windows are first-class targets, with the strongest power-loss sync each platform offers and atomic file replacement everywhere.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.87+.</strong> The crate is dual-licensed under <code>Apache-2.0 OR MIT</code> at your option.
    </p>
    <blockquote>
        <strong>0.5.0 re-platforms iQDB onto the iqdb crate family.</strong> The crate is now the integration layer that composes the family's shared vocabulary (<code>iqdb-types</code>), index seam (<code>iqdb-index</code>), exact and approximate indices (<code>iqdb-flat</code>, <code>iqdb-hnsw</code>, <code>iqdb-ivf</code>), durable storage (<code>iqdb-persist</code>), and an optional result cache (<code>iqdb-cache</code>). A database now fixes its dimensionality and distance metric at open time and routes searches through a selectable index — exact <code>Flat</code> by default, or approximate <code>Hnsw</code> / <code>Ivf</code> through <code>IqdbConfig</code>. This is a <b>breaking change</b> from the 0.4.x self-contained surface; the API is unstable until 1.0. <strong>0.6.0</strong> adds an opt-in async surface (<code>AsyncIqdb</code>) behind the <code>async</code> feature; the synchronous API is unchanged. See <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a> for the release-by-release surface and <a href="./docs/API.md"><code>docs/API.md</code></a> for the full reference.
    </blockquote>
</div>

<hr>
<br>

## Design Goals

iQDB is engineered against the <a href="./REPS.md">Rust Efficiency &amp; Performance Standards</a> (REPS). Every architectural decision is graded against the same hard constraints:

- **Embedded by default** — no daemon, no socket, no separate process. The library opens a path and gets out of the way.
- **Sub-millisecond queries** — exact and approximate search paths are budgeted in microseconds, not milliseconds, with the hot path measured under Criterion every release.
- **Enum-dispatched hot paths** — the index seam dispatches through a closed `match`, never `dyn`, so the query loop sees a concrete index with no virtual indirection.
- **Allocation-aware steady state** — vector payloads are shared as `Arc<[f32]>` between the authoritative store and the derived index, so a vector that lives in both costs one allocation, not two.
- **Pluggable indices** — flat, IVF, and HNSW share the `iqdb-index` trait surface so callers swap strategies through `IqdbConfig` without touching their query code. Flat is the exact recall ground truth the approximate indices are measured against.
- **Crash-safe writes** — the durable path uses write-ahead logging and atomic snapshot replacement. A pulled power cord must never corrupt the database.
- **Tier-1 cross-platform** — Linux (x86_64, aarch64), macOS (x86_64, Apple Silicon), Windows (x86_64) all compile and pass the full test suite on every commit.

<hr>
<br>

## Status &amp; Roadmap

iQDB ships milestone-by-milestone. Each tag below corresponds to a published release; everything above the current line is shipped, everything below is planned.

| Milestone | Status | Surface delivered |
|-----------|--------|-------------------|
| `v0.1.0` — scaffolding | shipped | Crate scaffolding, lifecycle handle, `Error` type, CI matrix on all three Tier-1 platforms. |
| `v0.2.0` — vector primitives | shipped | Validated vectors, distance metrics, typed payloads, in-memory store with thread-safe CRUD. |
| `v0.3.0` — search | shipped | Flat top-`k` search, filters, batch variants, NaN-aware ranking, property-based tests. |
| `v0.4.0` — durable storage | shipped | Directory-backed store, snapshot + WAL, cross-platform sync, atomic compaction, corrupt-tail recovery. |
| `v0.5.0` — family composition + approximate indices | shipped | Re-platformed onto the iqdb crate family. Re-exported vocabulary (`Vector`, `VectorId`, `Metadata`, `Value`, `Hit`, `Filter`, `DistanceMetric`). Selectable index — exact `Flat`, plus `Hnsw` and `Ivf` through `IqdbConfig`. Durable storage via `iqdb-persist`; optional result cache via `iqdb-cache`. Recall validated against the flat oracle. |
| `v0.6.0` — async surface | shipped | `async`-feature-gated `AsyncIqdb`: a Tokio adapter that offloads each blocking call via `spawn_blocking`. Additive; the synchronous API and default build are unchanged. |
| `v0.7.0` — durability tuning (alpha) | **current** | `IqdbConfig::fsync` (WAL fsync cadence) and `IqdbConfig::compression` (snapshot `zstd` / `lz4`), wiring the compression features through. Additive; defaults unchanged. |
| `v0.8.x` — beta · `v0.9.x` — RC | planned | Broader testing, final benchmarks, doc polish. |
| `v1.0.0` — API freeze | planned | Frozen public API and on-disk format. SemVer guarantees. Full benchmark suite. |

The per-release detail — what was added, what changed, and what was verified — lives in the [`CHANGELOG`](./CHANGELOG.md) and the per-version notes under [`docs/release/`](./docs/release/).

<hr>
<br>

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
iqdb = "0.5"
```

Optional features (all additive):

```toml
[dependencies]
iqdb = { version = "0.5", features = ["serde", "parallel", "zstd"] }
```

iQDB compiles on stable Rust **1.87** and newer. As of 0.5.0 it composes the published iqdb family crates (`iqdb-types`, `iqdb-index`, `iqdb-flat`, `iqdb-hnsw`, `iqdb-ivf`, `iqdb-build`, `iqdb-persist`, `iqdb-cache`, and their transitive dependencies); it is no longer a zero-dependency build.

<hr>
<br>

## Quick Start

A database fixes its dimensionality and distance metric at open time, then exposes a small surface: `upsert` / `get` / `delete` for records, `search` / `search_with` for queries.

```rust
use iqdb::{DistanceMetric, Iqdb, Result, Vector, VectorId};

fn main() -> Result<()> {
    // A 3-dimensional, in-memory database compared under cosine distance.
    let db = Iqdb::open_in_memory(3, DistanceMetric::Cosine)?;

    db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0, 0.0])?, None)?;
    db.upsert(VectorId::from(2u64), Vector::new(vec![0.99, 0.10, 0.0])?, None)?;

    // Top-k similarity search. Results are sorted nearest-first under the
    // smaller-is-closer rule; ties break on insertion order for determinism.
    let hits = db.search(&Vector::new(vec![1.0, 0.0, 0.0])?, 5)?;
    assert_eq!(hits[0].id, VectorId::from(1u64));

    db.close()
}
```

### Filtered and batch search

Filters are declarative [`Filter`] expressions evaluated against each record's [`Metadata`]. On the exact flat index the filter is applied before scoring, so the result is exact.

```rust
use iqdb::{DistanceMetric, Filter, Iqdb, Metadata, Result, Value, Vector, VectorId};

fn main() -> Result<()> {
    let db = Iqdb::open_in_memory(2, DistanceMetric::Cosine)?;

    let doc: Metadata = [("kind".to_string(), Value::String("doc".into()))]
        .into_iter().collect();
    let img: Metadata = [("kind".to_string(), Value::String("image".into()))]
        .into_iter().collect();
    db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0])?, Some(doc))?;
    db.upsert(VectorId::from(2u64), Vector::new(vec![0.99, 0.10])?, Some(img))?;

    // Only documents, ranked by cosine distance.
    let filter = Filter::eq("kind", Value::String("doc".into()));
    let hits = db.search_with(&Vector::new(vec![1.0, 0.0])?, 5, filter)?;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, VectorId::from(1u64));

    // Batch — one top-k result list per query, preserving input order.
    let queries = vec![Vector::new(vec![1.0, 0.0])?, Vector::new(vec![0.0, 1.0])?];
    let batches = db.search_batch(&queries, 1)?;
    assert_eq!(batches.len(), 2);

    Ok(())
}
```

### Choosing an index

Tier 1 (`open_in_memory` / `open`) defaults to the exact flat index. Tier 2 (`open_in_memory_with` / `open_with`) takes an [`IqdbConfig`] that selects an approximate index and tunes it, and can attach a result cache.

```rust
use iqdb::{CacheConfig, DistanceMetric, HnswConfig, IndexKind, Iqdb, IqdbConfig, Result};

fn main() -> Result<()> {
    let cfg = IqdbConfig::new(128, DistanceMetric::Cosine)
        .index(IndexKind::Hnsw(HnswConfig::default().with_ef_search(96)))
        .cache(CacheConfig::new().capacity(10_000));
    let db = Iqdb::open_in_memory_with(cfg)?;
    assert!(db.is_empty());
    Ok(())
}
```

On the approximate indices the metadata filter is applied after graph / cluster traversal, so a highly selective filter can return fewer than `k` hits — widen the search (HNSW `filter_widen`, IVF `n_probes`) when that matters. IVF is trained lazily from the stored vectors on the first search; after many writes, `Iqdb::optimize` retrains its centroids.

### Durable, file-backed storage

`Iqdb::open(path, dim, metric)` opens or creates a durable database. The path is the snapshot file; a write-ahead log lives beside it. Acknowledged writes survive a crash; reopening replays the log onto the snapshot.

```rust,no_run
use iqdb::{DistanceMetric, Iqdb, Result, Vector, VectorId};

fn main() -> Result<()> {
    let db = Iqdb::open("./data/vectors.iqdb", 3, DistanceMetric::Cosine)?;
    db.upsert(VectorId::from(1u64), Vector::new(vec![0.1, 0.2, 0.3])?, None)?;
    db.flush()?; // compact: fold the WAL into a fresh snapshot
    db.close()
}
```

A reopen whose requested `dim` / `metric` disagrees with the stored database fails with `Error::Config`. The stored index kind is part of the database identity and is restored from the snapshot regardless of the kind requested on reopen.

By default every acknowledged write is `fsync`ed and the snapshot is uncompressed. Trade durability for throughput, or shrink the snapshot, through `IqdbConfig`:

```rust,no_run
use iqdb::{Compression, DistanceMetric, FsyncPolicy, Iqdb, IqdbConfig};
use std::time::Duration;

# fn run() -> iqdb::Result<()> {
let cfg = IqdbConfig::new(128, DistanceMetric::Cosine)
    .fsync(FsyncPolicy::Periodic(Duration::from_millis(50))) // bound the un-synced window
    .compression(Compression::Zstd { level: 3 });            // requires the `zstd` feature
let db = Iqdb::open_with("./data/vectors.iqdb", cfg)?;
# let _ = db;
# Ok(())
# }
```

### Async (the `async` feature)

The family is synchronous by design, so the async surface is a thin Tokio adapter: `AsyncIqdb` holds an `Arc<Iqdb>` and runs each blocking call on Tokio's blocking pool via `spawn_blocking`, so awaiting a search or a write never stalls the executor. It is `Clone` + `Send` + `Sync`. Enable the `async` feature and bring your own runtime.

```rust,ignore
use iqdb::{AsyncIqdb, DistanceMetric, Result, Vector, VectorId};

#[tokio::main]
async fn main() -> Result<()> {
    let db = AsyncIqdb::open_in_memory(3, DistanceMetric::Cosine).await?;
    db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0, 0.0])?, None).await?;

    let hits = db.search(Vector::new(vec![1.0, 0.0, 0.0])?, 1).await?;
    assert_eq!(hits[0].id, VectorId::from(1u64));
    db.close().await
}
```

<hr>
<br>

## API Overview

The full API reference lives at [`docs/API.md`](./docs/API.md); the rustdoc at [docs.rs/iqdb](https://docs.rs/iqdb) carries the same information in browseable form. The public surface:

- [`Iqdb`](./src/handle.rs) — the database handle.
  - `Iqdb::open_in_memory(dim, metric)` — an ephemeral, exact-flat database.
  - `Iqdb::open_in_memory_with(config)` — an in-memory database from a full [`IqdbConfig`].
  - `Iqdb::open(path, dim, metric)` / `Iqdb::open_with(path, config)` — a durable, file-backed database.
  - `Iqdb::upsert(id, vector, metadata)` — insert or replace. Rejects a wrong-dimension vector at the boundary.
  - `Iqdb::get(id)` — look up the stored vector and metadata. `Ok(None)` when absent.
  - `Iqdb::delete(id)` — remove by id; returns whether it was present.
  - `Iqdb::len()` / `Iqdb::is_empty()` — cardinality.
  - `Iqdb::search(query, k)` — top-`k` similarity search under the database metric.
  - `Iqdb::search_with(query, k, filter)` — top-`k` restricted by a metadata [`Filter`].
  - `Iqdb::search_batch(...)` / `search_batch_with(...)` — order-preserving batch variants.
  - `Iqdb::optimize()` — rebuild / retrain the approximate index over the current vectors.
  - `Iqdb::cache_stats()` — cache hit/miss statistics, when a cache is configured.
  - `Iqdb::flush()` — compact a file-backed store; no-op in memory.
  - `Iqdb::close(self)` — final compaction, then release.
- [`AsyncIqdb`](./src/async_db.rs) — *(`async` feature)* a Tokio adapter mirroring the `Iqdb` surface; offloads each blocking call via `spawn_blocking`. `Clone` + `Send` + `Sync`.
- [`IqdbConfig`](./src/config.rs) — fluent construction config: `dim`, `metric`, an [`IndexKind`], and an optional [`CacheConfig`].
- [`IndexKind`](./src/config.rs) — `Flat` (exact), `Hnsw(HnswConfig)`, `Ivf(IvfConfig)`.
- [`HnswConfig`] / [`IvfConfig`] / [`CacheConfig`] — re-exported tuning structs for the approximate indices and the cache.
- [`Vector`] / [`VectorId`] / [`Metadata`] / [`Value`] / [`Hit`] / [`Filter`] / [`DistanceMetric`] — the shared vocabulary, re-exported from `iqdb-types`.
- [`Error`](./src/error.rs) / [`Result<T>`](./src/error.rs) — the unified error type (`#[non_exhaustive]`) and its `Result` alias.

[`Filter`]: https://docs.rs/iqdb-types
[`Metadata`]: https://docs.rs/iqdb-types
[`Value`]: https://docs.rs/iqdb-types
[`Vector`]: https://docs.rs/iqdb-types
[`VectorId`]: https://docs.rs/iqdb-types
[`Hit`]: https://docs.rs/iqdb-types
[`DistanceMetric`]: https://docs.rs/iqdb-types
[`HnswConfig`]: https://docs.rs/iqdb-hnsw
[`IvfConfig`]: https://docs.rs/iqdb-ivf
[`CacheConfig`]: https://docs.rs/iqdb-cache
[`IqdbConfig`]: ./src/config.rs
[`IndexKind`]: ./src/config.rs

### Error variants

| Variant | Meaning | Recovery |
|---------|---------|----------|
| `Error::Index(IqdbError)` | A failure from the index / vocabulary layer — dimension mismatch, absent id, invalid metric for the chosen index, malformed filter. | Inspect the wrapped [`iqdb_types::IqdbError`] kind and fix the construction or query site. |
| `Error::Persist(PersistError)` | A failure from the durable-storage layer — snapshot / WAL I/O, a corrupt or truncated file, a checksum mismatch, or an unsupported compression feature. | Inspect the wrapped [`iqdb_persist::PersistError`]. A corrupt WAL tail is truncated automatically; a corrupt snapshot fails the open. |
| `Error::Config(&'static str)` | A handle-level consistency check failed — most often a reopen whose `dim` / `metric` does not match the stored database. | Open with the values the database was created with. |

The enum is `#[non_exhaustive]`; always include a `_` arm in a `match`.

<hr>
<br>

## Examples

Self-contained examples live in [`examples/`](./examples). Run them with `cargo run --example <name>`.

- **`basic`** — open, upsert, get, search, delete. [`examples/basic.rs`](./examples/basic.rs)
- **`in_memory_store`** — metadata, replace-on-upsert, and a metadata-filtered search. [`examples/in_memory_store.rs`](./examples/in_memory_store.rs)
- **`search`** — top-`k`, batch, and the effect of the distance metric. [`examples/search.rs`](./examples/search.rs)
- **`persistence`** — three sessions against one durable file, showing data survives reopen. [`examples/persistence.rs`](./examples/persistence.rs)
- **`index_selection`** — flat vs HNSW vs IVF through `IqdbConfig`, plus a cache and `optimize`. [`examples/index_selection.rs`](./examples/index_selection.rs)
- **`async_search`** *(`async` feature)* — concurrent searches fanned out across Tokio tasks. [`examples/async_search.rs`](./examples/async_search.rs)

```sh
cargo run --example index_selection
cargo run --example async_search --features async
```

<hr>
<br>

## Benchmarks

A Criterion harness lives in [`benches/search.rs`](./benches/search.rs):

- **`flat/search_dim64_n1000_k10`** / **`hnsw/search_dim64_n1000_k10`** — top-`k` query throughput on the exact and approximate paths over 1 000 vectors at dim 64.
- **`flat/upsert_dim64`** — write throughput building a fresh database.

```sh
cargo bench --bench search
```

Criterion writes reports to `target/criterion/`. A regression beyond the REPS threshold (5% on a tracked metric) blocks a release.

<hr>
<br>

## Testing

Every public path has happy / error / edge-case coverage:

- Unit tests live in `#[cfg(test)] mod tests` blocks inside each source file.
- Integration tests live in [`tests/`](./tests):
  - [`tests/persistence.rs`](./tests/persistence.rs) — durable lifecycle: open / upsert / close / reopen, delete and metadata persistence, WAL replay without close, dim/metric-mismatch rejection, IVF round-trip, multi-session accumulation.
  - [`tests/properties.rs`](./tests/properties.rs) — `proptest`-driven invariants: flat ranking (sorted, bounded, unique) and the durable round-trip preserving arbitrary record sets.
  - [`tests/recall.rs`](./tests/recall.rs) — recall@k of HNSW and IVF measured against the exact flat oracle on deterministic synthetic data.
- Doc tests run as part of `cargo test` and validate every `# Examples` block.

```sh
cargo test
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

<hr>
<br>

## Cross-Platform Support

**Tier 1 targets** — every commit is built and tested on:

- Linux (`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`)
- macOS (`x86_64-apple-darwin`, `aarch64-apple-darwin`)
- Windows (`x86_64-pc-windows-msvc`)

Durable storage is provided by `iqdb-persist`, which takes the strongest power-loss sync each platform offers and replaces snapshots atomically. The on-disk format is little-endian on every platform, so a database written on one architecture reads back identically on another.

<hr>
<br>

## Configuration

### Feature flags

Feature flags are strictly additive (per REPS) — enabling any combination never removes or weakens existing functionality.

| Feature    | Default | Description |
|------------|---------|-------------|
| `serde`    | off     | Derive `Serialize` / `Deserialize` on the public data types (forwards to the family `serde` features). |
| `parallel` | off     | Rayon-backed parallel distance scan on the flat index (forwards to `iqdb-flat`). |
| `zstd`     | off     | Zstandard snapshot compression (forwards to `iqdb-persist`). |
| `lz4`      | off     | LZ4 snapshot compression (forwards to `iqdb-persist`). |
| `async`    | off     | Tokio-driven `AsyncIqdb` mirror of the public API. Pulls `tokio` (only the `rt` feature). |

```toml
iqdb = { version = "0.5", features = ["serde"] }
```

### Runtime configuration

`Iqdb::open_in_memory(dim, metric)` and `Iqdb::open(path, dim, metric)` cover the common case. Index selection, tuning, and caching are configured through the fluent [`IqdbConfig`](./src/config.rs) passed to the `_with` constructors.

<hr>
<br>

## Architecture

The crate is the integration layer over the iqdb family; each module owns one concern:

- `src/lib.rs` — crate root, lint profile, vocabulary re-exports.
- `src/handle.rs` — the [`Iqdb`](./src/handle.rs) handle and its `RwLock`-guarded in-memory / file-backed storage seam.
- `src/config.rs` — the fluent [`IqdbConfig`](./src/config.rs) and the [`IndexKind`](./src/config.rs) union.
- `src/error.rs` — the unified [`Error`](./src/error.rs) wrapping the family error vocabularies.
- `src/engine/mod.rs` — `IqdbCore`, the owned engine that implements the `iqdb-index` and `iqdb-persist` traits over an authoritative row store plus a derived index.
- `src/engine/store.rs` — the authoritative, insertion-ordered row store (the single source of truth for `len` and rebuilds).
- `src/engine/index.rs` — `AnyIndex`, the closed enum over `FlatIndex` / `HnswIndex` / `IvfIndex` with the IVF training hooks.
- `src/engine/codec.rs` — the little-endian on-disk payload codec inside the `iqdb-persist` frame.

### Compile-time guarantees

The crate root enables the strict REPS lint profile in [`src/lib.rs`](./src/lib.rs):

```text
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
```

Test modules locally relax the `unwrap_used` / `expect_used` lints — the strict profile is for production library code, not assertion scaffolding inside `#[cfg(test)]` blocks. The crate contains no `unsafe` code.

<hr>
<br>

## Contributing

Pull requests are welcome. Before opening one, please make sure the full CI gate passes locally:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo deny check
cargo audit
```

Every contribution is expected to honour the standards in [`REPS.md`](./REPS.md) — performance, security, error handling, testing, documentation, and dependency hygiene are all enforced as merge gates, not afterthoughts. Commit messages are imperative, lowercase, and scoped to a single logical change.

<hr>
<br>

<div align="center">
    <sup>
        <span>HOME</span>
        <span>&nbsp;│&nbsp;</span>
        <a href="https://github.com/jamesgober/iqdb/blob/main/CHANGELOG.md" title="Changelog"><b>CHANGELOG</b></a>
        <span>&nbsp;│&nbsp;</span>
        <a href="https://github.com/jamesgober/iqdb/blob/main/REPS.md" title="Standards"><b>STANDARDS</b></a>
        <span>&nbsp;│&nbsp;</span>
        <a href="https://docs.rs/iqdb" title="API Reference"><b>DOCS</b></a>
    </sup>
</div>
<br>

## Links

- [Documentation (docs.rs)](https://docs.rs/iqdb)
- [Crates.io](https://crates.io/crates/iqdb)
- [Repository](https://github.com/jamesgober/iqdb)
- [Issues](https://github.com/jamesgober/iqdb/issues)
- [Changelog](./CHANGELOG.md)
- [Standards (REPS)](./REPS.md)

<br>

<hr>
<br>

<!-- LICENSE
############################################ -->
<div id="license">
    <h2>LICENSE</h2>
    <p>Licensed under either of</p>
    <b>Apache License, Version 2.0</b>: <a href="./LICENSE-APACHE">LICENSE-APACHE</a>
      &mdash; <a href="http://www.apache.org/licenses/LICENSE-2.0" target="_blank">http://www.apache.org/licenses/LICENSE-2.0</a>
    <br><br>
    <b>MIT License</b>: <a href="./LICENSE-MIT">LICENSE-MIT</a> &mdash;
    <a href="http://opensource.org/licenses/MIT" target="_blank">http://opensource.org/licenses/MIT</a>
    <br><br>
    <p>At your option. Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.</p>
</div>


<!-- COPYRIGHT
------------------------------>
<div align="center">
    <h2></h2>
    Copyright &copy; 2026 James Gober.
</div>
