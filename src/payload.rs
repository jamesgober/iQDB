// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Typed payload metadata attached to stored records.
//!
//! A [`Payload`] is a typed key-value bag that travels alongside each
//! vector in the store. It exists so callers can attach arbitrary
//! metadata — document ids, source URIs, timestamps, categorical
//! filters — without serialising into the vector's numeric channels.
//!
//! The shape is intentionally JSON-like but **typed**: every value
//! carries its native Rust representation rather than going through a
//! `Cow<str>` or untagged dynamic encoding. This lets the v0.3.0
//! filter-by-payload predicates compare values without a parse step
//! on the hot path.
//!
//! Keys are owned `String`s ordered deterministically by the underlying
//! [`BTreeMap`]. Deterministic iteration order means payload hashes,
//! `serde` round-trips, and test assertions are stable across runs and
//! across machines.
//!
//! [`BTreeMap`]: std::collections::BTreeMap

use std::collections::{btree_map, BTreeMap};

/// A single value in a [`Payload`].
///
/// Mirrors the JSON value taxonomy with native Rust types: null, boolean,
/// signed integer, double-precision float, owned string, owned byte
/// buffer, ordered array, and nested object. `#[non_exhaustive]` so new
/// types can be added in minor releases without a SemVer break.
///
/// # Examples
///
/// ```
/// use iqdb::PayloadValue;
///
/// let pv = PayloadValue::Text("hello".to_string());
/// assert!(pv.is_text());
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum PayloadValue {
    /// Explicit null / missing-value marker.
    Null,
    /// Boolean.
    Bool(bool),
    /// 64-bit signed integer.
    Int(i64),
    /// 64-bit IEEE 754 floating-point.
    ///
    /// `NaN` and the infinities are permitted in payload metadata —
    /// unlike vector components, they have well-defined meaning in
    /// some metadata schemas (e.g. "no measurement"). Filter predicates
    /// in v0.3.0 will treat them per IEEE 754 ordering.
    Float(f64),
    /// Owned UTF-8 string.
    Text(String),
    /// Owned byte buffer — for blob metadata that should not be
    /// re-encoded as UTF-8.
    Bytes(Vec<u8>),
    /// Heterogeneous ordered list of values.
    Array(Vec<PayloadValue>),
    /// Nested object — keys preserved in `BTreeMap` order.
    Object(BTreeMap<String, PayloadValue>),
}

impl PayloadValue {
    /// `true` if the value is the explicit null marker.
    #[inline]
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// `true` if the value is a boolean.
    #[inline]
    #[must_use]
    pub const fn is_bool(&self) -> bool {
        matches!(self, Self::Bool(_))
    }

    /// `true` if the value is an integer.
    #[inline]
    #[must_use]
    pub const fn is_int(&self) -> bool {
        matches!(self, Self::Int(_))
    }

    /// `true` if the value is a float.
    #[inline]
    #[must_use]
    pub const fn is_float(&self) -> bool {
        matches!(self, Self::Float(_))
    }

    /// `true` if the value is a text string.
    #[inline]
    #[must_use]
    pub const fn is_text(&self) -> bool {
        matches!(self, Self::Text(_))
    }

    /// `true` if the value is a byte buffer.
    #[inline]
    #[must_use]
    pub const fn is_bytes(&self) -> bool {
        matches!(self, Self::Bytes(_))
    }

    /// `true` if the value is an array.
    #[inline]
    #[must_use]
    pub const fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    /// `true` if the value is a nested object.
    #[inline]
    #[must_use]
    pub const fn is_object(&self) -> bool {
        matches!(self, Self::Object(_))
    }

    /// Borrow as `bool` if this variant is a boolean.
    #[inline]
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Borrow as `i64` if this variant is an integer.
    #[inline]
    #[must_use]
    pub const fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// Borrow as `f64` if this variant is a float.
    #[inline]
    #[must_use]
    pub const fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Borrow as `&str` if this variant is a text string.
    #[inline]
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Borrow as `&[u8]` if this variant is a byte buffer.
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(b) => Some(b.as_slice()),
            _ => None,
        }
    }
}

impl From<bool> for PayloadValue {
    #[inline]
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for PayloadValue {
    #[inline]
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<i32> for PayloadValue {
    #[inline]
    fn from(value: i32) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<f64> for PayloadValue {
    #[inline]
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<f32> for PayloadValue {
    #[inline]
    fn from(value: f32) -> Self {
        Self::Float(f64::from(value))
    }
}

impl From<String> for PayloadValue {
    #[inline]
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for PayloadValue {
    #[inline]
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<u8>> for PayloadValue {
    #[inline]
    fn from(value: Vec<u8>) -> Self {
        Self::Bytes(value)
    }
}

impl From<Vec<PayloadValue>> for PayloadValue {
    #[inline]
    fn from(value: Vec<PayloadValue>) -> Self {
        Self::Array(value)
    }
}

impl From<BTreeMap<String, PayloadValue>> for PayloadValue {
    #[inline]
    fn from(value: BTreeMap<String, PayloadValue>) -> Self {
        Self::Object(value)
    }
}

/// Ordered, typed key-value metadata attached to a stored record.
///
/// `Payload` is a thin wrapper around a `BTreeMap<String, PayloadValue>`.
/// The deterministic iteration order means payloads serialise stably
/// (important for content-addressed storage and reproducible test
/// assertions) and the lookup cost is `O(log n)` — adequate for the
/// per-record metadata sizes payloads are designed for.
///
/// # Examples
///
/// Build a payload and look up individual fields:
///
/// ```
/// use iqdb::{Payload, PayloadValue};
///
/// let mut p = Payload::new();
/// p.insert("source", "wikipedia");
/// p.insert("year", 2026_i64);
/// p.insert("verified", true);
///
/// assert_eq!(p.get("source").and_then(PayloadValue::as_text), Some("wikipedia"));
/// assert_eq!(p.get("year").and_then(PayloadValue::as_int), Some(2026));
/// assert_eq!(p.get("verified").and_then(PayloadValue::as_bool), Some(true));
/// ```
///
/// Construct from a fixed list using `FromIterator`:
///
/// ```
/// use iqdb::{Payload, PayloadValue};
///
/// let p: Payload = [
///     ("kind".to_string(), PayloadValue::from("article")),
///     ("score".to_string(), PayloadValue::from(0.92_f64)),
/// ]
/// .into_iter()
/// .collect();
///
/// assert_eq!(p.len(), 2);
/// ```
#[derive(Debug, Default, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Payload {
    fields: BTreeMap<String, PayloadValue>,
}

impl Payload {
    /// Construct an empty payload.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Payload;
    ///
    /// let p = Payload::new();
    /// assert!(p.is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a key-value pair into the payload.
    ///
    /// Returns the previously-stored value for `key`, if any. The key
    /// is borrowed as anything that can be converted into a `String`
    /// (`&str`, `String`, `Box<str>`, …) and the value as anything
    /// that implements `Into<PayloadValue>` (booleans, integers,
    /// floats, owned strings, slices, byte vectors, sub-payloads).
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Payload;
    ///
    /// let mut p = Payload::new();
    /// assert!(p.insert("color", "red").is_none());
    /// // Replacing returns the prior value.
    /// let prior = p.insert("color", "blue").unwrap();
    /// assert_eq!(prior.as_text(), Some("red"));
    /// ```
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: impl Into<PayloadValue>,
    ) -> Option<PayloadValue> {
        self.fields.insert(key.into(), value.into())
    }

    /// Look up a value by key.
    ///
    /// Returns `None` if the key is absent.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{Payload, PayloadValue};
    ///
    /// let mut p = Payload::new();
    /// p.insert("title", "Rust");
    /// assert_eq!(p.get("title").and_then(PayloadValue::as_text), Some("Rust"));
    /// assert!(p.get("missing").is_none());
    /// ```
    #[inline]
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&PayloadValue> {
        self.fields.get(key)
    }

    /// Remove a value by key, returning the previously-stored value
    /// if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Payload;
    ///
    /// let mut p = Payload::new();
    /// p.insert("k", "v");
    /// let v = p.remove("k").unwrap();
    /// assert_eq!(v.as_text(), Some("v"));
    /// assert!(p.is_empty());
    /// ```
    pub fn remove(&mut self, key: &str) -> Option<PayloadValue> {
        self.fields.remove(key)
    }

    /// `true` if the payload has no fields.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Number of top-level fields.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// `true` if the payload contains a value for `key`.
    #[inline]
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.fields.contains_key(key)
    }

    /// Borrow the underlying `BTreeMap` of fields.
    ///
    /// Useful for callers that need bulk iteration without going
    /// through [`iter`][Self::iter]. The returned reference is valid
    /// for the lifetime of the borrow.
    #[inline]
    #[must_use]
    pub fn as_map(&self) -> &BTreeMap<String, PayloadValue> {
        &self.fields
    }

    /// Iterate over `(key, value)` pairs in key order.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Payload;
    ///
    /// let mut p = Payload::new();
    /// p.insert("a", 1_i64);
    /// p.insert("b", 2_i64);
    ///
    /// let keys: Vec<&str> = p.iter().map(|(k, _)| k.as_str()).collect();
    /// assert_eq!(keys, vec!["a", "b"]);
    /// ```
    #[inline]
    pub fn iter(&self) -> btree_map::Iter<'_, String, PayloadValue> {
        self.fields.iter()
    }
}

impl<'a> IntoIterator for &'a Payload {
    type Item = (&'a String, &'a PayloadValue);
    type IntoIter = btree_map::Iter<'a, String, PayloadValue>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.fields.iter()
    }
}

impl IntoIterator for Payload {
    type Item = (String, PayloadValue);
    type IntoIter = btree_map::IntoIter<String, PayloadValue>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.fields.into_iter()
    }
}

impl FromIterator<(String, PayloadValue)> for Payload {
    fn from_iter<I: IntoIterator<Item = (String, PayloadValue)>>(iter: I) -> Self {
        Self {
            fields: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let p = Payload::new();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
    }

    #[test]
    fn insert_then_get_round_trip() {
        let mut p = Payload::new();
        let prior = p.insert("title", "Rust");
        assert!(prior.is_none());
        let value = p.get("title").unwrap();
        assert_eq!(value.as_text(), Some("Rust"));
    }

    #[test]
    fn insert_replacing_returns_prior() {
        let mut p = Payload::new();
        assert!(p.insert("k", "a").is_none());
        let prior = p.insert("k", "b").unwrap();
        assert_eq!(prior.as_text(), Some("a"));
        assert_eq!(p.get("k").and_then(PayloadValue::as_text), Some("b"));
    }

    #[test]
    fn remove_returns_prior_value() {
        let mut p = Payload::new();
        p.insert("k", "v");
        let removed = p.remove("k").unwrap();
        assert_eq!(removed.as_text(), Some("v"));
        assert!(p.is_empty());
    }

    #[test]
    fn remove_absent_returns_none() {
        let mut p = Payload::new();
        assert!(p.remove("missing").is_none());
    }

    #[test]
    fn contains_key_reflects_presence() {
        let mut p = Payload::new();
        p.insert("k", true);
        assert!(p.contains_key("k"));
        assert!(!p.contains_key("missing"));
    }

    #[test]
    fn iter_is_ordered_by_key() {
        let mut p = Payload::new();
        p.insert("c", 3_i64);
        p.insert("a", 1_i64);
        p.insert("b", 2_i64);
        let order: Vec<&str> = p.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn typed_accessors_return_none_for_wrong_variant() {
        let pv = PayloadValue::Text("hi".to_string());
        assert!(pv.as_bool().is_none());
        assert!(pv.as_int().is_none());
        assert!(pv.as_float().is_none());
        assert!(pv.as_bytes().is_none());
    }

    #[test]
    fn from_iter_collects_into_payload() {
        let p: Payload = [
            ("a".to_string(), PayloadValue::from(1_i64)),
            ("b".to_string(), PayloadValue::from(2_i64)),
        ]
        .into_iter()
        .collect();
        assert_eq!(p.len(), 2);
        assert_eq!(p.get("a").and_then(PayloadValue::as_int), Some(1));
    }

    #[test]
    fn from_conversions_cover_primitive_types() {
        assert!(matches!(PayloadValue::from(true), PayloadValue::Bool(true)));
        assert!(matches!(PayloadValue::from(42_i64), PayloadValue::Int(42)));
        assert!(matches!(PayloadValue::from(42_i32), PayloadValue::Int(42)));
        assert!(matches!(
            PayloadValue::from(1.5_f64),
            PayloadValue::Float(v) if (v - 1.5).abs() < 1e-9
        ));
        assert!(matches!(
            PayloadValue::from(1.5_f32),
            PayloadValue::Float(v) if (v - 1.5).abs() < 1e-6
        ));
        assert!(matches!(
            PayloadValue::from("hi"),
            PayloadValue::Text(s) if s == "hi"
        ));
        assert!(matches!(
            PayloadValue::from("hi".to_string()),
            PayloadValue::Text(s) if s == "hi"
        ));
        assert!(matches!(
            PayloadValue::from(vec![1_u8, 2, 3]),
            PayloadValue::Bytes(b) if b == vec![1, 2, 3]
        ));
    }

    #[test]
    fn type_predicates_match_active_variant() {
        assert!(PayloadValue::Null.is_null());
        assert!(PayloadValue::Bool(true).is_bool());
        assert!(PayloadValue::Int(1).is_int());
        assert!(PayloadValue::Float(1.0).is_float());
        assert!(PayloadValue::Text("a".to_string()).is_text());
        assert!(PayloadValue::Bytes(vec![0]).is_bytes());
        assert!(PayloadValue::Array(Vec::new()).is_array());
        assert!(PayloadValue::Object(BTreeMap::new()).is_object());
    }

    #[test]
    fn into_iter_by_ref_yields_field_pairs() {
        let mut p = Payload::new();
        p.insert("k", 1_i64);
        for (key, value) in &p {
            assert_eq!(key.as_str(), "k");
            assert_eq!(value.as_int(), Some(1));
        }
    }

    #[test]
    fn into_iter_by_value_consumes_payload() {
        let mut p = Payload::new();
        p.insert("k", 1_i64);
        let collected: Vec<(String, PayloadValue)> = p.into_iter().collect();
        assert_eq!(collected.len(), 1);
    }
}
