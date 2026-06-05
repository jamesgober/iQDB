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

This document is the canonical API reference for **iqdb v0.4.0**. Every public type, method, error variant, and feature flag is recorded here with parameter descriptions and at least one runnable example. The narrative companion is the [README](../README.md); the per-release notes live under [`docs/release/`](./release/).

## Table of Contents

- [Installation](#installation)
- [Examples](#examples)
- [Quick Start](#quick-start)
- [Error Handling](#error-handling)
- [Public APIs](#public-apis)
  - [`Iqdb`](#iqdb)
  - [`Vector`](#vector)
  - [`DistanceMetric`](#distancemetric)
  - [`Payload`](#payload)
  - [`PayloadValue`](#payloadvalue)
  - [`RecordId`](#recordid)
  - [`Record`](#record)
  - [`SearchResult`](#searchresult)
  - [`Error` / `Result`](#error--result)
- [Feature Flags](#feature-flags)
- [Notes](#notes)

<br>

## Installation

Add to `Cargo.toml`:

```toml
[dependencies]
iqdb = "0.4"
```

Enable the optional `serde` feature to derive `Serialize` / `Deserialize` on every public data type:

```toml
[dependencies]
iqdb = { version = "0.4", features = ["serde"] }
```

iqdb compiles on stable Rust **1.87** and newer. The default build pulls **one** runtime dependency on Unix (`libc`, used exclusively for the macOS `F_FULLFSYNC` call) and **zero** on Windows. The `serde` feature additionally pulls only the `serde` crate itself.

<br>

## Examples

Self-contained runnable examples live in [`examples/`](../examples) and are declared in `Cargo.toml`. Run them with `cargo run --example <name>`.

- [`basic`](../examples/basic.rs) — open an in-memory instance, upsert a single vector, read it back, close.
- [`in_memory_store`](../examples/in_memory_store.rs) — populate the store with labelled records, inspect payloads, compare distances under L2 and Cosine, delete a record.
- [`search`](../examples/search.rs) — top-`k` similarity search: unfiltered, payload-filtered, and batch.
- [`persistence`](../examples/persistence.rs) — directory-backed store: open, upsert, close, reopen, delete, close, reopen again — three sessions all consistent against the same on-disk database.

<br>

## Quick Start

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

    let probe = Vector::new(vec![1.0, 0.0, 0.0])?;
    let hits = db.search(&probe, 5, DistanceMetric::Cosine)?;
    assert_eq!(hits.first().map(|h| h.id), Some(RecordId::new(1)));

    db.close()
}
```

<br>

## Error Handling

Every fallible operation returns `Result<T>` — an alias for `core::result::Result<T, Error>`. The [`Error`](#error--result) enum is `#[non_exhaustive]`; always include a `_` arm when matching so new variants in future minor releases don't break call sites.

`Error::Io` exposes the wrapped `std::io::Error` as the error chain source — `anyhow`, `tracing`, and report formatters can walk to the root cause without unwrapping the variant manually.

<br>

## Public APIs

### `Iqdb`

Source: [`src/db.rs`](../src/db.rs)

The top-level handle. Owns the active backend (in-memory or directory-backed) and brokers every read / write through a typed API. `Iqdb` is `Send + Sync`; share across threads via `Arc<Iqdb>`.

#### Lifecycle

- `Iqdb::open<P: AsRef<Path>>(path: P) -> Result<Self>` — open or create a **directory-backed** durable database at `path`. The directory contains a snapshot (`snap`) and a write-ahead log (`wal`). If the directory does not exist, it is created. If `path` exists but is not a directory, returns `Error::InvalidConfig`. On open, the snapshot is loaded into the in-memory map and the WAL is replayed on top; a corrupt WAL tail is truncated to the last known-good offset.
- `Iqdb::open_in_memory() -> Self` — open an ephemeral, in-memory instance. Never touches the filesystem.
- `Iqdb::flush(&self) -> Result<()>` — sync pending writes to durable storage. **File-backed**: runs `F_FULLFSYNC` on macOS, `fsync(2)` on other Unix, `FlushFileBuffers` on Windows. **In-memory**: returns `Ok(())` — no-op (there is no durable substrate).
- `Iqdb::close(self) -> Result<()>` — consume the handle and release resources. **File-backed**: runs a compaction — writes a fresh snapshot, atomically replaces the old one, truncates the WAL. **In-memory**: drops the in-memory map.

#### Record management

- `Iqdb::upsert(&self, record: Record) -> Result<()>` — insert or replace `record`. Idempotent.
- `Iqdb::get(&self, id: RecordId) -> Result<Option<Record>>` — id lookup. Returns `Ok(None)` when absent; the matched record is cloned out so the internal read lock is released before the value is returned.
- `Iqdb::delete(&self, id: RecordId) -> Result<bool>` — id removal. Returns `Ok(true)` when a record was removed, `Ok(false)` otherwise — idempotent.
- `Iqdb::len(&self) -> usize` — current cardinality.
- `Iqdb::is_empty(&self) -> bool` — `true` if no records are stored.

#### Search

- `Iqdb::search(&self, query: &Vector, k: usize, metric: DistanceMetric) -> Result<Vec<SearchResult>>` — exact top-`k` similarity search, no filter.
- `Iqdb::search_with<F>(&self, query: &Vector, k: usize, metric: DistanceMetric, filter: F) -> Result<Vec<SearchResult>>`
  - `where F: Fn(&Record) -> bool`
  - Filtered top-`k`. The predicate is monomorphised into the search loop — no per-record dynamic dispatch. Records that fail the predicate are excluded from heap admission.
  - **Warning:** the filter runs while the store's read lock is held. Do not call back into the same `Iqdb` handle from inside the filter.
- `Iqdb::search_batch(&self, queries: &[Vector], k: usize, metric: DistanceMetric) -> Result<Vec<Vec<SearchResult>>>` — sequential batch. Preserves input order: `output[i]` is the top-`k` for `queries[i]`.
- `Iqdb::search_batch_with<F>(&self, queries: &[Vector], k: usize, metric: DistanceMetric, filter: F) -> Result<Vec<Vec<SearchResult>>>`
  - `where F: Fn(&Record) -> bool`
  - Sequential batch with a shared filter applied to every query.

Each result is a [`SearchResult`](#searchresult) — `(id, score, payload)`. Results are sorted by `score` ascending under the smaller-is-closer convention; ties break on `id` ascending for deterministic ordering. `NaN` scores (produced by Cosine against a zero vector) sort to the tail.

#### Example

```rust
use iqdb::{DistanceMetric, Iqdb, Payload, PayloadValue, Record, RecordId, Result, Vector};

fn run() -> Result<()> {
    let db = Iqdb::open_in_memory();

    for (id, components, kind) in [
        (1_u64, vec![1.0, 0.0], "doc"),
        (2,     vec![0.99, 0.10], "doc"),
        (3,     vec![0.0, 1.0],  "image"),
    ] {
        let mut p = Payload::new();
        p.insert("kind", kind);
        db.upsert(Record::with_payload(RecordId::new(id), Vector::new(components)?, p))?;
    }

    let probe = Vector::new(vec![1.0, 0.0])?;

    // Unfiltered top-2 under cosine.
    let top2 = db.search(&probe, 2, DistanceMetric::Cosine)?;
    assert_eq!(top2.len(), 2);

    // Predicate-filtered.
    let docs_only = db.search_with(&probe, 5, DistanceMetric::Cosine, |rec| {
        rec.payload()
            .and_then(|p| p.get("kind"))
            .and_then(PayloadValue::as_text)
            == Some("doc")
    })?;
    assert!(docs_only.iter().all(|h| {
        h.payload.as_ref()
            .and_then(|p| p.get("kind"))
            .and_then(PayloadValue::as_text)
            == Some("doc")
    }));

    // Batch — three probes at once.
    let probes = vec![probe.clone(), Vector::new(vec![0.0, 1.0])?, Vector::new(vec![-1.0, 0.0])?];
    let batches = db.search_batch(&probes, 1, DistanceMetric::Cosine)?;
    assert_eq!(batches.len(), 3);

    Ok(())
}
# run().unwrap();
```

<br>

### `Vector`

Source: [`src/vector.rs`](../src/vector.rs)

An immutable, owned numeric vector of `f32` components. Backed by `Box<[f32]>` — dense, contiguous, no spare-capacity overhead. Construction validates the input; downstream code can treat every constructed `Vector` as known-good.

#### Constructors

- `Vector::new(data: Vec<f32>) -> Result<Self>` — consume an owned `Vec<f32>`.
- `Vector::from_slice(data: &[f32]) -> Result<Self>` — copy from a borrowed slice.
- `TryFrom<Vec<f32>>` / `TryFrom<&[f32]>` — ergonomic `try_into()` call sites.

Both constructors reject:

- Empty input → `Error::InvalidVector { reason: "vector is empty" }`.
- Any non-finite component (`NaN`, `+∞`, `−∞`) → `Error::InvalidVector { reason: "vector contains a non-finite value" }`.

Zero vectors (all components `0.0`) are **accepted** — they are valid sparse embeddings, even though they break cosine similarity (which produces `NaN`).

#### Accessors

- `dim(&self) -> usize` — dimensionality. Always `>= 1` for any constructed `Vector`.
- `as_slice(&self) -> &[f32]` — borrow components.
- `into_inner(self) -> Box<[f32]>` — consume and return the underlying boxed slice.
- `norm_squared(&self) -> f32` — `self · self`.
- `norm(&self) -> f32` — `sqrt(self · self)`.
- `AsRef<[f32]>` — flow `Vector` through APIs that accept any slice-like.

#### Example

```rust
use iqdb::Vector;

let v = Vector::new(vec![3.0, 4.0])?;
assert_eq!(v.dim(), 2);
assert_eq!(v.norm(), 5.0);
assert_eq!(v.as_slice(), &[3.0, 4.0]);

// Bad inputs surface at the boundary.
assert!(Vector::new(Vec::<f32>::new()).is_err());
assert!(Vector::new(vec![1.0, f32::NAN]).is_err());
# Ok::<(), iqdb::Error>(())
```

<br>

### `DistanceMetric`

Source: [`src/vector.rs`](../src/vector.rs)

Distance metric used to compare two vectors. Every variant follows the **smaller-is-closer** convention so a single ordering rule covers all three:

| Variant   | Returns                                       | Range            |
|-----------|-----------------------------------------------|------------------|
| `L2`      | Euclidean distance `‖a − b‖₂`                 | `[0, +∞)`        |
| `Cosine`  | `1 − (a · b) / (‖a‖ · ‖b‖)`                   | `[0, 2]`         |
| `Dot`     | `−(a · b)` — *negative* dot product           | `(−∞, +∞)`       |

Marked `#[non_exhaustive]` so new metrics can be added in minor releases without a SemVer break.

#### Methods

- `DistanceMetric::distance(self, a: &Vector, b: &Vector) -> Result<f32>` — compute the distance under this metric.
  - **Error:** `Error::DimensionMismatch { left, right }` when `a.dim() != b.dim()`.
  - **NaN:** `Cosine` returns `NaN` when either vector has zero norm. The search engine treats `NaN` scores as worst (sorts them to the tail of the result list).

#### Example

```rust
use iqdb::{DistanceMetric, Vector};

let a = Vector::new(vec![1.0, 0.0])?;
let b = Vector::new(vec![0.0, 1.0])?;

let l2 = DistanceMetric::L2.distance(&a, &b)?;
let cos = DistanceMetric::Cosine.distance(&a, &b)?;
let dot = DistanceMetric::Dot.distance(&a, &b)?;

assert!((l2 - std::f32::consts::SQRT_2).abs() < 1e-6);
assert!((cos - 1.0).abs() < 1e-6);
assert!(dot.abs() < 1e-6);
# Ok::<(), iqdb::Error>(())
```

<br>

### `Payload`

Source: [`src/payload.rs`](../src/payload.rs)

Ordered, typed key-value bag attached to a stored record. Thin wrapper around `BTreeMap<String, PayloadValue>` — deterministic iteration order so payload hashes, `serde` round-trips, and test assertions are stable.

#### Methods

- `Payload::new() -> Self` — construct an empty payload.
- `Payload::insert(&mut self, key: impl Into<String>, value: impl Into<PayloadValue>) -> Option<PayloadValue>` — insert or replace. Returns the prior value if any.
- `Payload::get(&self, key: &str) -> Option<&PayloadValue>` — borrow by key.
- `Payload::remove(&mut self, key: &str) -> Option<PayloadValue>` — remove by key, returning the prior value.
- `Payload::contains_key(&self, key: &str) -> bool`
- `Payload::len(&self) -> usize`
- `Payload::is_empty(&self) -> bool`
- `Payload::iter(&self) -> btree_map::Iter<'_, String, PayloadValue>` — borrowed pairs in key order.
- `Payload::as_map(&self) -> &BTreeMap<String, PayloadValue>` — borrow the underlying map.

Implements `IntoIterator` (by reference and by value) and `FromIterator<(String, PayloadValue)>` for the standard collection idioms.

#### Example

```rust
use iqdb::{Payload, PayloadValue};

let mut p = Payload::new();
p.insert("source", "wikipedia");
p.insert("year", 2026_i64);
p.insert("verified", true);

assert_eq!(p.get("source").and_then(PayloadValue::as_text), Some("wikipedia"));
assert_eq!(p.get("year").and_then(PayloadValue::as_int), Some(2026));
assert_eq!(p.get("verified").and_then(PayloadValue::as_bool), Some(true));

// Construct from a fixed list.
let q: Payload = [
    ("a".to_string(), PayloadValue::from(1_i64)),
    ("b".to_string(), PayloadValue::from(2_i64)),
]
.into_iter()
.collect();
assert_eq!(q.len(), 2);
```

<br>

### `PayloadValue`

Source: [`src/payload.rs`](../src/payload.rs)

Tagged union of typed payload values. Marked `#[non_exhaustive]`.

#### Variants

- `Null` — explicit missing-value marker.
- `Bool(bool)`
- `Int(i64)`
- `Float(f64)` — `NaN` / `±∞` are accepted for metadata (filters in v0.3.0 use IEEE 754 ordering).
- `Text(String)` — owned UTF-8.
- `Bytes(Vec<u8>)` — owned byte buffer (blobs that should not be re-encoded as UTF-8).
- `Array(Vec<PayloadValue>)` — heterogeneous ordered list.
- `Object(BTreeMap<String, PayloadValue>)` — nested keyed object.

#### Predicates

`is_null` / `is_bool` / `is_int` / `is_float` / `is_text` / `is_bytes` / `is_array` / `is_object` — match-free type checks.

#### Accessors

- `as_bool(&self) -> Option<bool>`
- `as_int(&self) -> Option<i64>`
- `as_float(&self) -> Option<f64>`
- `as_text(&self) -> Option<&str>`
- `as_bytes(&self) -> Option<&[u8]>`

#### `From<T>` conversions

`PayloadValue` implements `From<T>` for every scalar primitive that maps unambiguously: `bool`, `i32`, `i64`, `f32`, `f64`, `String`, `&str`, `Vec<u8>`, `Vec<PayloadValue>`, `BTreeMap<String, PayloadValue>`.

#### Example

```rust
use iqdb::PayloadValue;

let v = PayloadValue::from("hello");
assert!(v.is_text());
assert_eq!(v.as_text(), Some("hello"));

let n = PayloadValue::from(42_i64);
assert_eq!(n.as_int(), Some(42));

let f = PayloadValue::from(3.14_f64);
assert!(f.is_float());
```

<br>

### `RecordId`

Source: [`src/record.rs`](../src/record.rs)

Transparent newtype around `u64`. Cheap to copy (8 bytes), cheap to hash, stable across the wire under `serde`.

#### Methods

- `RecordId::new(id: u64) -> Self` — wrap a raw `u64`.
- `RecordId::get(self) -> u64` — unwrap.
- `From<u64>` / `Into<u64>` — bidirectional conversion.
- `Display` — writes the raw decimal.
- Implements `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `PartialOrd`, `Ord`.

#### Example

```rust
use iqdb::RecordId;

let id = RecordId::new(42);
assert_eq!(id.get(), 42);

let raw: u64 = id.into();
assert_eq!(raw, 42);

let back: RecordId = 42u64.into();
assert_eq!(back, id);

assert_eq!(format!("{id}"), "42");
```

<br>

### `Record`

Source: [`src/record.rs`](../src/record.rs)

The unit of read / write through the database handle — bundles `(RecordId, Vector, Option<Payload>)`. Two explicit constructors avoid the `Option`-typed argument pattern that degrades call-site readability.

#### Methods

- `Record::new(id: impl Into<RecordId>, vector: Vector) -> Self` — construct without metadata.
- `Record::with_payload(id: impl Into<RecordId>, vector: Vector, payload: Payload) -> Self` — construct with metadata.
- `Record::id(&self) -> RecordId` — identity.
- `Record::vector(&self) -> &Vector` — borrow the embedding.
- `Record::payload(&self) -> Option<&Payload>` — borrow the metadata.
- `Record::into_parts(self) -> (RecordId, Vector, Option<Payload>)` — decompose without a clone.

#### Example

```rust
use iqdb::{Payload, Record, RecordId, Vector};

let r = Record::new(RecordId::new(1), Vector::new(vec![1.0, 2.0])?);
assert_eq!(r.id().get(), 1);
assert!(r.payload().is_none());

let mut p = Payload::new();
p.insert("score", 0.97_f64);
let r2 = Record::with_payload(RecordId::new(2), Vector::new(vec![0.0, 1.0])?, p);
assert!(r2.payload().is_some());

let (id, vec, _payload) = r2.into_parts();
assert_eq!(id.get(), 2);
assert_eq!(vec.dim(), 2);
# Ok::<(), iqdb::Error>(())
```

<br>

### `SearchResult`

Source: [`src/search.rs`](../src/search.rs)

A single hit returned by similarity search.

#### Fields

- `id: RecordId` — identity of the matched record.
- `score: f32` — distance score under the active `DistanceMetric`. Smaller is closer; `NaN` when the metric is undefined for the matched pair (e.g. cosine against a zero vector).
- `payload: Option<Payload>` — cloned snapshot of the record's payload at search time.

Results returned by `Iqdb::search` / `search_with` / `search_batch` / `search_batch_with` are sorted by `score` ascending; ties break on `id` ascending; `NaN` scores sort to the tail.

#### Example

```rust
use iqdb::{DistanceMetric, Iqdb, Record, RecordId, Vector};

let db = Iqdb::open_in_memory();
db.upsert(Record::new(RecordId::new(1), Vector::new(vec![1.0, 0.0])?))?;
db.upsert(Record::new(RecordId::new(2), Vector::new(vec![0.0, 1.0])?))?;

let probe = Vector::new(vec![1.0, 0.0])?;
let hits = db.search(&probe, 2, DistanceMetric::Cosine)?;

assert_eq!(hits.len(), 2);
assert_eq!(hits[0].id, RecordId::new(1));
assert!(hits[0].score.abs() < 1e-6);
assert!(hits[1].score > hits[0].score);
# Ok::<(), iqdb::Error>(())
```

<br>

### `Error` / `Result`

Source: [`src/error.rs`](../src/error.rs)

```rust
pub type Result<T> = core::result::Result<T, Error>;
```

`Error` is the unified, `#[non_exhaustive]` error type. Always include a `_` arm when matching so new variants in future minor releases don't break call sites.

| Variant | Description |
|---------|-------------|
| `Error::Io(std::io::Error)` | A lower-level I/O failure occurred. Inspect the wrapped `ErrorKind` and decide retry / fallback / surface-to-user. `Error::source()` exposes the inner I/O error so report formatters can walk to the root cause. |
| `Error::InvalidConfig(&'static str)` | Configuration supplied at open time was invalid. Programmer error — fix the construction site. |
| `Error::InvalidVector { reason: &'static str }` | A vector failed boundary validation (empty input or non-finite component). Surfaced by `Vector::new` and `Vector::from_slice`. |
| `Error::DimensionMismatch { left: usize, right: usize }` | Two vectors of different dimensionality were combined. Surfaced by `DistanceMetric::distance` and the search methods. |
| `Error::Corrupt { reason: &'static str }` | On-disk data failed an integrity check during recovery — bad magic header, unknown format version, CRC mismatch, truncated frame. Surfaced by `Iqdb::open(path)`. The file-backed store stops replaying the WAL at the first corrupt frame and truncates the tail; this variant is only returned when the *snapshot* itself is corrupt. |
| `Error::NotImplemented` | Reserved for methods that defer their implementation to a later milestone. No public method on v0.4.0 returns this variant. |

`From<std::io::Error>` is implemented so the `?` operator threads I/O errors through naturally.

The `Display` impl deliberately redacts user-supplied content — e.g. the wrapped `std::io::Error`'s payload is replaced by its `ErrorKind`. Logs and error messages are safe to forward verbatim.

#### Example

```rust
use iqdb::{Error, Iqdb};

let db = Iqdb::open_in_memory();
let result = db.flush();
match result {
    Ok(()) => println!("flushed"),
    Err(Error::NotImplemented) => println!("backend has no flush"),
    Err(other) => return Err(other),
}
# Ok::<(), iqdb::Error>(())
```

<br>

## Durable Storage (v0.4.0)

`Iqdb::open(path)` is now load-bearing. The path is treated as a **directory**; iqdb owns two files inside it:

```text
<path>/
├── snap   — most recent durable snapshot (all records)
└── wal    — write-ahead log of changes since the snapshot
```

### Write path

Every `upsert` and `delete`:

1. Encodes the op into a framed binary entry (length prefix + body + CRC32 tail).
2. Appends the entry to the WAL file. **No `fsync` is performed at this point** — durability is on demand.
3. Applies the change to the in-memory map.

If the WAL append fails (`ENOSPC`, `EIO`, …) the in-memory map is not touched and the error is propagated. The caller decides whether to retry or surface to the user.

### Durability contract

A successful `upsert` followed by a successful `flush` is durable across a power cut. An `upsert` whose `flush` has not yet returned may be lost on a crash — the OS page cache holds the WAL bytes but they have not reached the platter / flash.

Per-write fsync (every `upsert` is durable before returning) is reserved for a later milestone — the implementation cost is the call itself; the milestone gate is benchmarking the tradeoff against expected workloads.

### Open / recover

On open, `iqdb`:

1. Reads `<path>/snap` into the in-memory map (if it exists).
2. Replays `<path>/wal` on top (if it exists).
3. Truncates the WAL to the last known-good offset (handles corrupt tails from prior crashes).

A corrupt frame mid-WAL stops replay — every record before it is preserved, anything after is discarded as un-recoverable. A corrupt snapshot fails the open with `Error::Corrupt`; the recovery story for that case is a future-milestone concern (snapshot version negotiation, fallback to an older snapshot on a checkpoint chain).

### Close / compact

`Iqdb::close` on a file-backed handle runs a **compaction**:

1. Acquire the write lock so the in-memory map cannot change.
2. Write a fresh snapshot to `<path>/snap.tmp`.
3. `full_sync` the snapshot file.
4. **Atomically** rename `snap.tmp` → `snap` (`MoveFileExW` on Windows, `rename(2)` on Unix).
5. Truncate the WAL to zero bytes.
6. `full_sync` the WAL.

After `close` returns, the on-disk state is the snapshot alone and the next open is a single-file load with no replay. If the process crashes between any of these steps, the next open sees a consistent state recoverable from the previous snapshot plus the un-truncated WAL.

### Platform-specific sync

| OS        | Primitive                                  |
|-----------|--------------------------------------------|
| Linux     | `fsync(2)`                                 |
| macOS     | `fcntl(fd, F_FULLFSYNC, 0)`                |
| Windows   | `FlushFileBuffers`                         |
| other Unix| `fsync(2)`                                 |

macOS is the only platform where `fsync` is not enough — the drive controller's write-back cache survives `fsync` but not power loss. `F_FULLFSYNC` asks the drive to drain that cache before returning. SQLite, Apple's own Core Data, and most embedded databases use it for the same reason.

### On-disk format

Both files use the same length-prefixed binary frame:

```text
+-----------------+----------------+
| payload_len: u32 LE              |
+-----------------+----------------+
| body bytes (op_kind + …)         |
+-----------------+----------------+
| crc32 over body: u32 LE          |
+-----------------+----------------+
```

The snapshot file additionally carries an 8-byte header — the ASCII bytes `"IQDB"` followed by a `u32 LE` format version. The WAL has no header; its frames stand alone and are versioned implicitly with the snapshot they accompany.

The encoding is little-endian on every platform regardless of host byte order, so a database written on x86_64 reads back identically on aarch64.

## Feature Flags

| Feature       | Default | Available     | Description                                                    |
|---------------|---------|---------------|----------------------------------------------------------------|
| `serde`       | off     | **shipping**  | Derives `Serialize` / `Deserialize` on every public data type. |
| `async`       | off     | planned v0.6.0 | Tokio-driven async mirror of the public API.                   |
| `mmap`        | off     | planned v0.4.0 | Memory-mapped read path for hot indices.                       |
| `io-uring`    | off     | planned v0.4.0 | Linux-only `io_uring` submission for batch writes.             |
| `full`        | off     | planned post-1.0 | All stable features in one switch.                          |

Feature flags are strictly additive (per REPS) — enabling any combination never removes or weakens existing functionality.

<br>

## Notes

- **Performance.** The flat search kernel is the correctness baseline. Approximate indices (IVF, HNSW) land in v0.5.0 and will sit alongside the flat kernel rather than replacing it — exact search remains the ground truth against which approximate recall is measured.
- **Concurrency.** `Iqdb` is `Send + Sync`. Both backends use `std::sync::RwLock<HashMap<…>>` for the in-memory map; the file-backed backend additionally serialises WAL appends behind a `Mutex`. All locks recover from poisoning (writer panic) so a panic in one operation does not propagate to unrelated readers.
- **Single-process safety.** v0.4.0 does **not** include a multi-process lockfile. Two processes opening the same path concurrently is undefined behaviour — the WAL writes interleave and the snapshot rename races. If you need multi-process safety today, place a lock around `Iqdb::open(path)` in your application; a built-in lockfile is a future-milestone concern.
- **Re-entrancy in filters.** The `search_with` / `search_batch_with` filters run while the backend's read lock is held. Do not call back into the same `Iqdb` handle from inside the filter — doing so risks a re-entrant lock acquisition.
- **NaN ordering.** Distance computations can produce `NaN` even when the inputs are finite (e.g. cosine against a zero vector). The search engine treats `NaN` scores as worst — they sort to the tail of the result list rather than corrupting the comparison chain.
- **Non-exhaustive types.** `Error`, `DistanceMetric`, and `PayloadValue` are all `#[non_exhaustive]`. Match-against-all-variants is a forward-compatibility hazard; always include a wildcard `_` arm.
- **Cross-platform on-disk format.** The binary format is little-endian on every platform. A database written on x86_64 reads back identically on aarch64; a database copied between Linux, macOS, and Windows hosts (same architecture) opens cleanly on each.

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
