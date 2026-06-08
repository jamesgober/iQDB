// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! The unified `iqdb` error type.
//!
//! Every fallible operation on the public surface returns [`Result<T>`],
//! whose error is the single [`Error`] enum. `iqdb` composes two upstream
//! error vocabularies — [`iqdb_types::IqdbError`] for the index and
//! vector-vocabulary layer, and [`iqdb_persist::PersistError`] for the
//! durable-storage layer — and folds them into one `#[non_exhaustive]`
//! type so callers match on a single surface. A third variant,
//! [`Error::Config`], covers handle-level consistency checks that belong
//! to neither layer (for example, a reopen whose requested `dim` / `metric`
//! disagrees with the stored database).
//!
//! Every variant's [`Display`](fmt::Display) carries only static text or
//! values the library itself produced — never echoed vector payloads — so
//! an error string is safe to forward to a log sink.

use core::fmt;

use iqdb_persist::PersistError;
use iqdb_types::IqdbError;

/// An error from any `iqdb` operation.
///
/// The enum is `#[non_exhaustive]`: a `match` on it must include a wildcard
/// arm, and future releases may add variants without that being a breaking
/// change.
#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    /// A failure from the index / vocabulary layer — a dimension mismatch,
    /// an absent id, an invalid metric for the chosen index, and so on. See
    /// [`iqdb_types::IqdbError`] for the full set of causes.
    Index(IqdbError),
    /// A failure from the durable-storage layer — snapshot or write-ahead-log
    /// I/O, a corrupt or truncated on-disk file, a checksum mismatch, or an
    /// unsupported compression feature. See [`iqdb_persist::PersistError`].
    Persist(PersistError),
    /// A handle-level configuration or consistency check failed. `reason` is
    /// a short static description (for example, that a reopened database's
    /// `dim` or `metric` does not match the value passed to `open`).
    Config(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Index(e) => write!(f, "{e}"),
            Self::Persist(e) => write!(f, "{e}"),
            Self::Config(reason) => write!(f, "invalid configuration: {reason}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Index(e) => Some(e),
            Self::Persist(e) => Some(e),
            Self::Config(_) => None,
        }
    }
}

impl From<IqdbError> for Error {
    fn from(value: IqdbError) -> Self {
        Self::Index(value)
    }
}

impl From<PersistError> for Error {
    fn from(value: PersistError) -> Self {
        Self::Persist(value)
    }
}

/// A specialized [`Result`](core::result::Result) whose error is [`Error`].
///
/// # Examples
///
/// ```
/// use iqdb::{DistanceMetric, Iqdb, Result, Vector, VectorId};
///
/// fn run() -> Result<()> {
///     let db = Iqdb::open_in_memory(3, DistanceMetric::Cosine)?;
///     db.upsert(VectorId::from(1u64), Vector::new(vec![0.1, 0.2, 0.3])?, None)?;
///     assert_eq!(db.len(), 1);
///     Ok(())
/// }
/// # run().unwrap();
/// ```
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_iqdb_error_wraps_in_index_variant() {
        let err: Error = IqdbError::NotFound.into();
        assert!(matches!(err, Error::Index(IqdbError::NotFound)));
    }

    #[test]
    fn from_persist_error_wraps_in_persist_variant() {
        let err: Error = PersistError::Unsupported {
            feature: "compression",
            available_in: "the `zstd` cargo feature",
        }
        .into();
        assert!(matches!(
            err,
            Error::Persist(PersistError::Unsupported { .. })
        ));
    }

    #[test]
    fn display_delegates_to_inner_index_error() {
        let err = Error::Index(IqdbError::DimensionMismatch {
            expected: 3,
            found: 2,
        });
        assert_eq!(
            err.to_string(),
            "vector dimension mismatch: expected 3, found 2"
        );
    }

    #[test]
    fn display_config_variant_is_prefixed() {
        let err = Error::Config("dim must be greater than zero");
        assert_eq!(
            err.to_string(),
            "invalid configuration: dim must be greater than zero"
        );
    }

    #[test]
    fn source_is_present_for_wrapped_errors_and_absent_for_config() {
        use std::error::Error as _;
        assert!(Error::Index(IqdbError::NotFound).source().is_some());
        assert!(Error::Config("x").source().is_none());
    }
}
