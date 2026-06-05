//! Open a directory-backed `iqdb`, write a few records, close cleanly,
//! reopen, and prove the records survived.
//!
//! Run with:
//! ```sh
//! cargo run --example persistence --release
//! ```
//!
//! The example creates `./data/iqdb-persistence-demo/` under the
//! current working directory and leaves it in place when done — feel
//! free to `rm -rf` it once you have looked at the on-disk files.

use std::path::PathBuf;

use iqdb::{DistanceMetric, Iqdb, Payload, Record, RecordId, Result, Vector};

fn main() -> Result<()> {
    let dir: PathBuf = "./data/iqdb-persistence-demo".into();

    println!("== Session 1: open, upsert, close ==");
    {
        let db = Iqdb::open(&dir)?;
        println!("opened {dir:?} (len={})", db.len());

        for (id, components, topic) in [
            (1_u64, vec![1.0, 0.0, 0.0], "rust"),
            (2, vec![0.99, 0.10, 0.0], "rust"),
            (3, vec![0.0, 1.0, 0.0], "python"),
        ] {
            let mut payload = Payload::new();
            let _ = payload.insert("topic", topic);
            db.upsert(Record::with_payload(
                RecordId::new(id),
                Vector::new(components)?,
                payload,
            ))?;
        }
        println!("upserted {} records", db.len());

        db.flush()?;
        println!("flushed (WAL synced to durable storage)");

        db.close()?;
        println!("closed (snapshot rewritten, WAL truncated)");
    }

    println!("\n== Session 2: reopen, verify, search ==");
    {
        let db = Iqdb::open(&dir)?;
        println!("reopened {dir:?} (len={})", db.len());
        assert_eq!(db.len(), 3, "all three records should survive close()");

        let probe = Vector::new(vec![1.0, 0.0, 0.0])?;
        let hits = db.search(&probe, 2, DistanceMetric::Cosine)?;
        println!("nearest 2 to {:?}:", probe.as_slice());
        for hit in &hits {
            println!("  id={:<3} score={:>8.4}", hit.id.get(), hit.score);
        }

        // Delete one and close again; next open should reflect the delete.
        let _ = db.delete(RecordId::new(3))?;
        db.close()?;
    }

    println!("\n== Session 3: confirm delete persisted ==");
    {
        let db = Iqdb::open(&dir)?;
        println!("reopened {dir:?} (len={})", db.len());
        assert_eq!(db.len(), 2);
        assert!(db.get(RecordId::new(3))?.is_none());
        db.close()?;
    }

    println!("\nall sessions consistent");
    Ok(())
}
