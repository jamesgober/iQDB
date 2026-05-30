// Copyright 2026 James Gober.
// Licensed under Apache-2.0 OR MIT.

//! # iqdb — embedded vector database for Rust
//!
//! `iqdb` is a single-process, in-application similarity-search engine
//! designed for high-dimensional workloads where every microsecond on
//! the query path matters. It targets the same operational shape as
//! [`sqlite`] or [`redb`]: no daemon, no network hop, no separate
//! runtime. Open a handle, write vectors, query nearest neighbours —
//! all from inside your binary.
//!
//! The `0.1.0` release is the **scaffolding milestone**. The crate
//! compiles, lints clean at the strict REPS profile, and passes the
//! full test matrix on Linux, macOS, and Windows. The public lifecycle
//! handle ([`Iqdb::open`], [`Iqdb::open_in_memory`], [`Iqdb::flush`],
//! [`Iqdb::close`]) and the [`Error`] / [`Result`] types are in place.
//! The query verbs ([`upsert`], [`search`], [`delete`]) land in
//! subsequent milestones — every stub returns [`Error::NotImplemented`]
//! until the engine is wired underneath it. The API is **unstable**
//! until 1.0.
//!
//! See the repository [`README`] and [`CHANGELOG`] for the
//! release-by-release surface and the milestone roadmap.
//!
//! [`sqlite`]: https://www.sqlite.org/
//! [`redb`]: https://crates.io/crates/redb
//! [`upsert`]: Iqdb
//! [`search`]: Iqdb
//! [`delete`]: Iqdb
//! [`README`]: https://github.com/jamesgober/iqdb/blob/main/README.md
//! [`CHANGELOG`]: https://github.com/jamesgober/iqdb/blob/main/CHANGELOG.md
//!
//! # Examples
//!
//! Open an ephemeral, in-memory instance and close it cleanly:
//!
//! ```
//! use iqdb::{Iqdb, Result};
//!
//! fn run() -> Result<()> {
//!     let db = Iqdb::open_in_memory();
//!     db.close()?;
//!     Ok(())
//! }
//! # run().unwrap();
//! ```
//!
//! Match on the staged surface so call sites can be wired ahead of the
//! engine landing — the [`Error::NotImplemented`] branch disappears
//! once the corresponding milestone ships:
//!
//! ```
//! use iqdb::{Error, Iqdb, Result};
//!
//! fn flush_if_supported(db: &Iqdb) -> Result<()> {
//!     match db.flush() {
//!         Ok(()) => Ok(()),
//!         Err(Error::NotImplemented) => Ok(()),
//!         Err(err) => Err(err),
//!     }
//! }
//!
//! let db = Iqdb::open_in_memory();
//! flush_if_supported(&db).unwrap();
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
// Test code is allowed to use the convenience panickers — the strict
// lint profile above is for production library code, not assertion
// scaffolding inside `#[cfg(test)] mod tests` blocks.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::print_stdout,
        clippy::print_stderr
    )
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod db;
mod error;

pub use db::Iqdb;
pub use error::{Error, Result};
