//! Minimal example — open an in-memory instance and close it.
//!
//! Run with:
//! ```sh
//! cargo run --example basic
//! ```

use iqdb::Iqdb;

fn main() {
    let db = Iqdb::open_in_memory();
    println!("iqdb instance opened (stub)");
    match db.close() {
        Ok(()) => println!("closed cleanly"),
        Err(err) => eprintln!("failed to close: {err}"),
    }
}
