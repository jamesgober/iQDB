// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Public handle for the `iqdb` embedded vector database.
//!
//! Every method on [`Iqdb`] is currently a stub. The
//! shape of the public API is intentionally minimal — `open` /
//! `open_in_memory` / `flush` / `close` cover the lifecycle most
//! embedded DB consumers expect, without committing to a query surface
//! that varies by DB kind. Implement real bodies as the storage engine
//! comes online; widen the surface (`insert`, `get`, `range`, etc.)
//! once the underlying data model is settled.

use std::path::Path;

use crate::error::{Error, Result};

/// Top-level handle for an open `iqdb` database.
///
/// Construct one with [`Iqdb::open`] for a file-backed
/// store or [`Iqdb::open_in_memory`] for an ephemeral
/// instance.
#[derive(Debug)]
pub struct Iqdb {
    _private: (),
}

impl Iqdb {
    /// Open the database at the given path.
    ///
    /// # Errors
    ///
    /// Currently always returns [`Error::NotImplemented`]; replace with
    /// real open-path logic once the storage engine is wired up.
    pub fn open<P: AsRef<Path>>(_path: P) -> Result<Self> {
        Err(Error::NotImplemented)
    }

    /// Open an ephemeral, in-memory instance.
    ///
    /// In-memory instances do not touch disk and are dropped when the
    /// handle is dropped.
    pub fn open_in_memory() -> Self {
        Self { _private: () }
    }

    /// Flush all pending writes to durable storage.
    ///
    /// # Errors
    ///
    /// Currently always returns [`Error::NotImplemented`]; replace with
    /// the real durability path once the storage engine is wired up.
    pub fn flush(&self) -> Result<()> {
        Err(Error::NotImplemented)
    }

    /// Close the database handle, releasing any held resources.
    ///
    /// # Errors
    ///
    /// Currently always succeeds; widen as needed when close has real
    /// work to do (sync, advisory-lock release, etc.).
    pub fn close(self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_returns_handle() {
        let _db = Iqdb::open_in_memory();
    }

    #[test]
    fn open_returns_not_implemented() {
        let result = Iqdb::open("/tmp/iqdb-test");
        assert!(matches!(result, Err(Error::NotImplemented)));
    }

    #[test]
    fn flush_returns_not_implemented() {
        let db = Iqdb::open_in_memory();
        let result = db.flush();
        assert!(matches!(result, Err(Error::NotImplemented)));
    }

    #[test]
    fn close_succeeds() {
        let db = Iqdb::open_in_memory();
        assert!(db.close().is_ok());
    }
}
