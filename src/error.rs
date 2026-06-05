// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Error types for the `iqdb` crate.
//!
//! All fallible operations return [`Result<T>`] — an alias for
//! `core::result::Result<T, Error>`. The [`Error`] type enumerates every
//! failure mode the crate can produce. Error codes are reserved under the
//! `IQERR-XXXXX` prefix in the wider Hive error registry.

use core::fmt;

/// Convenient `Result` alias where the error type is fixed to [`Error`].
pub type Result<T> = core::result::Result<T, Error>;

/// The top-level error type returned by every fallible operation in `iqdb`.
///
/// The enum is `#[non_exhaustive]`; new variants may be added in minor
/// releases as new failure modes emerge. Callers must never write
/// exhaustive `match` arms over `Error` — always include a `_` arm.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A lower-level I/O failure occurred.
    ///
    /// Callers should inspect the wrapped `std::io::ErrorKind` and decide
    /// whether retry, fallback, or surface-to-user behavior is appropriate.
    Io(std::io::Error),

    /// Invalid runtime configuration.
    ///
    /// This indicates programmer error when constructing the database.
    InvalidConfig(&'static str),

    /// A vector failed validation at the system boundary.
    ///
    /// The `reason` is a static string describing why the input was
    /// rejected (empty vector, non-finite component, etc.). Validation
    /// happens in [`Vector::new`](crate::Vector::new) and friends so
    /// that downstream code can treat every constructed `Vector` as
    /// known-good — no internal path needs to re-check.
    InvalidVector {
        /// Static description of why the vector was rejected. Never
        /// contains user-supplied data, so it is safe to log.
        reason: &'static str,
    },

    /// Two vectors with different dimensionality were combined.
    ///
    /// Returned by distance-metric computations and by store operations
    /// that need to enforce a homogeneous schema. The `left` and
    /// `right` fields carry the observed dimensions so callers can
    /// surface an actionable message.
    DimensionMismatch {
        /// Dimensionality of the first vector in the offending pair.
        left: usize,
        /// Dimensionality of the second vector in the offending pair.
        right: usize,
    },

    /// On-disk data was found to be corrupt during recovery.
    ///
    /// Surfaced by the file-backed store when the snapshot or WAL
    /// fails an integrity check (bad magic header, unrecognised
    /// version, CRC mismatch, truncated entry). The `reason` is a
    /// static string identifying which check failed; the file path
    /// is not embedded so log forwarding stays safe by default.
    ///
    /// Recovery behaviour: the file-backed store stops replaying at
    /// the first corrupt entry. Records committed before the
    /// corruption are still loaded; anything after is discarded.
    Corrupt {
        /// Static description of which integrity check failed.
        reason: &'static str,
    },

    /// The requested operation is not yet implemented.
    ///
    /// Used by methods whose engine path lands in a later milestone
    /// (currently `Iqdb::open(path)` and `Iqdb::flush` — both arrive
    /// with the durable-storage substrate in v0.4.0). Letting these
    /// stubs return a typed error rather than panicking lets callers
    /// wire them now and gate behaviour on the variant.
    NotImplemented,
}

impl Error {
    /// Construct an [`Error::InvalidVector`] from a static reason string.
    ///
    /// Internal helper used by the vector constructors. Kept `pub(crate)`
    /// so the surface of constructible reasons stays inside the crate.
    pub(crate) const fn invalid_vector(reason: &'static str) -> Self {
        Self::InvalidVector { reason }
    }

    /// Construct an [`Error::Corrupt`] from a static reason string.
    ///
    /// Internal helper used by the codec and the file-backed store
    /// when an integrity check fails. Kept `pub(crate)` so the surface
    /// of constructible reasons stays inside the crate.
    pub(crate) const fn corrupt(reason: &'static str) -> Self {
        Self::Corrupt { reason }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "iqdb: io error ({})", err.kind()),
            Self::InvalidConfig(msg) => write!(f, "iqdb: invalid configuration ({msg})"),
            Self::InvalidVector { reason } => write!(f, "iqdb: invalid vector ({reason})"),
            Self::DimensionMismatch { left, right } => {
                write!(f, "iqdb: dimension mismatch (left={left}, right={right})")
            }
            Self::Corrupt { reason } => write!(f, "iqdb: corrupt store ({reason})"),
            Self::NotImplemented => f.write_str("iqdb: not implemented"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_implements_std_error() {
        fn assert_error<E: std::error::Error>() {}
        assert_error::<Error>();
    }

    #[test]
    fn io_error_display_does_not_leak_payload() {
        let err = Error::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "secret",
        ));
        let msg = format!("{err}");
        assert!(msg.contains("permission denied") || msg.contains("PermissionDenied"));
        assert!(!msg.contains("secret"));
    }

    #[test]
    fn invalid_config_display_is_stable() {
        let msg = format!("{}", Error::InvalidConfig("bad path"));
        assert!(msg.contains("invalid configuration"));
        assert!(msg.contains("bad path"));
    }

    #[test]
    fn invalid_vector_display_is_stable() {
        let err = Error::invalid_vector("empty vector");
        let msg = format!("{err}");
        assert!(msg.contains("invalid vector"));
        assert!(msg.contains("empty vector"));
    }

    #[test]
    fn dimension_mismatch_display_includes_both_dims() {
        let msg = format!("{}", Error::DimensionMismatch { left: 4, right: 8 });
        assert!(msg.contains("dimension mismatch"));
        assert!(msg.contains("left=4"));
        assert!(msg.contains("right=8"));
    }

    #[test]
    fn corrupt_display_includes_reason() {
        let err = Error::corrupt("bad magic");
        let msg = format!("{err}");
        assert!(msg.contains("corrupt store"));
        assert!(msg.contains("bad magic"));
    }

    #[test]
    fn not_implemented_display_is_stable() {
        let msg = format!("{}", Error::NotImplemented);
        assert!(msg.contains("not implemented"));
    }

    #[test]
    fn from_io_maps_to_io_variant() {
        let err: Error = std::io::Error::new(std::io::ErrorKind::NotFound, "missing").into();
        assert!(matches!(err, Error::Io(_)));
    }

    #[test]
    fn io_error_exposes_source() {
        let inner = std::io::Error::other("x");
        let err = Error::Io(inner);
        assert!(std::error::Error::source(&err).is_some());
    }

    #[test]
    fn non_io_variant_has_no_source() {
        let err = Error::invalid_vector("empty vector");
        assert!(std::error::Error::source(&err).is_none());
    }
}
