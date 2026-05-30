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
    <img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.75%2B-blue.svg?style=flat-square" title="Rust Version">
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
        The engine is built against a <b>lock-free hot path</b>, <b>allocation-free steady state</b>, and a <b>cache-aware</b> on-disk layout. Approximate-nearest-neighbour indices, exact brute-force search, payload metadata, and durable journaling are all in scope. Indices and storage are pluggable so workloads can trade recall for latency without rewriting the surrounding application.
    </p>
    <p>
        Built for <b>cross-platform</b> deployment from day one — Linux, macOS, and Windows are first-class targets, with <code>io_uring</code> on Linux and atomic file replacement everywhere else. The async surface is opt-in: synchronous embedded use stays free of executor coupling, while applications already running on Tokio can drive the same engine without bridging.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.75+.</strong> The crate is dual-licensed under <code>Apache-2.0 OR MIT</code> at your option.
    </p>
    <blockquote>
        <strong>0.3.0 ships exact top-<code>k</code> search.</strong> <code>Iqdb::search</code> / <code>search_with</code> / <code>search_batch</code> / <code>search_batch_with</code> run an exact flat scan with a bounded top-<code>k</code> heap; filters monomorphise into the scan loop with no per-record dynamic dispatch. Results — a <code>SearchResult { id, score, payload }</code> per hit — are returned sorted ascending under the smaller-is-closer convention. Approximate indices (IVF, HNSW) land in <code>v0.5.0</code> alongside the flat kernel rather than replacing it. The durable backend is still <code>v0.4.0</code>; <code>Iqdb::open(path)</code> and <code>Iqdb::flush</code> return <a href="./src/error.rs"><code>Error::NotImplemented</code></a> until then. The API is <b>unstable</b> until 1.0; see <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a> for the release-by-release surface, and <a href="./docs/API.md"><code>docs/API.md</code></a> for the full reference.
    </blockquote>
</div>

<hr>
<br>

## Design Goals

iQDB is engineered against the <a href="./REPS.md">Rust Efficiency &amp; Performance Standards</a> (REPS). Every architectural decision is graded against the same hard constraints:

- **Embedded by default** — no daemon, no socket, no separate process. The library opens a path and gets out of the way.
- **Sub-millisecond queries** — exact and approximate search paths are budgeted in microseconds, not milliseconds, with the hot path measured under Criterion every release.
- **Lock-free hot paths** — atomic reads on the query path; writers do not contend with concurrent readers.
- **Allocation-free steady state** — buffers are pooled and reused. Per-query allocation is a design defect.
- **Cache-aware layout** — vectors land contiguous in memory and on disk; index nodes are padded to the cache line.
- **Pluggable indices** — flat, IVF, and HNSW share a common trait surface so callers can swap strategies without touching their query code.
- **Pluggable storage** — durable file-backed storage, transient in-memory storage, and the org-default `storage-io` substrate are all addressable behind the same handle.
- **Crash-safe writes** — the durable path uses write-ahead logging and atomic file replacement. A pulled power cord must never corrupt the index.
- **Tier-1 cross-platform** — Linux (x86_64, aarch64), macOS (x86_64, Apple Silicon), Windows (x86_64) all compile and pass the full test suite on every commit.

<hr>
<br>

## Status &amp; Roadmap

iQDB ships milestone-by-milestone. Each tag below corresponds to a published release; everything above the current line is shipped, everything below is planned.

| Milestone | Status | Surface delivered |
|-----------|--------|-------------------|
| `v0.1.0` — scaffolding | shipped | Crate scaffolding, lifecycle handle (`open` / `open_in_memory` / `flush` / `close`), `Error` type, integration test, criterion harness, CI matrix on all three Tier-1 platforms. |
| `v0.2.0` — vector primitives | shipped | `Vector` (validated f32 embeddings), `DistanceMetric` (L2 / Cosine / Dot), `Payload` & `PayloadValue` (typed metadata), `RecordId`, `Record`. In-memory store with thread-safe `upsert` / `get` / `delete` / `len` / `is_empty`. Optional `serde` feature. |
| `v0.3.0` — search | **current** | `Iqdb::search` / `search_with` / `search_batch` / `search_batch_with` over the flat index. `SearchResult { id, score, payload }`. Monomorphic predicate filters. NaN-aware ranking with id tie-break. Property-based ranking tests via `proptest`. `docs/API.md` published. |
| `v0.4.0` — durable storage | planned | File-backed storage substrate. Write-ahead log, atomic-replace snapshots, crash recovery. `Iqdb::open(path)` becomes load-bearing. |
| `v0.5.0` — approximate indices | planned | IVF and HNSW indices behind the same trait the flat index implements. Build-time index selection via the builder. |
| `v0.6.0` — async surface | planned | `async`-feature-gated mirror of the public API. Driven by Tokio. Cancellation-safe. |
| `v0.7.0` — collections | planned | Named collections / namespaces with per-collection metric and dimensionality. Collection-scoped iteration. |
| `v1.0.0` — API freeze | planned | Frozen public API. SemVer guarantees. Full benchmark suite. Migration guide from 0.x. |

The per-release detail — what was added, what changed, and what was verified — lives in the [`CHANGELOG`](./CHANGELOG.md) and the per-version notes under [`docs/release/`](./docs/release/).

<hr>
<br>

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
iqdb = "0.3"
```

Enable the optional `serde` feature to derive `Serialize` / `Deserialize` on every public data type:

```toml
[dependencies]
iqdb = { version = "0.3", features = ["serde"] }
```

iQDB compiles on stable Rust **1.75** and newer. The default build pulls zero runtime dependencies; the `serde` feature pulls only `serde` itself.

<hr>
<br>

## Quick Start

The `0.3.0` surface exposes typed vector primitives, the in-memory store, and exact top-`k` similarity search. The durable file-backed substrate lands in `v0.4.0`; approximate indices (IVF, HNSW) in `v0.5.0` will sit alongside the flat kernel rather than replacing it — exact search remains the correctness baseline.

```rust
use iqdb::{DistanceMetric, Iqdb, Payload, Record, RecordId, Result, Vector};

fn main() -> Result<()> {
    let db = Iqdb::open_in_memory();

    let mut meta = Payload::new();
    meta.insert("kind", "doc");

    db.upsert(Record::with_payload(
        RecordId::new(1),
        Vector::new(vec![1.0, 0.0, 0.0])?,
        meta,
    ))?;
    db.upsert(Record::new(
        RecordId::new(2),
        Vector::new(vec![0.99, 0.10, 0.0])?,
    ))?;

    // Top-k similarity search. Results are sorted ascending under the
    // smaller-is-closer rule; ties break on id for determinism.
    let probe = Vector::new(vec![1.0, 0.0, 0.0])?;
    let hits = db.search(&probe, 5, DistanceMetric::Cosine)?;
    assert_eq!(hits.first().map(|h| h.id), Some(RecordId::new(1)));

    db.close()
}
```

### Filtered and batch search

Filters are generic — the predicate monomorphises into the search loop, so there is no per-record dynamic dispatch. The smaller-is-closer convention holds across all three metrics; `Dot` returns `−(a · b)` so a single ordering rule covers L2, Cosine, and Dot.

```rust
use iqdb::{DistanceMetric, Iqdb, Payload, PayloadValue, Record, RecordId, Result, Vector};

fn main() -> Result<()> {
    let db = Iqdb::open_in_memory();

    let mut doc = Payload::new();
    doc.insert("kind", "doc");
    db.upsert(Record::with_payload(
        RecordId::new(1),
        Vector::new(vec![1.0, 0.0])?,
        doc,
    ))?;

    let mut image = Payload::new();
    image.insert("kind", "image");
    db.upsert(Record::with_payload(
        RecordId::new(2),
        Vector::new(vec![0.99, 0.10])?,
        image,
    ))?;

    let probe = Vector::new(vec![1.0, 0.0])?;

    // Filter the candidate set before heap admission.
    let docs_only = db.search_with(&probe, 5, DistanceMetric::Cosine, |rec| {
        rec.payload()
            .and_then(|p| p.get("kind"))
            .and_then(PayloadValue::as_text)
            == Some("doc")
    })?;
    assert_eq!(docs_only.len(), 1);
    assert_eq!(docs_only[0].id, RecordId::new(1));

    // Batch — one top-k result list per query, preserves input order.
    let probes = vec![
        Vector::new(vec![1.0, 0.0])?,
        Vector::new(vec![0.0, 1.0])?,
    ];
    let batches = db.search_batch(&probes, 1, DistanceMetric::Cosine)?;
    assert_eq!(batches.len(), 2);

    Ok(())
}
```

### Handling the staged surface

Because `Iqdb::open(path)` and `Iqdb::flush` still belong to the durable-storage milestone, downstream callers can wire them in advance and gate behaviour on the error variant — the `Err` arm disappears when `v0.4.0` ships:

```rust
use iqdb::{Error, Iqdb};

fn flush_if_supported(db: &Iqdb) -> Result<(), Error> {
    match db.flush() {
        Ok(()) => Ok(()),
        Err(Error::NotImplemented) => {
            // The active milestone does not yet support flushing.
            // In v0.4.0 (durable storage) this branch disappears.
            Ok(())
        }
        Err(err) => Err(err),
    }
}

let db = Iqdb::open_in_memory();
flush_if_supported(&db).unwrap();
```

<hr>
<br>

## API Overview

The full API reference lives at [`docs/API.md`](./docs/API.md); the rustdoc-generated docs at [docs.rs/iqdb](https://docs.rs/iqdb) carry the same information in browseable form. The currently-stable items are:

- [`Iqdb`](./src/db.rs) — the top-level database handle.
  - `Iqdb::open(path)` — open or create a file-backed database (planned for v0.4.0 — currently returns `Error::NotImplemented`).
  - `Iqdb::open_in_memory()` — open an ephemeral instance backed entirely by RAM.
  - `Iqdb::upsert(record)` — insert or replace a record. Idempotent.
  - `Iqdb::get(id)` — look up by id. Returns `Ok(None)` when absent.
  - `Iqdb::delete(id)` — remove by id. Returns whether the id was present.
  - `Iqdb::len()` / `Iqdb::is_empty()` — store cardinality.
  - `Iqdb::search(query, k, metric)` — exact top-`k` similarity search, no filter.
  - `Iqdb::search_with(query, k, metric, filter)` — top-`k` with a per-record predicate. The filter monomorphises into the scan loop; no per-record dynamic dispatch.
  - `Iqdb::search_batch(queries, k, metric)` / `search_batch_with(...)` — sequential batch variants. Preserves input order.
  - `Iqdb::flush()` — flush pending writes to durable storage (planned for v0.4.0).
  - `Iqdb::close(self)` — close the handle and release any held resources.
- [`Vector`](./src/vector.rs) — owned, contiguous, validated f32 embedding. `Vector::new(Vec<f32>)` / `Vector::from_slice(&[f32])` validate at the system boundary (no empty vectors, no `NaN`, no infinity); `as_slice` / `dim` / `norm` / `norm_squared` are non-allocating.
- [`DistanceMetric`](./src/vector.rs) — `L2`, `Cosine`, `Dot`. `metric.distance(a, b)` returns a `Result<f32>` under the smaller-is-closer convention; dimensional homogeneity is enforced.
- [`Payload`](./src/payload.rs) — typed `BTreeMap<String, PayloadValue>` for metadata. Deterministic iteration order makes payloads stable across `serde` round-trips and test assertions.
- [`PayloadValue`](./src/payload.rs) — `Null` / `Bool` / `Int` / `Float` / `Text` / `Bytes` / `Array` / nested `Object`. `From<T>` conversions cover the primitives.
- [`RecordId`](./src/record.rs) — transparent newtype around `u64`. Cheap to copy, hash, and compare.
- [`Record`](./src/record.rs) — `(id, vector, optional payload)` aggregate. `Record::new` / `Record::with_payload` are the two constructors; `into_parts` decomposes without a clone.
- [`SearchResult`](./src/search.rs) — `{ id, score, payload }` returned by the search methods. Sorted ascending by `score`; ties broken on `id`; `NaN` scores sort to the tail.
- [`Error`](./src/error.rs) — the unified error type. `#[non_exhaustive]`.
- [`Result<T>`](./src/error.rs) — alias for `core::result::Result<T, Error>`.

### Error variants

| Variant | Meaning | Recovery |
|---------|---------|----------|
| `Error::Io(std::io::Error)` | A lower-level I/O failure occurred — disk full, permission denied, missing path, etc. | Inspect the wrapped `ErrorKind`. Retry transient errors; surface permanent ones. |
| `Error::InvalidConfig(&'static str)` | Configuration supplied at open time was invalid (e.g., zero-length path, unsupported metric). | Programmer error — fix the construction site. |
| `Error::InvalidVector { reason }` | A vector failed boundary validation — empty, or contains `NaN` / ±∞. | Sanitise the input at the producer side; `Vector::new` rejects bad data before it enters the store. |
| `Error::DimensionMismatch { left, right }` | A distance-metric or store operation combined two vectors of different dimensionality. | Enforce a homogeneous schema at the producer side or surface a typed error to the caller. |
| `Error::NotImplemented` | The requested operation belongs to a later milestone and has no engine behind it yet. | Either upgrade to a release where the verb is implemented, or branch on the variant and fall back. |

The enum is `#[non_exhaustive]`. New variants will appear in minor releases as new failure modes emerge. Exhaustive `match` arms are a forward-compatibility hazard — always include `_`.

<hr>
<br>

## Examples

Self-contained examples live in [`examples/`](./examples) and are declared in `Cargo.toml`. Run them with `cargo run --example <name>`.

- **Lifecycle (`basic`)** — open an in-memory instance, upsert a vector, read it back, close.
  - File: [`examples/basic.rs`](./examples/basic.rs)
  - Run:
    ```sh
    cargo run --example basic --release
    ```

- **In-memory store walk-through (`in_memory_store`)** — populate the store with three records (vectors + typed payloads), compare distances between them under L2 and cosine, then delete one.
  - File: [`examples/in_memory_store.rs`](./examples/in_memory_store.rs)
  - Run:
    ```sh
    cargo run --example in_memory_store --release
    ```

- **Top-`k` search (`search`)** — unfiltered cosine top-`k`, payload-filtered search, and batch search across three probes in one file.
  - File: [`examples/search.rs`](./examples/search.rs)
  - Run:
    ```sh
    cargo run --example search --release
    ```

Approximate-index examples land alongside their milestone (`v0.5.0`).

<hr>
<br>

## Benchmarks

A criterion harness is wired in [`benches/vector_ops.rs`](./benches/vector_ops.rs). v0.3.0 ships four groups:

- **`vector_new`** — construction-time validation cost across small / medium / large dimensionalities (32 / 128 / 1024).
- **`distance`** — single-shot distance computation under each of the three [`DistanceMetric`](./src/vector.rs) variants at dim 128.
- **`store`** — `upsert` and `get` throughput against a populated in-memory store at 1 000 records, dim 128.
- **`search`** — flat top-`k` search at 1 000 and 10 000 records, dim 128. Three variants: unfiltered, payload-filtered (~50% pruning), and batch-of-4.

Run with:

```sh
cargo bench --bench vector_ops
```

Criterion writes reports to `target/criterion/`. Approximate-index benches land with `v0.5.0` and the durable-write benches land with `v0.4.0`; CI will gate merges on a regression threshold once the benches are stable.

<hr>
<br>

## Testing

Every public path has happy / error / edge-case coverage:

- Unit tests live in `#[cfg(test)] mod tests` blocks inside each source file.
- Integration tests live in [`tests/`](./tests):
  - [`tests/in_memory.rs`](./tests/in_memory.rs) — CRUD surface plus `serde` round-trips behind a feature gate.
  - [`tests/search.rs`](./tests/search.rs) — the four search entry points: top-`k` ordering, filter pruning, batch order, dimension-mismatch handling, payload preservation, concurrent readers.
  - [`tests/properties.rs`](./tests/properties.rs) — `proptest`-driven property tests for distance-metric algebra (symmetry, identity, range bounds) and search-ranking invariants (length bound, ascending order, perfect-match presence, no-filter parity).
  - [`tests/smoke.rs`](./tests/smoke.rs) — minimal lifecycle smoke check.
- Doc tests run as part of `cargo test` and validate every `# Examples` block in the rustdoc.

```sh
# Full test sweep (unit + integration + doc tests)
cargo test
cargo test --all-features

# Documentation build with no warnings (matches CI gating)
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

# Lint at the strict profile CI enforces
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings

# Formatting check (no diffs)
cargo fmt --all -- --check
```

<hr>
<br>

## Cross-Platform Support

**Tier 1 targets** — every commit is built and tested on:

- Linux (`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`)
- macOS (`x86_64-apple-darwin`, `aarch64-apple-darwin`)
- Windows (`x86_64-pc-windows-msvc`)

**Platform-specific paths** — when the durable storage substrate lands, the platform-conditional code paths become observable:

- **Linux**: `io_uring` submission for batch writes; `fsync` for journal durability.
- **macOS**: `F_FULLFSYNC` for journal durability; `mmap` for read-mostly indices.
- **Windows**: `FlushFileBuffers` for journal durability; `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` for atomic snapshot replacement.

No platform is silently degraded — fallbacks are explicit and documented inline at each `#[cfg]` boundary, per the REPS *Platform-Specific Code* rule.

<hr>
<br>

## Configuration

### Feature flags

Feature flags are strictly additive (per REPS) — enabling any combination never removes or weakens existing functionality.

| Feature       | Default | Available     | Description                                                          |
|---------------|---------|---------------|----------------------------------------------------------------------|
| `serde`       | off     | **shipping**  | Derives `Serialize` / `Deserialize` on every public data type.       |
| `async`       | off     | planned v0.6.0 | Tokio-driven async mirror of the public API.                         |
| `mmap`        | off     | planned v0.4.0 | Memory-mapped read path for hot indices.                             |
| `io-uring`    | off     | planned v0.4.0 | Linux-only `io_uring` submission for batch writes.                   |
| `full`        | off     | planned post-1.0 | All stable features in one switch.                                |

```toml
iqdb = { version = "0.2", features = ["serde"] }
```

### Runtime configuration

`Iqdb::open_in_memory()` takes no parameters by design. A builder (`IqdbBuilder`) is introduced in `v0.4.0` once the file-backed path lands and has more than two open-time knobs. Until then, the constructor surface is the entire configuration surface.

<hr>
<br>

## Architecture

The crate is split along strict module boundaries — each module owns one concern and exposes a single trait or type as its contract:

- `src/lib.rs` — crate root, lint profile, public re-exports.
- `src/db.rs` — the [`Iqdb`](./src/db.rs) handle and the public CRUD verbs.
- `src/vector.rs` — the [`Vector`](./src/vector.rs) primitive and the [`DistanceMetric`](./src/vector.rs) dispatch.
- `src/payload.rs` — the [`Payload`](./src/payload.rs) / [`PayloadValue`](./src/payload.rs) typed metadata layer.
- `src/record.rs` — the [`RecordId`](./src/record.rs) / [`Record`](./src/record.rs) aggregate.
- `src/store.rs` — the crate-internal `MemoryStore` (the read/write engine behind `open_in_memory`).
- `src/error.rs` — the [`Error`](./src/error.rs) enum and `Result` alias.

As milestones land, the tree grows along the same bounded-responsibility pattern (`index/`, `journal/`, `query/`, `async_impl.rs`). The boundary between layers is always a trait or a concrete type with a typed surface — concrete backend implementations are crate-internal and gated behind `pub(crate)`.

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

Test modules locally relax the `unwrap_used` / `expect_used` lints — the strict profile is for production library code, not assertion scaffolding inside `#[cfg(test)]` blocks.

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
