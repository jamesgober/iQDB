# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0] — 2026-06-08

v0.7.0 opens the alpha line with durable-storage tuning: callers can now choose the write-ahead-log fsync cadence and snapshot compression through [`IqdbConfig`]. Additive — the default behaviour (fsync every write, no compression) is unchanged.

### Added

- [`IqdbConfig::fsync`](./src/config.rs) takes a [`FsyncPolicy`] — `Always` (default), `Periodic(Duration)`, or `Never` — to trade durability for write throughput on the file-backed path.
- [`IqdbConfig::compression`](./src/config.rs) takes a [`Compression`] — `None` (default), `Zstd { level }`, or `Lz4` — applied to the snapshot. Both re-exported from `iqdb-persist`.
- New durability integration tests at [`tests/persistence.rs`](./tests/persistence.rs): a relaxed-fsync round-trip, a `zstd`-compressed round-trip (under the `zstd` feature), and rejection of a compression scheme whose feature is not compiled in.

### Changed

- Version bumped to 0.7.0. Durability and compression default to the prior behaviour, so existing callers are unaffected. The in-memory backend ignores both knobs.

### Fixed

- The `zstd` and `lz4` cargo features were declared in v0.5.0 but had no runtime effect — the durable open path always used `Compression::None`. They are now reachable through [`IqdbConfig::compression`].

[`FsyncPolicy`]: https://docs.rs/iqdb-persist
[`Compression`]: https://docs.rs/iqdb-persist

## [0.6.0] — 2026-06-08

v0.6.0 adds an opt-in **async surface**. It is purely additive — the default build and the entire synchronous API are unchanged, and no Tokio dependency is pulled unless the `async` feature is enabled.

### Added

- `async` Cargo feature and [`AsyncIqdb`](./src/async_db.rs) — a Tokio adapter over [`Iqdb`](./src/handle.rs). It holds an `Arc<Iqdb>` and runs each blocking operation on Tokio's blocking pool via `tokio::task::spawn_blocking`, so awaiting a search or a write never stalls the executor. The family is synchronous by design (a search is CPU-bound, a durable write is a blocking `fsync`), so this is a thin adapter, not a re-implementation — the sync `Iqdb` remains the source of truth.
  - Async mirror of the public surface: `open_in_memory` / `open_in_memory_with` / `open` / `open_with`, `upsert`, `get`, `delete`, `search`, `search_with`, `search_batch`, `search_batch_with`, `optimize`, `flush`, `close`. Search and batch methods take their query by value (the work runs on another thread).
  - Cheap, non-blocking accessors (`len`, `is_empty`, `dim`, `metric`, `cache_stats`) stay synchronous.
  - `AsyncIqdb` is `Clone` (shares the handle through the `Arc`), `Send`, and `Sync`. A panic in a blocking closure is re-raised on the awaiting task rather than swallowed.
- New `async_search` example (`cargo run --example async_search --features async`) showing concurrent searches fanned out across tasks.
- Async integration tests at [`tests/async_ops.rs`](./tests/async_ops.rs) — durable round-trip, reopen dim-mismatch rejection, and concurrent searches on a multi-thread runtime.

### Changed

- Version bumped to 0.6.0. The synchronous API is unchanged; this release is additive.

## [0.5.0] — 2026-06-08

v0.5.0 re-platforms `iqdb` from a self-contained crate onto the **iqdb crate family**. `iqdb` is now the integration layer that composes the family for its vocabulary, index seam, index implementations, durability, and caching. This is a **breaking change**: the entire public surface moves to the family vocabulary, and a database now fixes its dimensionality and distance metric at open time. The crate is pre-1.0, so the break is permitted under SemVer.

### Added

- The crate now depends on and composes the published 1.0 family: [`iqdb-types`], [`iqdb-distance`], [`iqdb-index`], [`iqdb-filter`], [`iqdb-flat`], [`iqdb-hnsw`], [`iqdb-ivf`], [`iqdb-build`], [`iqdb-persist`], and [`iqdb-cache`].
- The shared vocabulary is re-exported from `iqdb-types`: `Vector`, `VectorId`, `Metadata`, `Value`, `Hit`, `Filter`, `DistanceMetric`, `SearchParams`.
- Selectable index implementation through [`IndexKind`](./src/config.rs): `Flat` (exact, the recall ground truth), `Hnsw(HnswConfig)` (graph ANN), and `Ivf(IvfConfig)` (clustered ANN, IVF-Flat or IVF-PQ). `HnswConfig`, `IvfConfig`, and `CacheConfig` are re-exported for tuning.
- Fluent [`IqdbConfig`](./src/config.rs) (`IqdbConfig::new(dim, metric).index(..).cache(..)`) and the Tier-2 constructors `Iqdb::open_in_memory_with` / `Iqdb::open_with`.
- Durable, file-backed storage via `iqdb-persist`: atomic snapshot + write-ahead log, CRC32-checked frames, crash recovery with corrupt-tail truncation, and optional `zstd` / `lz4` snapshot compression.
- Optional result cache via `iqdb-cache`, configured through `IqdbConfig::cache`; `Iqdb::cache_stats` reports hit/miss counts.
- `Iqdb::optimize` — rebuilds / retrains the approximate index (notably IVF centroids) over the current vectors.
- New `index_selection` example demonstrating flat / HNSW / IVF selection, caching, and `optimize`. The `basic`, `in_memory_store`, `search`, and `persistence` examples are rewritten for the new surface.
- Recall validation at [`tests/recall.rs`](./tests/recall.rs): HNSW and IVF measured against the exact flat oracle on deterministic synthetic data.
- New Criterion bench group at [`benches/search.rs`](./benches/search.rs) — flat and HNSW search throughput plus the write path.
- Feature flags `serde`, `parallel`, `zstd`, and `lz4`, each forwarding to the relevant family crate.

### Changed

- **Edition bumped to 2024** (MSRV unchanged at 1.87).
- **`dim` and `metric` are fixed at construction.** All constructors take `(dim, metric)` (or an `IqdbConfig` carrying them) instead of inferring dimensionality from the first vector.
- **`search` loses its `metric` argument** — `search(&query, k)` and `search_with(&query, k, filter)` use the metric fixed at open. (Breaking.)
- **`upsert` takes `(VectorId, Vector, Option<Metadata>)`** rather than a `Record`. Wrong-dimension vectors are rejected at `upsert`, not at search time. (Breaking.)
- **`get` returns `Option<(Vector, Option<Metadata>)>`** and **`search` returns `Vec<Hit>`** (`Hit { id, distance, metadata }`) instead of `Vec<SearchResult>`. (Breaking.)
- **Filters are declarative `Filter` expressions** evaluated against `Metadata`, replacing the closure predicate. On flat the filter is exact (pre-scan); on HNSW / IVF it is a post-filter that can under-return under high selectivity. (Breaking.)
- **`open(path, dim, metric)` treats `path` as the snapshot file** (the WAL lives beside it), replacing the v0.4.0 directory layout. A reopen whose `dim` / `metric` disagrees with the stored database fails with `Error::Config`. (Breaking.)
- **`open_in_memory` / `open_in_memory_with` return `Result<Self>`** — construction validates `dim` (and the index configuration) and reports `dim == 0` and unsupported IVF-PQ metrics as errors rather than panicking. (Breaking.)
- **`Error` is now a three-variant `#[non_exhaustive]` enum** — `Index(IqdbError)`, `Persist(PersistError)`, `Config(&'static str)` — wrapping the family error vocabularies. (Breaking.)

### Removed

- The self-contained vocabulary — `Record`, `RecordId`, `Payload`, `PayloadValue`, `SearchResult`, and the in-crate `Vector` / `DistanceMetric` / `Error` — is replaced by the re-exported family types. `DistanceMetric` now offers `Cosine`, `DotProduct`, `Euclidean`, `Manhattan`, and `Hamming` (the v0.4.0 `L2` becomes `Euclidean`, `Dot` becomes `DotProduct`). (Breaking.)
- The hand-rolled persistence stack (`codec`, `file_store`, `platform`) and the `Error::NotImplemented` / `Error::Corrupt` variants are gone — durability and corrupt-frame handling now come from `iqdb-persist`. The Unix-only `libc` dependency is dropped.
- The "zero runtime dependencies" property no longer holds: composing the family pulls the family crates and their transitive dependencies.

[`iqdb-types`]: https://crates.io/crates/iqdb-types
[`iqdb-distance`]: https://crates.io/crates/iqdb-distance
[`iqdb-index`]: https://crates.io/crates/iqdb-index
[`iqdb-filter`]: https://crates.io/crates/iqdb-filter
[`iqdb-flat`]: https://crates.io/crates/iqdb-flat
[`iqdb-hnsw`]: https://crates.io/crates/iqdb-hnsw
[`iqdb-ivf`]: https://crates.io/crates/iqdb-ivf
[`iqdb-build`]: https://crates.io/crates/iqdb-build
[`iqdb-persist`]: https://crates.io/crates/iqdb-persist
[`iqdb-cache`]: https://crates.io/crates/iqdb-cache

## [0.4.0] — 2026-05-30

### Added

- [`Iqdb::open(path)`](./src/db.rs) is now **load-bearing**. The path is treated as a directory; iqdb creates `<path>/snap` (most recent durable snapshot) and `<path>/wal` (write-ahead log) and manages both. Path validation rejects existing non-directory paths with `Error::InvalidConfig`. Missing directories are created with `create_dir_all` semantics.
- Crate-internal `FileStore` ([`src/file_store.rs`](./src/file_store.rs)) — directory-backed durable store. Write path: encode op → append framed entry to WAL → apply to in-memory map. Read path: serve from the in-memory mirror. Recovery on open: load snapshot, replay WAL on top, truncate corrupt tail to last known-good offset. Compaction on close: write fresh snapshot, atomic-rename over old snapshot, truncate WAL.
- Cross-platform `full_sync` primitive at [`src/platform.rs`](./src/platform.rs). macOS uses `fcntl(fd, F_FULLFSYNC, 0)` (the only platform that needs an escape hatch beyond `fsync` for true power-loss durability — SQLite and Core Data use it for the same reason). Other Unix uses `fsync(2)` via `File::sync_all`. Windows uses `FlushFileBuffers` via `File::sync_all`.
- Binary frame codec at [`src/codec.rs`](./src/codec.rs) — length-prefixed (u32 LE) + CRC32 (IEEE 802.3) tail. Encodes upsert and delete ops over `RecordId` / `Vector` / `Payload` / `PayloadValue`. Little-endian throughout regardless of host byte order, so databases written on x86_64 read back identically on aarch64. Versioned snapshot header (4-byte magic `IQDB` + u32 LE format version) so future format changes can negotiate compatibility without silent degradation.
- Crate-internal `Backend` enum at [`src/backend.rs`](./src/backend.rs) — `Memory(MemoryStore)` / `File(FileStore)`. Every `Iqdb` method dispatches through a hand-written match for zero dynamic-dispatch cost on the hot path. The search kernel binds to both backends through the same `with_records` shape.
- `Error::Corrupt { reason: &'static str }` — surfaced by `Iqdb::open(path)` when the snapshot fails an integrity check (bad magic, unknown format version, truncated header). WAL corruption is handled internally (truncation to last good offset) and does not surface as an error.
- `Iqdb::flush` and `Iqdb::close` are now load-bearing for both backends:
  - **In-memory**: `flush` returns `Ok(())` (the in-memory map is already as durable as a memory-only backend can be); `close` drops the map.
  - **File-backed**: `flush` runs `full_sync` on the WAL; `close` runs a full compaction (snapshot rewrite + atomic rename + WAL truncate) so the next open is a single-file load with no replay.
- `MemoryStore::with_records` is now mirrored by `FileStore::with_records` and exposed through `Backend::with_records`, so the search kernel works identically on both backends.
- Integration test suite at [`tests/persistence.rs`](./tests/persistence.rs) — 12 tests covering the full durable lifecycle: fresh-directory open, file-path rejection, upsert/delete round-trip across close+reopen, payload round-trip through compaction, recovery without close, recovery without flush, search against recovered data, multi-cycle state preservation, WAL truncation on close, snapshot integrity check, and silent WAL-tail truncation on corruption.
- Property-based persistence test at [`tests/properties.rs`](./tests/properties.rs) — proves the full open → upsert → close → reopen round-trip preserves arbitrary record sets (bounded to ≤8 records, dim 4, so the default 256-case sweep finishes under a second).
- New example [`examples/persistence.rs`](./examples/persistence.rs) — three-session walkthrough: open + upsert + close, reopen + verify + search + delete + close, reopen + confirm-delete + close. Demonstrates the full durable workflow against a single on-disk database.
- New benchmark group `file_store` in [`benches/vector_ops.rs`](./benches/vector_ops.rs) — `upsert_dim128_then_flush` measures the durable-write path (fresh DB per iteration so the bench is not dominated by the cumulative cost of a growing WAL), `open_snapshot_only_1k_records_dim128` measures recovery throughput against a snapshot-only DB.
- Unix-only `libc = "0.2"` runtime dependency (gated on `cfg(unix)`) — used exclusively for the macOS `fcntl(F_FULLFSYNC)` call. Windows builds pull zero runtime dependencies; Linux pulls only `libc`.
- `serde_json` dev-dependency promoted to cover the new persistence integration tests' edge cases. (Already present from v0.2.0 for the optional `serde` feature; no production impact.)
- Full v0.4.0 surface documented in [`docs/API.md`](./docs/API.md) — new "Durable Storage" section covering write path, durability contract, open/recover, close/compact, platform-specific sync, and on-disk format. New `Error::Corrupt` row in the error table. Persistence example linked in the Examples section.

### Changed

- `Iqdb::flush()` no longer returns `Error::NotImplemented`. Both backends now answer `flush` meaningfully — in-memory as a no-op `Ok(())`, file-backed as `full_sync` on the WAL. The previous staged-surface doctest pattern (`match Err(Error::NotImplemented) => fallback`) is removed from the crate-level rustdoc; the v0.4.0 example shows the full durable lifecycle instead.
- `Iqdb::open(path)` no longer returns `Error::NotImplemented`. It opens or creates a directory-backed durable database at the given path. Source-compatible with v0.3.0 callers that branched on the `NotImplemented` variant (the branch becomes dead code; no compilation break).
- The v0.2.0 integration test `flush_and_open_path_still_not_implemented_in_v0_2_0` is renamed to `flush_and_close_on_in_memory_are_ok_in_v0_4_0` and updated to assert the new `Ok(())` semantics.
- README updated: v0.4.0 milestone marked current, v0.3.0 marked shipped; Quick Start replaces the staged-surface flush example with a directory-backed `open(path)` walkthrough; benchmark and testing sections updated for the new `file_store` group and `tests/persistence.rs`; Architecture module list expanded to include `backend.rs`, `file_store.rs`, `codec.rs`, `platform.rs`.
- Crate-level rustdoc (in `src/lib.rs`) — the third runnable example now demonstrates the durable-storage lifecycle (open, upsert, flush, close) instead of the staged-surface `Error::NotImplemented` pattern.

### Removed

Nothing removed in v0.4.0 — the surface is additive on top of v0.3.0. The `Error::NotImplemented` variant remains in the public API (still `#[non_exhaustive]`) so future-milestone wiring patterns can continue to use it.

[Unreleased]: https://github.com/jamesgober/iqdb/compare/v0.7.0...HEAD
[0.7.0]: https://github.com/jamesgober/iqdb/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/jamesgober/iqdb/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/jamesgober/iqdb/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/jamesgober/iqdb/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/jamesgober/iqdb/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/jamesgober/iqdb/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/iqdb/releases/tag/v0.1.0
