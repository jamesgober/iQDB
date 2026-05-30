# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] — 2026-05-28

### Added

- Initial public crate surface. The top-level [`Iqdb`](./src/db.rs) handle exposes the database lifecycle — `open`, `open_in_memory`, `flush`, and `close` — with an in-memory backend wired through and a path-backed backend stubbed for the durable-storage milestone.
- Unified error type [`Error`](./src/error.rs) with `Io`, `InvalidConfig`, and `NotImplemented` variants. `#[non_exhaustive]` so new failure modes can land without a breaking change. `Result<T>` alias for `core::result::Result<T, Error>`.
- Crate-level documentation with runnable examples for both the in-memory lifecycle and the staged-surface error pattern (matching on `Error::NotImplemented` so call sites can be wired ahead of the engine landing).
- Strict REPS lint profile at the crate root (`#![deny(warnings)]`, `#![deny(missing_docs)]`, `#![deny(unsafe_op_in_unsafe_fn)]`, `#![deny(unused_must_use)]`, `#![deny(unused_results)]`, plus the full Clippy correctness deny set).
- Integration test scaffold at [`tests/smoke.rs`](./tests/smoke.rs) exercising the in-memory lifecycle.
- Criterion benchmark harness at [`benches/scaffold.rs`](./benches/scaffold.rs) — placeholder workload that proves the harness wires up; real benches land per milestone.
- Runnable example at [`examples/basic.rs`](./examples/basic.rs).
- Cross-platform CI matrix in [`.github/workflows/ci.yml`](./.github/workflows/ci.yml) covering build + test on Linux, macOS, and Windows; MSRV (1.75) verification; `rustfmt` / `clippy` / `rustdoc` lint gates; `cargo audit` security scan; `cargo deny` dependency-policy gate.
- Dependency-policy configuration at [`deny.toml`](./deny.toml) — MIT / Apache-2.0 family permissive licenses allowed, copyleft licenses denied, wildcard versions rejected, yanked crates surfaced as warnings.
- `Cargo.toml` metadata complete: description, keywords (`database`, `embedded`, `vector`, `similarity-search`, `ann`), categories, repository / homepage / documentation URLs, MSRV pin, dual-license declaration, docs.rs `all-features` configuration.
- Release profile tuned for embedded-DB hot paths: `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, symbols stripped. Bench profile mirrors the release optimisation flags so Criterion measures realistic numbers.
- Project standards published at [`REPS.md`](./REPS.md) — the Rust Efficiency & Performance Standards every contribution is held to.

[Unreleased]: https://github.com/jamesgober/iqdb/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/jamesgober/iqdb/releases/tag/v0.1.0
