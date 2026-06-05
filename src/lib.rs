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
//! The `0.4.0` release adds **durable file-backed storage**:
//! [`Iqdb::open(path)`] now opens or creates a directory-backed
//! database with a snapshot file (`<path>/snap`) and a write-ahead
//! log (`<path>/wal`). [`Iqdb::flush`] drives the WAL through the
//! strongest sync primitive each platform offers (`F_FULLFSYNC` on
//! macOS, `fsync(2)` on other Unix, `FlushFileBuffers` on Windows).
//! [`Iqdb::close`] runs a compaction — writes a fresh snapshot,
//! atomically replaces the old one, truncates the WAL — so the next
//! open is a single-file load with no replay. Recovery handles
//! corrupted WAL tails by truncating to the last known-good offset.
//!
//! The v0.3.0 surface (CRUD, top-`k` search, filters, batch) is
//! unchanged — every method dispatches through a `pub(crate)`
//! `Backend` enum so the in-memory and file-backed paths share the
//! same public API. Approximate indices (IVF, HNSW) follow in v0.5.0
//! and will sit alongside the flat kernel rather than replacing it.
//!
//! Enable the optional `serde` Cargo feature to derive
//! `Serialize` / `Deserialize` on [`Vector`], [`Payload`],
//! [`PayloadValue`], [`RecordId`], [`Record`], and [`DistanceMetric`].
//! The default build pulls no runtime dependencies.
//!
//! [`sqlite`]: https://www.sqlite.org/
//! [`redb`]: https://crates.io/crates/redb
//! [`Iqdb::open(path)`]: Iqdb::open
//!
//! # Examples
//!
//! Open an in-memory instance, upsert a record, look it up, and close
//! the handle:
//!
//! ```
//! use iqdb::{Iqdb, Record, RecordId, Result, Vector};
//!
//! fn run() -> Result<()> {
//!     let db = Iqdb::open_in_memory();
//!
//!     db.upsert(Record::new(
//!         RecordId::new(1),
//!         Vector::new(vec![0.1, 0.2, 0.3])?,
//!     ))?;
//!
//!     let hit = db.get(RecordId::new(1))?.expect("record present");
//!     assert_eq!(hit.vector().as_slice(), &[0.1, 0.2, 0.3]);
//!
//!     db.close()?;
//!     Ok(())
//! }
//! # run().unwrap();
//! ```
//!
//! Run a filtered top-`k` similarity search — the filter narrows the
//! candidate set before the bounded heap admit decision, so payload
//! predicates compose cleanly with the distance metric:
//!
//! ```
//! use iqdb::{DistanceMetric, Iqdb, Payload, PayloadValue, Record, RecordId, Result, Vector};
//!
//! fn run() -> Result<()> {
//!     let db = Iqdb::open_in_memory();
//!
//!     let mut doc = Payload::new();
//!     doc.insert("kind", "doc");
//!     db.upsert(Record::with_payload(
//!         RecordId::new(1),
//!         Vector::new(vec![1.0, 0.0, 0.0])?,
//!         doc,
//!     ))?;
//!
//!     let mut image = Payload::new();
//!     image.insert("kind", "image");
//!     db.upsert(Record::with_payload(
//!         RecordId::new(2),
//!         Vector::new(vec![1.0, 0.01, 0.0])?,
//!         image,
//!     ))?;
//!
//!     let probe = Vector::new(vec![1.0, 0.0, 0.0])?;
//!     let hits = db.search_with(&probe, 5, DistanceMetric::Cosine, |rec| {
//!         rec.payload()
//!             .and_then(|p| p.get("kind"))
//!             .and_then(PayloadValue::as_text)
//!             == Some("doc")
//!     })?;
//!
//!     assert_eq!(hits.len(), 1);
//!     assert_eq!(hits[0].id, RecordId::new(1));
//!     Ok(())
//! }
//! # run().unwrap();
//! ```
//!
//! Open a directory-backed durable database, write a record, sync to
//! disk, and close cleanly. The directory is created if it does not
//! exist; the snapshot + WAL pair inside it survives process restarts:
//!
//! ```no_run
//! use iqdb::{Iqdb, Record, RecordId, Result, Vector};
//!
//! fn run() -> Result<()> {
//!     let db = Iqdb::open("./data/my-db")?;
//!     db.upsert(Record::new(
//!         RecordId::new(1),
//!         Vector::new(vec![0.1, 0.2, 0.3])?,
//!     ))?;
//!     db.flush()?; // F_FULLFSYNC on macOS, fsync on other unix, FlushFileBuffers on Windows
//!     db.close()  // runs a final compaction: snapshot rewrite + WAL truncate
//! }
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
// Test code is allowed to use the convenience panickers — the strict
// lint profile above is for production library code, not assertion
// scaffolding inside `#[cfg(test)] mod tests` blocks.
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

mod backend;
mod codec;
mod db;
mod error;
mod file_store;
mod payload;
mod platform;
mod record;
mod search;
pub(crate) mod store;
mod vector;

pub use db::Iqdb;
pub use error::{Error, Result};
pub use payload::{Payload, PayloadValue};
pub use record::{Record, RecordId};
pub use search::SearchResult;
pub use vector::{DistanceMetric, Vector};
