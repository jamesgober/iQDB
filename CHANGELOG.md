# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] — 2026-05-30

### Added

- [`Iqdb::search`](./src/db.rs) — exact top-`k` similarity search. Returns `Vec<SearchResult>` sorted ascending by `score` under the smaller-is-closer convention, with `id` as the deterministic tie-breaker. The kernel is a brute-force flat scan with a bounded top-`k` heap (`O(N · D + N · log k)` for `N` records of dimensionality `D`); approximate indices in v0.5.0 will sit alongside the flat kernel rather than replacing it.
- [`Iqdb::search_with`](./src/db.rs) — predicate-filtered top-`k`. The `Fn(&Record) -> bool` filter is monomorphised into the search loop — no per-record dynamic dispatch. Records that fail the predicate are excluded from heap admission, so the filter composes cleanly with the distance metric.
- [`Iqdb::search_batch`](./src/db.rs) — sequential batch search. `output[i]` is the top-`k` for `queries[i]`; input order is preserved.
- [`Iqdb::search_batch_with`](./src/db.rs) — batch search with a shared filter applied to every query.
- [`SearchResult`](./src/search.rs) — `{ id: RecordId, score: f32, payload: Option<Payload> }`. The payload field carries a clone of the record's metadata at search time so callers do not need a follow-up `get`. Behind the `serde` feature, derives `Serialize` / `Deserialize`.
- Crate-internal flat-search kernel in [`src/search.rs`](./src/search.rs) — uses a bounded `BinaryHeap<HeapEntry>` with `HeapEntry { score, id }` (the payload clone is deferred until the heap settles to its `k` survivors) and a NaN-aware total order with id tie-break. `NaN` scores (from cosine against zero vectors) sort to the tail of the result list rather than corrupting the comparison chain.
- `MemoryStore::with_records` — scoped read-lock access for the search kernel; the closure receives the underlying `HashMap` borrow and the lock is released as soon as it returns.
- Property-based test suite at [`tests/properties.rs`](./tests/properties.rs) — 8 `proptest`-driven properties covering distance-metric algebra (identity, symmetry, L2 non-negativity, cosine in `[0, 2]`) and search-ranking invariants (length bound, ascending order, perfect-match presence, no-filter parity with always-true filter).
- Integration test suite at [`tests/search.rs`](./tests/search.rs) — 14 tests covering the four public entry points: top-`k` ordering, `k = 0` short-circuit, `k > store.len()` cap, filter pruning, empty filter result, dimension-mismatch propagation, payload preservation, payload-absent fallback, batch input-order preservation, batch on empty store, shared filter across batch, and concurrent reader safety through `Arc<Iqdb>`.
- Search bench group in [`benches/vector_ops.rs`](./benches/vector_ops.rs) — three variants (`flat_k10_dim128`, `flat_k10_dim128_filter_half`, `batch4_k10_dim128`) at 1 000 and 10 000 records.
- New example [`examples/search.rs`](./examples/search.rs) — walk-through of unfiltered cosine search, payload-filtered search, and a 3-probe batch.
- Full API reference at [`docs/API.md`](./docs/API.md) — every public type, method, error variant, and feature flag is documented with parameter descriptions and runnable examples, following the metrics-lib API.md format.
- `proptest = "1"` dev-dependency (default-features off, `std` feature) for the property tests. Pinned to the 1.x line for MSRV 1.75 compatibility.

### Changed

- `examples/basic.rs` — unchanged structurally; still exercises the lifecycle + a single upsert/get round-trip.
- Crate-level rustdoc in `src/lib.rs` — the second runnable example now demonstrates filtered top-`k` search instead of standalone distance computation, so the rustdoc surface always exercises the latest milestone.
- README — `Quick Start` rewritten around `Iqdb::search`; `Filtered and batch search` subsection added; `API Overview` now points to `docs/API.md` as the canonical reference; benchmark and testing sections updated for the new groups and the property tests.

### Removed

Nothing removed in v0.3.0 — the surface is additive on top of v0.2.0.

[Unreleased]: https://github.com/jamesgober/iqdb/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/jamesgober/iqdb/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/jamesgober/iqdb/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/iqdb/releases/tag/v0.1.0
