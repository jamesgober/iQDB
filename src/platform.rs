// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Cross-platform durability primitives for the file-backed store.
//!
//! Every supported platform ends up calling the strictest available
//! sync primitive at this layer:
//!
//! | OS        | Primitive                                  | Rust call site             |
//! |-----------|--------------------------------------------|----------------------------|
//! | Linux     | `fsync(2)` (drains the page cache + WAL)   | [`std::fs::File::sync_all`] |
//! | macOS     | `fcntl(fd, F_FULLFSYNC, 0)`                | direct `libc::fcntl`        |
//! | Windows   | `FlushFileBuffers`                         | [`std::fs::File::sync_all`] |
//! | other Unix| `fsync(2)`                                  | [`std::fs::File::sync_all`] |
//!
//! macOS is the only platform where the standard library's
//! [`File::sync_all`] is not enough: on Apple silicon and Intel macs
//! both, `fsync` returns once the data has reached the drive's
//! write-back cache, not the platter / flash. The drive controller's
//! cache can be lost on a power cut. `F_FULLFSYNC` is the documented
//! escape hatch — it asks the drive to flush its cache to durable
//! storage before returning. SQLite, Apple's own Core Data, and most
//! embedded databases use it for the same reason.
//!
//! The trade-off is throughput: `F_FULLFSYNC` is materially slower
//! than `fsync` on rotating media, and somewhat slower on SSDs.
//! Callers that prioritise throughput over durability can still get
//! the faster `fsync` semantics by calling `File::sync_all` directly
//! — but the public [`Iqdb::flush`](crate::Iqdb::flush) and
//! [`Iqdb::close`](crate::Iqdb::close) entry points always take the
//! strongest available primitive.
//!
//! [`File::sync_all`]: std::fs::File::sync_all

use std::fs::File;
use std::io;

/// Flush the file's user-space buffers to durable storage.
///
/// See the module-level documentation for the per-platform primitive
/// this resolves to. The call is synchronous — it does not return
/// until the OS reports the flush has completed (or has failed).
///
/// # Errors
///
/// Propagates the OS-level error verbatim as `std::io::Error`. Common
/// failures: `ENOSPC` (disk full), `EIO` (hardware error), `EBADF`
/// (file already closed).
#[inline]
pub(crate) fn full_sync(file: &File) -> io::Result<()> {
    full_sync_impl(file)
}

#[cfg(target_os = "macos")]
fn full_sync_impl(file: &File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;
    // SAFETY: `file.as_raw_fd()` returns a valid, open file descriptor
    // owned by `file`. `libc::fcntl` with `F_FULLFSYNC` reads no
    // arbitrary pointer — the third arg is ignored under this command,
    // and `0` is the convention every Apple example follows. The
    // call neither moves nor invalidates `file`.
    let ret = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC, 0) };
    if ret == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn full_sync_impl(file: &File) -> io::Result<()> {
    // Linux, FreeBSD, OpenBSD, NetBSD, illumos: `fsync(2)` is the
    // strongest portable primitive. `File::sync_all` resolves to
    // `fsync` in std.
    file.sync_all()
}

#[cfg(windows)]
fn full_sync_impl(file: &File) -> io::Result<()> {
    // Windows: `File::sync_all` resolves to `FlushFileBuffers`,
    // which is the documented strongest sync primitive. There is no
    // `F_FULLFSYNC`-equivalent escape hatch on NTFS.
    file.sync_all()
}

// Fallback for cfg combinations the matrix above does not cover.
// All Tier-1 targets (linux / macos / windows on x86_64 + aarch64)
// hit one of the three branches above; this exists only so that
// `cargo check --target some-unknown-os` does not stop the build.
#[cfg(not(any(unix, windows)))]
fn full_sync_impl(file: &File) -> io::Result<()> {
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn full_sync_succeeds_on_a_simple_write() {
        let dir = tempdir();
        let path = dir.join("sync-test.bin");
        let mut file = File::create(&path).expect("create");
        file.write_all(b"hello").expect("write");
        full_sync(&file).expect("sync");
    }

    /// Tiny dedicated tempdir helper so the test suite stays free of a
    /// `tempfile` dev-dependency. Returns a path inside the system
    /// temp directory and creates it; the directory is left in place
    /// when the test exits — the OS cleans up `TMP` on its own
    /// schedule, and the test workload is a few hundred bytes.
    fn tempdir() -> std::path::PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("iqdb-test-{nanos}"));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }
}
