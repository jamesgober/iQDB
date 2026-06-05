# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/jamesgober/iqdb/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/jamesgober/iqdb/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/jamesgober/iqdb/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/jamesgober/iqdb/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/iqdb/releases/tag/v0.1.0
