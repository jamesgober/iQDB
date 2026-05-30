//! Integration-test scaffold.
//!
//! Exercises the public API surface of the crate. Add real
//! integration tests here as functionality lands.

use iqdb::Iqdb;

#[test]
fn open_in_memory_and_close() {
    let db = Iqdb::open_in_memory();
    db.close()
        .expect("close on in-memory handle should succeed");
}
