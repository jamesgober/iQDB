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
//! The `0.3.0` release adds **exact top-`k` similarity search** on
//! top of the v0.2.0 primitives: [`Iqdb::search`], [`Iqdb::search_with`]
//! (predicate-filtered), [`Iqdb::search_batch`], and
//! [`Iqdb::search_batch_with`], all returning ordered
//! [`SearchResult`]s. The kernel is a brute-force flat scan with a
//! bounded top-`k` heap; approximate indices (IVF, HNSW) follow in
//! v0.5.0 and will sit alongside the flat kernel rather than
//! replacing it.
//!
//! Durable file-backed storage lands in v0.4.0. Until then,
//! [`Iqdb::open(path)`] and [`Iqdb::flush`] return
//! [`Error::NotImplemented`] so call sites can be wired against the
//! final API shape today.
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
//! Branch on [`Error::NotImplemented`] when wiring methods whose
//! engine path lands in a later milestone — the `Err` arm disappears
//! when the corresponding release ships:
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
        unused_results,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::print_stdout,
        clippy::print_stderr
    )
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod db;
mod error;
mod payload;
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
