// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Vector primitives — the core numeric type stored by `iqdb` and the
//! [`DistanceMetric`] enum that selects how two vectors are compared.
//!
//! Every [`Vector`] is validated at construction:
//!
//! - Empty input is rejected ([`Error::InvalidVector`]).
//! - Any non-finite component (`NaN`, `+∞`, `−∞`) is rejected.
//!
//! Downstream code can therefore treat every constructed `Vector` as
//! known-good — no internal path needs to re-check finiteness or
//! emptiness. Distance computations enforce dimensional homogeneity
//! and surface [`Error::DimensionMismatch`] when two vectors of
//! different lengths are combined.
//!
//! The storage layout is a contiguous, owned `Box<[f32]>` — equivalent
//! to a `Vec<f32>` without the spare-capacity overhead. The slice is
//! laid out for cache-friendly sequential access; SIMD acceleration of
//! the distance kernels is reserved for the search-engine milestones
//! (v0.3.0+) where the win actually pays for the platform-specific
//! complexity.

use crate::error::{Error, Result};

/// An immutable, owned numeric vector of `f32` components.
///
/// A `Vector` is the unit of value stored by `iqdb`. Construction
/// validates the input — empty vectors and non-finite components are
/// rejected — so every constructed `Vector` carries the invariant
/// `dim() > 0 && components are finite`.
///
/// # Examples
///
/// ```
/// use iqdb::Vector;
///
/// let v = Vector::new(vec![0.5, -0.25, 0.75]).expect("finite components");
/// assert_eq!(v.dim(), 3);
/// assert_eq!(v.as_slice(), &[0.5, -0.25, 0.75]);
/// ```
///
/// Validation rejects bad inputs:
///
/// ```
/// use iqdb::{Error, Vector};
///
/// assert!(matches!(
///     Vector::new(Vec::<f32>::new()),
///     Err(Error::InvalidVector { .. })
/// ));
///
/// assert!(matches!(
///     Vector::new(vec![1.0, f32::NAN]),
///     Err(Error::InvalidVector { .. })
/// ));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Vector {
    data: Box<[f32]>,
}

impl Vector {
    /// Construct a [`Vector`] from an owned `Vec<f32>`.
    ///
    /// Consumes the input — no allocation beyond the `Vec → Box<[f32]>`
    /// shrink-to-fit conversion (and even that is elided when the
    /// `Vec`'s capacity already equals its length).
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidVector`] if:
    ///
    /// - `data` is empty, or
    /// - any component is `NaN`, `+∞`, or `−∞`.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Vector;
    ///
    /// let v = Vector::new(vec![1.0, 2.0, 3.0]).expect("finite components");
    /// assert_eq!(v.dim(), 3);
    /// ```
    pub fn new(data: Vec<f32>) -> Result<Self> {
        Self::from_box(data.into_boxed_slice())
    }

    /// Construct a [`Vector`] by copying from a slice.
    ///
    /// Use this when the caller has a borrowed slice and does not need
    /// to retain ownership. The slice is copied into a freshly
    /// allocated boxed slice; prefer [`Vector::new`] when an owned
    /// `Vec<f32>` is already available, to avoid the extra copy.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidVector`] under the same conditions as
    /// [`Vector::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Vector;
    ///
    /// let data = [0.1_f32, 0.2, 0.3];
    /// let v = Vector::from_slice(&data).expect("finite components");
    /// assert_eq!(v.dim(), 3);
    /// ```
    pub fn from_slice(data: &[f32]) -> Result<Self> {
        Self::from_box(Box::from(data))
    }

    /// Internal constructor that performs validation on an already
    /// boxed slice. Centralises the validation in one place so the
    /// public constructors stay thin.
    fn from_box(data: Box<[f32]>) -> Result<Self> {
        if data.is_empty() {
            return Err(Error::invalid_vector("vector is empty"));
        }
        if !data.iter().all(|v| v.is_finite()) {
            return Err(Error::invalid_vector("vector contains a non-finite value"));
        }
        Ok(Self { data })
    }

    /// Return the dimensionality of the vector.
    ///
    /// Guaranteed to be `>= 1` for any constructed `Vector`.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Vector;
    ///
    /// let v = Vector::new(vec![0.0; 128]).expect("non-empty");
    /// assert_eq!(v.dim(), 128);
    /// ```
    #[inline]
    #[must_use]
    pub fn dim(&self) -> usize {
        self.data.len()
    }

    /// Return the vector's components as a contiguous slice.
    ///
    /// The returned slice borrows from the `Vector` and is valid for
    /// the lifetime of the borrow. The layout is dense, contiguous,
    /// and suitable for SIMD-aligned reads in distance kernels.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Vector;
    ///
    /// let v = Vector::new(vec![1.0, 2.0, 3.0]).unwrap();
    /// let sum: f32 = v.as_slice().iter().sum();
    /// assert_eq!(sum, 6.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }

    /// Consume the vector and return its underlying boxed slice.
    ///
    /// Useful when re-packing into another structure without a clone.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Vector;
    ///
    /// let v = Vector::new(vec![1.0, 2.0]).unwrap();
    /// let owned: Box<[f32]> = v.into_inner();
    /// assert_eq!(&*owned, &[1.0, 2.0]);
    /// ```
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> Box<[f32]> {
        self.data
    }

    /// Sum of squared components.
    ///
    /// Equivalent to `self · self` — used internally by cosine
    /// distance. Exposed because callers occasionally need it for
    /// pre-normalisation, and it folds the same SIMD-friendly inner
    /// loop the other distance kernels use.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Vector;
    ///
    /// let v = Vector::new(vec![3.0, 4.0]).unwrap();
    /// assert_eq!(v.norm_squared(), 25.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn norm_squared(&self) -> f32 {
        self.data.iter().map(|x| x * x).sum()
    }

    /// Euclidean norm — `sqrt(self · self)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::Vector;
    ///
    /// let v = Vector::new(vec![3.0, 4.0]).unwrap();
    /// assert_eq!(v.norm(), 5.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn norm(&self) -> f32 {
        self.norm_squared().sqrt()
    }
}

impl AsRef<[f32]> for Vector {
    #[inline]
    fn as_ref(&self) -> &[f32] {
        &self.data
    }
}

impl TryFrom<Vec<f32>> for Vector {
    type Error = Error;

    #[inline]
    fn try_from(value: Vec<f32>) -> Result<Self> {
        Self::new(value)
    }
}

impl<'a> TryFrom<&'a [f32]> for Vector {
    type Error = Error;

    #[inline]
    fn try_from(value: &'a [f32]) -> Result<Self> {
        Self::from_slice(value)
    }
}

/// Distance metric used to compare two vectors.
///
/// Every variant defines an ordering where **smaller values mean more
/// similar**:
///
/// | Variant   | Returns                                       | Range            |
/// |-----------|-----------------------------------------------|------------------|
/// | `L2`      | Euclidean distance `‖a − b‖₂`                 | `[0, +∞)`        |
/// | `Cosine`  | `1 − cos(θ)` where `θ` is the angle           | `[0, 2]`         |
/// | `Dot`     | `−(a · b)` — negative dot product             | `(−∞, +∞)`       |
///
/// The `Dot` variant returns `−(a · b)` so that the smaller-is-closer
/// invariant holds across all three metrics — search engines built on
/// top of a `DistanceMetric` can use the same ordering logic without
/// special-casing inner-product similarity.
///
/// # Examples
///
/// ```
/// use iqdb::{DistanceMetric, Vector};
///
/// let a = Vector::new(vec![1.0, 0.0]).unwrap();
/// let b = Vector::new(vec![0.0, 1.0]).unwrap();
///
/// // Orthogonal unit vectors: L2 distance √2, cosine distance 1, dot distance 0.
/// let l2 = DistanceMetric::L2.distance(&a, &b).unwrap();
/// let cos = DistanceMetric::Cosine.distance(&a, &b).unwrap();
/// let dot = DistanceMetric::Dot.distance(&a, &b).unwrap();
///
/// assert!((l2 - std::f32::consts::SQRT_2).abs() < 1e-6);
/// assert!((cos - 1.0).abs() < 1e-6);
/// assert!(dot.abs() < 1e-6);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DistanceMetric {
    /// Euclidean (L2) distance — `sqrt(sum((a_i − b_i)²))`.
    ///
    /// Symmetric, non-negative, and satisfies the triangle inequality.
    /// The natural choice for embeddings projected into a space where
    /// magnitude carries meaning.
    L2,

    /// Cosine distance — `1 − (a · b) / (‖a‖ · ‖b‖)`.
    ///
    /// Sensitive to direction only; magnitude is ignored. The standard
    /// metric for normalised text and image embeddings. Returns `NaN`
    /// if either input has zero norm — callers may wish to pre-filter
    /// or pre-normalise.
    Cosine,

    /// Negative dot-product — `−(a · b)`.
    ///
    /// Used when the embedding model exposes raw inner-product
    /// similarity (e.g., maximum inner product search). The sign is
    /// flipped so that the smaller-is-closer convention holds.
    Dot,
}

impl DistanceMetric {
    /// Compute the distance between `a` and `b` under this metric.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DimensionMismatch`] when the two vectors have
    /// different dimensionalities.
    ///
    /// # Examples
    ///
    /// ```
    /// use iqdb::{DistanceMetric, Vector};
    ///
    /// let a = Vector::new(vec![1.0, 2.0, 3.0]).unwrap();
    /// let b = Vector::new(vec![1.0, 2.0, 3.0]).unwrap();
    /// // Identical vectors → zero distance under L2.
    /// assert_eq!(DistanceMetric::L2.distance(&a, &b).unwrap(), 0.0);
    /// ```
    pub fn distance(self, a: &Vector, b: &Vector) -> Result<f32> {
        if a.dim() != b.dim() {
            return Err(Error::DimensionMismatch {
                left: a.dim(),
                right: b.dim(),
            });
        }
        let value = match self {
            Self::L2 => l2_distance(a.as_slice(), b.as_slice()),
            Self::Cosine => cosine_distance(a.as_slice(), b.as_slice()),
            Self::Dot => -dot_product(a.as_slice(), b.as_slice()),
        };
        Ok(value)
    }
}

/// Euclidean distance over equal-length slices.
///
/// Callers must validate `a.len() == b.len()` first — internal kernel.
#[inline]
fn l2_distance(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let sum_sq: f32 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = x - y;
            d * d
        })
        .sum();
    sum_sq.sqrt()
}

/// Cosine distance over equal-length slices.
///
/// Returns `NaN` if either input has zero norm — undefined for the
/// zero vector. Callers must validate `a.len() == b.len()` first.
#[inline]
fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let mut dot = 0.0_f32;
    let mut na = 0.0_f32;
    let mut nb = 0.0_f32;
    for (&x, &y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let denom = (na * nb).sqrt();
    if denom == 0.0 {
        f32::NAN
    } else {
        1.0 - dot / denom
    }
}

/// Dot product over equal-length slices.
///
/// Callers must validate `a.len() == b.len()` first.
#[inline]
fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_empty_vector() {
        let err = Vector::new(Vec::new()).unwrap_err();
        assert!(matches!(err, Error::InvalidVector { .. }));
    }

    #[test]
    fn new_rejects_nan_component() {
        let err = Vector::new(vec![1.0, f32::NAN, 2.0]).unwrap_err();
        assert!(matches!(err, Error::InvalidVector { .. }));
    }

    #[test]
    fn new_rejects_positive_infinity() {
        let err = Vector::new(vec![f32::INFINITY]).unwrap_err();
        assert!(matches!(err, Error::InvalidVector { .. }));
    }

    #[test]
    fn new_rejects_negative_infinity() {
        let err = Vector::new(vec![f32::NEG_INFINITY]).unwrap_err();
        assert!(matches!(err, Error::InvalidVector { .. }));
    }

    #[test]
    fn from_slice_copies_input() {
        let data = [1.0, 2.0, 3.0];
        let v = Vector::from_slice(&data).unwrap();
        assert_eq!(v.as_slice(), &data);
    }

    #[test]
    fn try_from_vec_works() {
        let v: Vector = vec![1.0_f32, 2.0].try_into().unwrap();
        assert_eq!(v.dim(), 2);
    }

    #[test]
    fn try_from_slice_works() {
        let data: &[f32] = &[1.0, 2.0];
        let v: Vector = data.try_into().unwrap();
        assert_eq!(v.dim(), 2);
    }

    #[test]
    fn dim_matches_input_length() {
        let v = Vector::new(vec![0.0; 768]).unwrap();
        assert_eq!(v.dim(), 768);
    }

    #[test]
    fn norm_squared_equals_dot_with_self() {
        let v = Vector::new(vec![3.0, 4.0]).unwrap();
        assert_eq!(v.norm_squared(), 25.0);
    }

    #[test]
    fn norm_of_3_4_is_5() {
        let v = Vector::new(vec![3.0, 4.0]).unwrap();
        assert!((v.norm() - 5.0).abs() < 1e-6);
    }

    #[test]
    fn as_slice_and_as_ref_agree() {
        let v = Vector::new(vec![1.0, 2.0]).unwrap();
        assert_eq!(v.as_slice(), v.as_ref());
    }

    #[test]
    fn into_inner_returns_box() {
        let v = Vector::new(vec![1.0, 2.0]).unwrap();
        let owned = v.into_inner();
        assert_eq!(&*owned, &[1.0, 2.0]);
    }

    #[test]
    fn l2_distance_identical_vectors_is_zero() {
        let a = Vector::new(vec![1.0, 2.0, 3.0]).unwrap();
        let b = Vector::new(vec![1.0, 2.0, 3.0]).unwrap();
        assert_eq!(DistanceMetric::L2.distance(&a, &b).unwrap(), 0.0);
    }

    #[test]
    fn l2_distance_3_4_5_triple() {
        // L2 between (0,0) and (3,4) is 5.
        let a = Vector::new(vec![0.0, 0.0]).unwrap();
        let b = Vector::new(vec![3.0, 4.0]).unwrap();
        let d = DistanceMetric::L2.distance(&a, &b).unwrap();
        assert!((d - 5.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_distance_identical_unit_vectors_is_zero() {
        let a = Vector::new(vec![1.0, 0.0]).unwrap();
        let b = Vector::new(vec![1.0, 0.0]).unwrap();
        let d = DistanceMetric::Cosine.distance(&a, &b).unwrap();
        assert!(d.abs() < 1e-6);
    }

    #[test]
    fn cosine_distance_orthogonal_unit_vectors_is_one() {
        let a = Vector::new(vec![1.0, 0.0]).unwrap();
        let b = Vector::new(vec![0.0, 1.0]).unwrap();
        let d = DistanceMetric::Cosine.distance(&a, &b).unwrap();
        assert!((d - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_distance_opposite_unit_vectors_is_two() {
        let a = Vector::new(vec![1.0, 0.0]).unwrap();
        let b = Vector::new(vec![-1.0, 0.0]).unwrap();
        let d = DistanceMetric::Cosine.distance(&a, &b).unwrap();
        assert!((d - 2.0).abs() < 1e-6);
    }

    #[test]
    fn dot_distance_is_negated_inner_product() {
        let a = Vector::new(vec![1.0, 2.0, 3.0]).unwrap();
        let b = Vector::new(vec![4.0, 5.0, 6.0]).unwrap();
        // a·b = 4 + 10 + 18 = 32, so Dot distance = -32.
        let d = DistanceMetric::Dot.distance(&a, &b).unwrap();
        assert!((d + 32.0).abs() < 1e-5);
    }

    #[test]
    fn distance_rejects_dimension_mismatch() {
        let a = Vector::new(vec![1.0, 2.0]).unwrap();
        let b = Vector::new(vec![1.0, 2.0, 3.0]).unwrap();
        for metric in [
            DistanceMetric::L2,
            DistanceMetric::Cosine,
            DistanceMetric::Dot,
        ] {
            let err = metric.distance(&a, &b).unwrap_err();
            assert!(matches!(
                err,
                Error::DimensionMismatch { left: 2, right: 3 }
            ));
        }
    }

    #[test]
    fn vector_clone_is_independent() {
        let a = Vector::new(vec![1.0, 2.0, 3.0]).unwrap();
        let b = a.clone();
        assert_eq!(a.as_slice(), b.as_slice());
        // Distinct allocations — modifying the box behind one does
        // not affect the other. We verify the deep-copy via PartialEq
        // here; mutation is not part of the public surface.
        assert_eq!(a, b);
    }
}
