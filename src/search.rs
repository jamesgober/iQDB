// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Flat (exact, brute-force) top-`k` similarity search.
//!
//! The `flat_search` function backs [`Iqdb::search`] and friends: for
//! every record in the store, compute the configured
//! [`DistanceMetric`], and keep the `k` smallest scores in a
//! cache-friendly bounded heap. Asymptotic cost is `O(N · D)` for `N`
//! records of dimensionality `D`, plus `O(N · log k)` for the heap
//! maintenance.
//!
//! The algorithm is exact — every record is compared. Approximate
//! indices (IVF, HNSW) land in v0.5.0 and will live alongside this
//! kernel rather than replacing it; "compare against everything" is
//! always the correctness baseline.
//!
//! ## Ordering
//!
//! Every [`DistanceMetric`] returns its values under the
//! **smaller-is-closer** convention (the `Dot` variant returns
//! `−(a · b)` for exactly this reason). The ranking sort puts the
//! smallest scores first.
//!
//! ## NaN handling
//!
//! Distance computations can produce `NaN` even when the inputs are
//! finite — e.g. cosine distance against a zero vector. `Vector`
//! construction does not reject zero vectors (they are valid sparse
//! embeddings), so the search path tolerates `NaN` scores by ordering
//! them as worst. They surface at the tail of the result list rather
//! than corrupting the heap.
//!
//! [`Iqdb::search`]: crate::Iqdb::search

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::error::Result;
use crate::payload::Payload;
use crate::record::{Record, RecordId};
use crate::store::MemoryStore;
use crate::vector::{DistanceMetric, Vector};

/// A single hit returned by similarity search.
///
/// Hits are ordered by `score` ascending — smaller is closer — and
/// the `payload` field carries the record's metadata at search time
/// so callers do not need a follow-up `get` to read the payload.
///
/// # Examples
///
/// ```
/// use iqdb::{DistanceMetric, Iqdb, Record, RecordId, Vector};
///
/// let db = Iqdb::open_in_memory();
/// db.upsert(Record::new(RecordId::new(1), Vector::new(vec![1.0, 0.0]).unwrap())).unwrap();
/// db.upsert(Record::new(RecordId::new(2), Vector::new(vec![0.0, 1.0]).unwrap())).unwrap();
///
/// let probe = Vector::new(vec![1.0, 0.0]).unwrap();
/// let hits = db.search(&probe, 1, DistanceMetric::Cosine).unwrap();
/// assert_eq!(hits.len(), 1);
/// assert_eq!(hits[0].id, RecordId::new(1));
/// assert!(hits[0].score.abs() < 1e-6);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SearchResult {
    /// Identity of the matched record.
    pub id: RecordId,
    /// Distance score under the active [`DistanceMetric`].
    /// Smaller is closer; `NaN` if the metric is undefined for the
    /// matched pair (e.g. cosine against a zero vector).
    pub score: f32,
    /// Cloned snapshot of the record's payload at search time.
    pub payload: Option<Payload>,
}

/// Top-`k` flat (brute-force) search over the in-memory store.
///
/// The `filter` predicate is applied **after** distance computation
/// but **before** the heap admit decision — so a record that fails
/// the filter is never considered for the result set, regardless of
/// how good its score is.
///
/// The filter runs while the store's read lock is held. Callers
/// **must not** call back into the same [`Iqdb`](crate::Iqdb) handle
/// from inside the filter — doing so risks a re-entrant lock
/// acquisition.
///
/// # Errors
///
/// Returns [`Error::DimensionMismatch`](crate::Error::DimensionMismatch)
/// if the query vector's dimensionality does not match any stored
/// record. (The first mismatch aborts the search — the store is
/// expected to hold a homogeneous schema; v0.7.0 collections will
/// enforce this at upsert time.)
///
/// # Notes
///
/// The bounded heap implementation maintains at most `k + 1`
/// allocations during the scan and admits each new candidate in
/// `O(log k)`. For `k = 0` the function short-circuits with an
/// empty result.
pub(crate) fn flat_search<F>(
    store: &MemoryStore,
    query: &Vector,
    k: usize,
    metric: DistanceMetric,
    filter: F,
) -> Result<Vec<SearchResult>>
where
    F: Fn(&Record) -> bool,
{
    if k == 0 {
        return Ok(Vec::new());
    }

    store.with_records(|records| {
        let mut heap: BinaryHeap<HeapEntry> = BinaryHeap::with_capacity(k + 1);

        for record in records.values() {
            let score = metric.distance(query, record.vector())?;
            if !filter(record) {
                continue;
            }
            let candidate = HeapEntry {
                score,
                id: record.id(),
            };
            if heap.len() < k {
                heap.push(candidate);
            } else if let Some(worst) = heap.peek() {
                if compare_score(candidate.score, worst.score) == Ordering::Less {
                    let _ = heap.pop();
                    heap.push(candidate);
                }
            }
        }

        let mut results: Vec<SearchResult> = heap
            .into_iter()
            .map(|entry| {
                let payload = records.get(&entry.id).and_then(|r| r.payload().cloned());
                SearchResult {
                    id: entry.id,
                    score: entry.score,
                    payload,
                }
            })
            .collect();
        // Same total order as the heap's `HeapEntry::cmp`: score
        // ascending under the NaN-as-worst rule, ties broken on id
        // ascending — so equal-score results land in a deterministic
        // order regardless of `BinaryHeap::into_iter`'s arbitrary
        // drain sequence.
        results.sort_by(|a, b| match compare_score(a.score, b.score) {
            Ordering::Equal => a.id.cmp(&b.id),
            other => other,
        });
        Ok(results)
    })
}

/// Lightweight pair held in the bounded top-`k` heap.
///
/// Carries only the score and id during the scan — payloads are
/// cloned at the end, after the heap has settled into the final
/// `k` survivors. That avoids cloning a payload for every candidate
/// that briefly enters the heap and is later evicted.
#[derive(Debug, Clone, Copy)]
struct HeapEntry {
    score: f32,
    id: RecordId,
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        compare_score(self.score, other.score) == Ordering::Equal && self.id == other.id
    }
}

impl Eq for HeapEntry {}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Natural order on score (smaller = better). Tie-break on id
        // so the ordering is total and deterministic — required for
        // `BinaryHeap` correctness and for stable test assertions.
        match compare_score(self.score, other.score) {
            Ordering::Equal => self.id.cmp(&other.id),
            other_ord => other_ord,
        }
    }
}

/// Compare two scores under the smaller-is-closer convention with
/// `NaN` ordered as worst (greatest).
///
/// Necessary because `f32` is only `PartialOrd`; the top-`k` heap
/// requires a total order to function. Treating `NaN` as worst
/// pushes undefined-metric pairs to the tail of the result list
/// rather than corrupting the comparison chain.
#[inline]
fn compare_score(a: f32, b: f32) -> Ordering {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_store(records: &[(u64, Vec<f32>)]) -> MemoryStore {
        let store = MemoryStore::new();
        for (id, components) in records {
            let v = Vector::new(components.clone()).expect("finite");
            store
                .upsert(Record::new(RecordId::new(*id), v))
                .expect("upsert ok");
        }
        store
    }

    fn always_accept(_record: &Record) -> bool {
        true
    }

    #[test]
    fn k_zero_returns_empty() {
        let store = build_store(&[(1, vec![1.0, 0.0])]);
        let q = Vector::new(vec![1.0, 0.0]).unwrap();
        let out = flat_search(&store, &q, 0, DistanceMetric::L2, always_accept).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn empty_store_returns_empty() {
        let store = MemoryStore::new();
        let q = Vector::new(vec![1.0, 0.0]).unwrap();
        let out = flat_search(&store, &q, 5, DistanceMetric::L2, always_accept).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn returns_at_most_k_results() {
        let store = build_store(&[
            (1, vec![1.0, 0.0]),
            (2, vec![0.0, 1.0]),
            (3, vec![0.5, 0.5]),
            (4, vec![1.0, 1.0]),
        ]);
        let q = Vector::new(vec![1.0, 0.0]).unwrap();
        let out = flat_search(&store, &q, 2, DistanceMetric::L2, always_accept).unwrap();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn k_larger_than_store_returns_all() {
        let store = build_store(&[(1, vec![1.0, 0.0]), (2, vec![0.0, 1.0])]);
        let q = Vector::new(vec![1.0, 0.0]).unwrap();
        let out = flat_search(&store, &q, 100, DistanceMetric::L2, always_accept).unwrap();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn results_are_sorted_ascending_by_score() {
        let store = build_store(&[
            (1, vec![10.0, 0.0]),
            (2, vec![1.0, 0.0]),
            (3, vec![5.0, 0.0]),
        ]);
        let q = Vector::new(vec![0.0, 0.0]).unwrap();
        let out = flat_search(&store, &q, 3, DistanceMetric::L2, always_accept).unwrap();
        assert_eq!(out.len(), 3);
        // Distance from origin: 1, 5, 10 → ids 2, 3, 1.
        assert_eq!(out[0].id, RecordId::new(2));
        assert_eq!(out[1].id, RecordId::new(3));
        assert_eq!(out[2].id, RecordId::new(1));
        assert!(out[0].score <= out[1].score);
        assert!(out[1].score <= out[2].score);
    }

    #[test]
    fn filter_excludes_records_before_admission() {
        let store = build_store(&[
            (1, vec![1.0, 0.0]),
            (2, vec![0.99, 0.0]),
            (3, vec![0.5, 0.0]),
        ]);
        let q = Vector::new(vec![1.0, 0.0]).unwrap();
        // Filter out the perfect match (id 1).
        let out = flat_search(&store, &q, 2, DistanceMetric::L2, |r| r.id().get() != 1).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|r| r.id != RecordId::new(1)));
    }

    #[test]
    fn dimension_mismatch_returns_error() {
        let store = build_store(&[(1, vec![1.0, 0.0])]);
        let q = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
        let err = flat_search(&store, &q, 1, DistanceMetric::L2, always_accept).unwrap_err();
        assert!(matches!(
            err,
            crate::Error::DimensionMismatch { left: 3, right: 2 }
        ));
    }

    #[test]
    fn cosine_against_zero_vector_yields_nan_at_tail() {
        // Cosine is undefined when either operand has zero norm.
        let store = build_store(&[
            (1, vec![1.0, 0.0]),
            (2, vec![0.0, 0.0]), // zero vector → cosine produces NaN
            (3, vec![0.5, 0.5]),
        ]);
        let q = Vector::new(vec![1.0, 0.0]).unwrap();
        let out = flat_search(&store, &q, 3, DistanceMetric::Cosine, always_accept).unwrap();
        assert_eq!(out.len(), 3);
        // The NaN-scoring record sorts to the tail.
        assert_eq!(out[2].id, RecordId::new(2));
        assert!(out[2].score.is_nan());
        // The first two have finite, ascending scores.
        assert!(out[0].score.is_finite());
        assert!(out[1].score.is_finite());
        assert!(out[0].score <= out[1].score);
    }

    #[test]
    fn tie_break_by_id_is_deterministic() {
        // Two records at the same distance → ids decide ordering.
        let store = build_store(&[
            (10, vec![1.0, 0.0]),
            (5, vec![1.0, 0.0]),
            (20, vec![1.0, 0.0]),
        ]);
        let q = Vector::new(vec![1.0, 0.0]).unwrap();
        let out = flat_search(&store, &q, 3, DistanceMetric::L2, always_accept).unwrap();
        let ids: Vec<u64> = out.iter().map(|r| r.id.get()).collect();
        // All scores 0; tie-break ascending on id.
        assert_eq!(ids, vec![5, 10, 20]);
    }

    #[test]
    fn payload_is_attached_to_results() {
        let store = MemoryStore::new();
        let mut payload = crate::Payload::new();
        payload.insert("kind", "doc");
        store
            .upsert(Record::with_payload(
                RecordId::new(1),
                Vector::new(vec![1.0, 0.0]).unwrap(),
                payload,
            ))
            .unwrap();
        let q = Vector::new(vec![1.0, 0.0]).unwrap();
        let out = flat_search(&store, &q, 1, DistanceMetric::L2, always_accept).unwrap();
        let hit = &out[0];
        let attached = hit.payload.as_ref().expect("payload attached");
        assert!(attached.contains_key("kind"));
    }
}
