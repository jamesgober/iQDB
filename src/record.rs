// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Record identity and the [`Record`] aggregate stored in the database.
//!
//! A record is the unit of read / write through the [`Iqdb`] handle —
//! it bundles a [`RecordId`] (caller-supplied unique key), a [`Vector`]
//! (the embedding), and an optional [`Payload`] (typed metadata).
//!
//! [`RecordId`] is a transparent newtype around `u64`. The `u64`
//! representation is deliberately exposed via `From<u64>` /
//! `Into<u64>` so callers can pull ids from any source — hashed
//! strings, autoincrement counters, snowflake ids — without an
//! adapter layer. The newtype prevents accidental mixing with other
//! numeric keys in the surrounding code-base.
//!
//! [`Iqdb`]: crate::Iqdb

use crate::payload::Payload;
use crate::vector::Vector;

/// Caller-supplied unique identifier for a stored [`Record`].
///
/// Transparent over `u64`. Cheap to copy (8 bytes), cheap to hash, and
/// stable across the wire when the optional `serde` feature is on.
///
/// # Examples
///
/// ```
/// use iqdb::RecordId;
///
/// let id = RecordId::new(42);
/// assert_eq!(id.get(), 42);
///
/// // Inter-conversion with the raw `u64`.
/// let raw: u64 = id.into();
/// assert_eq!(raw, 42);
///
/// let back: RecordId = 42u64.into();
/// assert_eq!(back, id);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct RecordId(u64);

impl RecordId {
    /// Wrap a raw `u64` as a [`RecordId`].
    #[inline]
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Return the underlying `u64`.
    #[inline]
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for RecordId {
    #[inline]
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<RecordId> for u64 {
    #[inline]
    fn from(value: RecordId) -> Self {
        value.0
    }
}

impl core::fmt::Display for RecordId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The unit of read / write through the database handle.
///
/// A `Record` aggregates the three components every stored entry
/// carries:
///
/// - `id` — caller-supplied [`RecordId`], unique within the store.
/// - `vector` — the [`Vector`] embedding to index and search against.
/// - `payload` — optional [`Payload`] of typed metadata.
///
/// Both [`Iqdb::upsert`](crate::Iqdb::upsert) and the [`get`]
/// equivalents flow through `Record` so callers have a single,
/// stable shape to bind to. The intentional split between the
/// `Record::new` (vector-only) and `Record::with_payload`
/// constructors avoids the `Option`-typed argument pattern that
/// degrades readability at call sites.
///
/// [`get`]: crate::Iqdb::get
///
/// # Examples
///
/// Construct a record without metadata:
///
/// ```
/// use iqdb::{Record, RecordId, Vector};
///
/// let r = Record::new(
///     RecordId::new(1),
///     Vector::new(vec![0.1, 0.2, 0.3]).unwrap(),
/// );
/// assert_eq!(r.id().get(), 1);
/// assert_eq!(r.vector().dim(), 3);
/// assert!(r.payload().is_none());
/// ```
///
/// Construct a record with typed metadata:
///
/// ```
/// use iqdb::{Payload, Record, RecordId, Vector};
///
/// let mut meta = Payload::new();
/// meta.insert("source", "wikipedia");
/// meta.insert("year", 2026_i64);
///
/// let r = Record::with_payload(
///     RecordId::new(7),
///     Vector::new(vec![0.0, 1.0, 0.0]).unwrap(),
///     meta,
/// );
/// assert!(r.payload().is_some());
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Record {
    id: RecordId,
    vector: Vector,
    payload: Option<Payload>,
}

impl Record {
    /// Construct a record without metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{Record, RecordId, Vector};
    ///
    /// let r = Record::new(
    ///     RecordId::new(1),
    ///     Vector::new(vec![0.0, 1.0]).unwrap(),
    /// );
    /// assert!(r.payload().is_none());
    /// ```
    #[inline]
    #[must_use]
    pub fn new(id: impl Into<RecordId>, vector: Vector) -> Self {
        Self {
            id: id.into(),
            vector,
            payload: None,
        }
    }

    /// Construct a record with metadata attached.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{Payload, Record, RecordId, Vector};
    ///
    /// let mut p = Payload::new();
    /// p.insert("score", 0.97_f64);
    ///
    /// let r = Record::with_payload(
    ///     RecordId::new(1),
    ///     Vector::new(vec![1.0, 0.0]).unwrap(),
    ///     p,
    /// );
    /// assert!(r.payload().is_some());
    /// ```
    #[inline]
    #[must_use]
    pub fn with_payload(id: impl Into<RecordId>, vector: Vector, payload: Payload) -> Self {
        Self {
            id: id.into(),
            vector,
            payload: Some(payload),
        }
    }

    /// Caller-supplied identity.
    #[inline]
    #[must_use]
    pub const fn id(&self) -> RecordId {
        self.id
    }

    /// Borrow the stored vector.
    #[inline]
    #[must_use]
    pub const fn vector(&self) -> &Vector {
        &self.vector
    }

    /// Borrow the stored payload, if any.
    #[inline]
    #[must_use]
    pub const fn payload(&self) -> Option<&Payload> {
        self.payload.as_ref()
    }

    /// Decompose the record into its three constituent parts.
    ///
    /// Useful for callers that need owned access to the vector or
    /// payload without re-cloning the `Record`.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{Record, RecordId, Vector};
    ///
    /// let r = Record::new(
    ///     RecordId::new(1),
    ///     Vector::new(vec![1.0, 2.0]).unwrap(),
    /// );
    /// let (id, vec, payload) = r.into_parts();
    /// assert_eq!(id.get(), 1);
    /// assert_eq!(vec.dim(), 2);
    /// assert!(payload.is_none());
    /// ```
    #[inline]
    #[must_use]
    pub fn into_parts(self) -> (RecordId, Vector, Option<Payload>) {
        (self.id, self.vector, self.payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_id_round_trips_through_u64() {
        let id = RecordId::new(42);
        let raw: u64 = id.into();
        assert_eq!(raw, 42);
        let back: RecordId = raw.into();
        assert_eq!(back, id);
    }

    #[test]
    fn record_id_orders_lexicographically_on_u64() {
        assert!(RecordId::new(1) < RecordId::new(2));
        assert!(RecordId::new(u64::MAX) > RecordId::new(0));
    }

    #[test]
    fn record_id_display_is_raw_u64() {
        let s = format!("{}", RecordId::new(123));
        assert_eq!(s, "123");
    }

    #[test]
    fn record_new_has_no_payload() {
        let r = Record::new(1_u64, Vector::new(vec![0.0, 1.0]).unwrap());
        assert!(r.payload().is_none());
        assert_eq!(r.id().get(), 1);
        assert_eq!(r.vector().dim(), 2);
    }

    #[test]
    fn record_with_payload_carries_metadata() {
        let mut p = Payload::new();
        p.insert("k", "v");
        let r = Record::with_payload(RecordId::new(1), Vector::new(vec![0.0, 1.0]).unwrap(), p);
        let payload = r.payload().expect("payload set");
        assert!(payload.contains_key("k"));
    }

    #[test]
    fn into_parts_decomposes_without_clone() {
        let r = Record::new(RecordId::new(7), Vector::new(vec![1.0, 2.0, 3.0]).unwrap());
        let (id, vec, payload) = r.into_parts();
        assert_eq!(id.get(), 7);
        assert_eq!(vec.dim(), 3);
        assert!(payload.is_none());
    }
}
