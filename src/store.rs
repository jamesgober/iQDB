// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! In-memory record store.
//!
//! The store is the backend behind [`Iqdb::open_in_memory`]: a
//! `RwLock<HashMap<RecordId, Record>>` keyed by caller-supplied
//! [`RecordId`]. Writers take the write lock; readers take the read
//! lock and clone the matched record out, releasing the lock before
//! handing the clone back. The clone-on-read pattern keeps borrowed
//! references from extending the lock guard's lifetime — a common
//! source of deadlocks when callers compose store reads with other
//! locked structures.
//!
//! The store is the single source of truth for v0.2.0 — durable
//! file-backed storage (`v0.4.0`) and the index layer (`v0.3.0` /
//! `v0.5.0`) consume it as a substrate rather than replacing it.
//! Until that lands, [`Iqdb::open(path)`] returns
//! [`Error::NotImplemented`].
//!
//! [`Iqdb::open_in_memory`]: crate::Iqdb::open_in_memory
//! [`Iqdb::open(path)`]: crate::Iqdb::open
//! [`Error::NotImplemented`]: crate::Error::NotImplemented

use std::collections::HashMap;
use std::sync::RwLock;

use crate::error::Result;
use crate::record::{Record, RecordId};

/// In-memory record store backing [`Iqdb::open_in_memory`].
///
/// Thread-safe — wrap in `Arc<MemoryStore>` to share across tasks.
/// Reads acquire a shared read lock; writes acquire the exclusive
/// write lock. The lock is `std::sync::RwLock`, which is adequate for
/// the v0.2.0 single-process workload; switching to a sharded or
/// fully lock-free structure is on the table for `v0.3.0` once the
/// search-engine hot path is characterised.
///
/// [`Iqdb::open_in_memory`]: crate::Iqdb::open_in_memory
#[derive(Debug, Default)]
pub(crate) struct MemoryStore {
    records: RwLock<HashMap<RecordId, Record>>,
}

impl MemoryStore {
    /// Construct an empty in-memory store.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Insert or replace `record`.
    ///
    /// Returns `Ok(())` whether the id was previously present or not —
    /// upsert semantics. The unconditional return preserves the call
    /// site shape that the durable backend will adopt in `v0.4.0`,
    /// where partial writes can fail mid-operation.
    pub(crate) fn upsert(&self, record: Record) -> Result<()> {
        let id = record.id();
        let mut guard = self.write();
        let _previous = guard.insert(id, record);
        Ok(())
    }

    /// Look up by id.
    ///
    /// The matched `Record` is cloned out so the read lock can be
    /// released before the value is returned. Returns `Ok(None)`
    /// when the id is absent.
    pub(crate) fn get(&self, id: RecordId) -> Result<Option<Record>> {
        let guard = self.read();
        Ok(guard.get(&id).cloned())
    }

    /// Remove by id, returning whether the id was present.
    pub(crate) fn delete(&self, id: RecordId) -> Result<bool> {
        let mut guard = self.write();
        Ok(guard.remove(&id).is_some())
    }

    /// Number of records currently stored.
    pub(crate) fn len(&self) -> usize {
        self.read().len()
    }

    /// `true` if no records are stored.
    pub(crate) fn is_empty(&self) -> bool {
        self.read().is_empty()
    }

    /// Run a closure against the entire record map under the read
    /// lock.
    ///
    /// The closure receives a borrowed `HashMap<RecordId, Record>`
    /// and returns whatever the caller needs out of it. The read
    /// lock is released as soon as the closure returns.
    ///
    /// **Important:** the closure must not call back into this store
    /// (or any other operation that acquires this lock) — doing so
    /// risks a re-entrant lock acquisition that the standard library
    /// does not promise to handle.
    pub(crate) fn with_records<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&HashMap<RecordId, Record>) -> R,
    {
        let guard = self.read();
        f(&guard)
    }

    /// Acquire the read lock, recovering from poisoning.
    ///
    /// Poisoning happens when a writer panics while holding the
    /// write lock. The store's invariants are simple enough that a
    /// poisoned map is still safe to read — we proceed with the
    /// inner guard rather than propagating the panic. This matches
    /// the REPS *Failure Modes & Degradation* rule: a panic in one
    /// subsystem must not propagate unchecked into unrelated ones.
    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<RecordId, Record>> {
        match self.records.read() {
            Ok(guard) => guard,
            Err(poison) => poison.into_inner(),
        }
    }

    /// Acquire the write lock, recovering from poisoning.
    ///
    /// See [`Self::read`] for the rationale.
    fn write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<RecordId, Record>> {
        match self.records.write() {
            Ok(guard) => guard,
            Err(poison) => poison.into_inner(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::Vector;

    fn record(id: u64, components: Vec<f32>) -> Record {
        Record::new(RecordId::new(id), Vector::new(components).unwrap())
    }

    #[test]
    fn new_store_is_empty() {
        let s = MemoryStore::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn upsert_inserts_new_record() {
        let s = MemoryStore::new();
        s.upsert(record(1, vec![0.1, 0.2])).unwrap();
        assert_eq!(s.len(), 1);
        let got = s.get(RecordId::new(1)).unwrap().expect("present");
        assert_eq!(got.vector().as_slice(), &[0.1, 0.2]);
    }

    #[test]
    fn upsert_replaces_existing_record() {
        let s = MemoryStore::new();
        s.upsert(record(1, vec![0.1, 0.2])).unwrap();
        s.upsert(record(1, vec![0.9, 0.8])).unwrap();
        assert_eq!(s.len(), 1);
        let got = s.get(RecordId::new(1)).unwrap().expect("present");
        assert_eq!(got.vector().as_slice(), &[0.9, 0.8]);
    }

    #[test]
    fn get_returns_none_for_missing_id() {
        let s = MemoryStore::new();
        assert!(s.get(RecordId::new(99)).unwrap().is_none());
    }

    #[test]
    fn delete_removes_existing_record() {
        let s = MemoryStore::new();
        s.upsert(record(1, vec![0.1, 0.2])).unwrap();
        assert!(s.delete(RecordId::new(1)).unwrap());
        assert!(s.is_empty());
    }

    #[test]
    fn delete_returns_false_when_absent() {
        let s = MemoryStore::new();
        assert!(!s.delete(RecordId::new(99)).unwrap());
    }

    #[test]
    fn len_reflects_distinct_ids() {
        let s = MemoryStore::new();
        s.upsert(record(1, vec![0.0])).unwrap();
        s.upsert(record(2, vec![0.0])).unwrap();
        s.upsert(record(3, vec![0.0])).unwrap();
        // Replacing #2 does not increase the count.
        s.upsert(record(2, vec![1.0])).unwrap();
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn store_is_thread_safe_across_arc() {
        use std::sync::Arc;
        use std::thread;

        let store = Arc::new(MemoryStore::new());
        let mut handles = Vec::new();
        for id in 0..16_u64 {
            let s = Arc::clone(&store);
            handles.push(thread::spawn(move || {
                s.upsert(record(id, vec![id as f32; 4])).unwrap();
            }));
        }
        for h in handles {
            h.join().expect("worker thread panicked");
        }
        assert_eq!(store.len(), 16);
        // Spot-check that every inserted id is readable.
        for id in 0..16 {
            let got = store
                .get(RecordId::new(id))
                .unwrap()
                .expect("record present");
            assert_eq!(got.id().get(), id);
        }
    }

    #[test]
    fn store_recovers_from_poisoned_lock() {
        use std::sync::Arc;
        use std::thread;

        let store = Arc::new(MemoryStore::new());
        store.upsert(record(1, vec![0.1, 0.2])).unwrap();

        // Poison the lock by panicking inside a writer.
        let poisoner = Arc::clone(&store);
        let _ = thread::spawn(move || {
            let _guard = poisoner.records.write().unwrap();
            panic!("intentional");
        })
        .join();

        assert!(store.records.is_poisoned());
        // Subsequent reads recover via `read()` / `write()`.
        let got = store.get(RecordId::new(1)).unwrap();
        assert!(got.is_some());
    }
}
