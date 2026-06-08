// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! The authoritative row store.
//!
//! `iqdb` owns the vectors. The approximate index implementations
//! (`iqdb-hnsw`, `iqdb-ivf`) cannot be serialized — their internal graph and
//! cluster state is private and the [`IndexCore`](iqdb_index::IndexCore)
//! trait exposes no way to enumerate stored vectors — so durability and
//! index rebuilds both read from this store, never from the derived index.
//!
//! It is a thin, insertion-ordered map from [`VectorId`] to its
//! `(Arc<[f32]>, Option<Metadata>)` row. Insertion order is preserved so a
//! save → load → rebuild cycle replays inserts in a deterministic order
//! (the family indices assign tie-break sequence numbers by insert order).

use std::collections::HashMap;
use std::sync::Arc;

use iqdb_types::{Metadata, VectorId};

/// One stored vector: its id, payload, and optional metadata.
///
/// The payload is an [`Arc<[f32]>`] so the row shares a single allocation
/// with the derived index — no copy when a vector lives in both.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    pub(crate) id: VectorId,
    pub(crate) vector: Arc<[f32]>,
    pub(crate) meta: Option<Metadata>,
}

/// An insertion-ordered, id-keyed store of vector rows.
#[derive(Debug, Default)]
pub(crate) struct RowStore {
    rows: Vec<Row>,
    index: HashMap<VectorId, usize>,
}

impl RowStore {
    /// An empty store.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// An empty store pre-sized for `cap` rows.
    pub(crate) fn with_capacity(cap: usize) -> Self {
        Self {
            rows: Vec::with_capacity(cap),
            index: HashMap::with_capacity(cap),
        }
    }

    /// Number of live rows.
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    /// `true` if the store holds no rows.
    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// `true` if `id` is present.
    pub(crate) fn contains(&self, id: &VectorId) -> bool {
        self.index.contains_key(id)
    }

    /// Borrow the row for `id`, if present.
    pub(crate) fn get(&self, id: &VectorId) -> Option<&Row> {
        self.index.get(id).map(|&pos| &self.rows[pos])
    }

    /// Insert or replace the row for `id`. Returns `true` if the id was newly
    /// inserted, `false` if an existing row was replaced in place (preserving
    /// its position so the iteration order stays stable across upserts).
    pub(crate) fn upsert(
        &mut self,
        id: VectorId,
        vector: Arc<[f32]>,
        meta: Option<Metadata>,
    ) -> bool {
        if let Some(&pos) = self.index.get(&id) {
            self.rows[pos] = Row { id, vector, meta };
            false
        } else {
            let pos = self.rows.len();
            let _ = self.index.insert(id.clone(), pos);
            self.rows.push(Row { id, vector, meta });
            true
        }
    }

    /// Remove the row for `id`. Returns `true` if a row was removed, `false`
    /// if `id` was already absent.
    pub(crate) fn remove(&mut self, id: &VectorId) -> bool {
        let Some(pos) = self.index.remove(id) else {
            return false;
        };
        let _removed = self.rows.swap_remove(pos);
        // `swap_remove` moved the last row into `pos` (unless we removed the
        // last row); repoint its index entry.
        if pos < self.rows.len() {
            let moved_id = self.rows[pos].id.clone();
            let _ = self.index.insert(moved_id, pos);
        }
        true
    }

    /// Iterate the rows in storage order.
    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = &Row> {
        self.rows.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(xs: &[f32]) -> Arc<[f32]> {
        Arc::from(xs)
    }

    #[test]
    fn upsert_insert_then_replace_keeps_len_and_position() {
        let mut s = RowStore::new();
        assert!(s.upsert(VectorId::from(1u64), v(&[1.0, 0.0]), None));
        assert!(s.upsert(VectorId::from(2u64), v(&[0.0, 1.0]), None));
        assert_eq!(s.len(), 2);

        // Replacing id 1 keeps len at 2 and does not move it to the end.
        assert!(!s.upsert(VectorId::from(1u64), v(&[2.0, 2.0]), None));
        assert_eq!(s.len(), 2);
        let order: Vec<_> = s.iter().map(|r| r.id.clone()).collect();
        assert_eq!(order, vec![VectorId::from(1u64), VectorId::from(2u64)]);
        assert_eq!(
            s.get(&VectorId::from(1u64)).unwrap().vector.as_ref(),
            &[2.0, 2.0]
        );
    }

    #[test]
    fn remove_returns_false_when_absent_and_repairs_index() {
        let mut s = RowStore::new();
        assert!(s.upsert(VectorId::from(1u64), v(&[1.0]), None));
        assert!(s.upsert(VectorId::from(2u64), v(&[2.0]), None));
        assert!(s.upsert(VectorId::from(3u64), v(&[3.0]), None));

        // Remove the middle id; the moved (last) row must still be findable.
        assert!(s.remove(&VectorId::from(2u64)));
        assert!(!s.remove(&VectorId::from(2u64)));
        assert_eq!(s.len(), 2);
        assert_eq!(
            s.get(&VectorId::from(3u64)).unwrap().vector.as_ref(),
            &[3.0]
        );
        assert!(!s.contains(&VectorId::from(2u64)));
    }

    #[test]
    fn get_absent_is_none() {
        let s = RowStore::new();
        assert!(s.get(&VectorId::from(9u64)).is_none());
        assert!(s.is_empty());
    }
}
