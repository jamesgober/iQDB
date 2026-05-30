//! Minimal example — open an in-memory instance, store a vector,
//! read it back, and close the handle.
//!
//! Run with:
//! ```sh
//! cargo run --example basic
//! ```

use iqdb::{Iqdb, Record, RecordId, Result, Vector};

fn main() -> Result<()> {
    let db = Iqdb::open_in_memory();

    db.upsert(Record::new(
        RecordId::new(1),
        Vector::new(vec![0.1_f32, 0.2, 0.3])?,
    ))?;

    if let Some(hit) = db.get(RecordId::new(1))? {
        println!("stored vector: {:?}", hit.vector().as_slice());
    }

    db.close()
}
