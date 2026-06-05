// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Backend dispatch for the [`Iqdb`](crate::Iqdb) handle.
//!
//! [`Backend`] is a small `pub(crate)` enum that lets the public
//! handle dispatch CRUD and `with_records` calls to either the
//! in-memory store or the file-backed store without going through a
//! `Box<dyn Trait>`. Enum dispatch keeps the hot paths
//! (`with_records` → `flat_search`) free of dynamic dispatch — the
//! compiler can inline the match arm into the calling function and
//! the inner search loop sees a concrete `HashMap` borrow with no
//! virtual indirection.
//!
//! The two backends share an identical `with_records` shape so the
//! search kernel in [`crate::search`] binds to both via the same
//! match.

use std::collections::HashMap;

use crate::error::Result;
use crate::file_store::FileStore;
use crate::record::{Record, RecordId};
use crate::store::MemoryStore;

/// Active storage backend for an open [`Iqdb`](crate::Iqdb) handle.
///
/// Constructed by [`Iqdb::open_in_memory`](crate::Iqdb::open_in_memory)
/// (which produces [`Backend::Memory`]) and
/// [`Iqdb::open`](crate::Iqdb::open) (which produces [`Backend::File`]).
/// The enum is intentionally kept inside the crate; widening it to
/// `pub` would commit the public API to a specific set of backends
/// before the async / mmap milestones land.
#[derive(Debug)]
pub(crate) enum Backend {
    /// Ephemeral, in-memory backend with no durable substrate.
    Memory(MemoryStore),
    /// Directory-backed durable store (snapshot + WAL).
    File(FileStore),
}

impl Backend {
    pub(crate) fn upsert(&self, record: Record) -> Result<()> {
        match self {
            Self::Memory(store) => store.upsert(record),
            Self::File(store) => store.upsert(record),
        }
    }

    pub(crate) fn get(&self, id: RecordId) -> Result<Option<Record>> {
        match self {
            Self::Memory(store) => store.get(id),
            Self::File(store) => store.get(id),
        }
    }

    pub(crate) fn delete(&self, id: RecordId) -> Result<bool> {
        match self {
            Self::Memory(store) => store.delete(id),
            Self::File(store) => store.delete(id),
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Memory(store) => store.len(),
            Self::File(store) => store.len(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Self::Memory(store) => store.is_empty(),
            Self::File(store) => store.is_empty(),
        }
    }

    /// Run `f` under the backend's read lock.
    ///
    /// Both backends use the same `RwLock<HashMap<…>>` shape under the
    /// hood, so the closure can be reused verbatim. The lock is
    /// released as soon as `f` returns.
    pub(crate) fn with_records<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&HashMap<RecordId, Record>) -> R,
    {
        match self {
            Self::Memory(store) => store.with_records(f),
            Self::File(store) => store.with_records(f),
        }
    }

    /// Drive any backend-specific buffers to durable storage.
    ///
    /// In-memory backend: returns `Ok(())` immediately — there is no
    /// durable substrate, so "flush" is a no-op and reporting success
    /// is more honest than reporting `NotImplemented` (the data is
    /// already as durable as a memory-only backend can be).
    ///
    /// File backend: calls
    /// [`platform::full_sync`](crate::platform::full_sync) on the WAL
    /// — `F_FULLFSYNC` on macOS, `fsync(2)` on other Unix,
    /// `FlushFileBuffers` on Windows.
    pub(crate) fn flush(&self) -> Result<()> {
        match self {
            Self::Memory(_) => Ok(()),
            Self::File(store) => store.flush(),
        }
    }

    /// Cleanly close the backend, running any backend-specific
    /// shutdown work.
    ///
    /// In-memory backend: nothing to do — the `HashMap` drops with
    /// the handle.
    ///
    /// File backend: triggers a compaction — writes a fresh snapshot,
    /// atomically replaces the old one, truncates the WAL. After
    /// `close` returns, the on-disk state is the snapshot alone; the
    /// next open is a single-file load with no WAL replay.
    pub(crate) fn close(self) -> Result<()> {
        match self {
            Self::Memory(_) => Ok(()),
            Self::File(store) => store.compact(),
        }
    }
}
