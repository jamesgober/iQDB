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

    /// The requested operation is not yet implemented.
    ///
    /// This crate ships from the `embed-db-template` scaffold with stub
    /// methods on the public handle. Returning this variant lets the
    /// generated crate compile and lint cleanly before the real engine
    /// is in place. Replace the stub paths in `db.rs` and remove this
    /// variant when the engine is wired up.
    NotImplemented,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "iqdb: io error ({})", err.kind()),
            Self::InvalidConfig(msg) => write!(f, "iqdb: invalid configuration ({msg})"),
            Self::NotImplemented => f.write_str("iqdb: not implemented"),
        }
    }
}

impl std::error::Error for Error {}

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
    fn not_implemented_display_is_stable() {
        let msg = format!("{}", Error::NotImplemented);
        assert!(msg.contains("not implemented"));
    }

    #[test]
    fn from_io_maps_to_io_variant() {
        let err: Error = std::io::Error::new(std::io::ErrorKind::NotFound, "missing").into();
        assert!(matches!(err, Error::Io(_)));
    }
}
