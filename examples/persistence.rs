// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Three sessions against one durable, file-backed database, showing that
//! state survives between process-like boundaries (each block opens, works,
//! and closes a fresh handle to the same file).
//!
//! Run with `cargo run --example persistence`.

use iqdb::{DistanceMetric, Iqdb, Result, Value, Vector, VectorId};

fn main() -> Result<()> {
    // A scratch location under the OS temp dir; cleaned up at the end.
    let dir = std::env::temp_dir().join("iqdb-persistence-example");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let path = dir.join("topics.iqdb");

    // Session 1: create, write three records with a "topic" tag, close.
    {
        let db = Iqdb::open(&path, 3, DistanceMetric::Cosine)?;
        for (id, vector, topic) in [
            (1u64, [1.0, 0.0, 0.0], "rust"),
            (2, [0.0, 1.0, 0.0], "databases"),
            (3, [0.0, 0.0, 1.0], "vectors"),
        ] {
            let meta = [("topic".to_string(), Value::String(topic.to_string()))]
                .into_iter()
                .collect();
            db.upsert(
                VectorId::from(id),
                Vector::new(vector.to_vec())?,
                Some(meta),
            )?;
        }
        db.flush()?;
        db.close()?;
        println!("session 1: wrote 3 records to {}", path.display());
    }

    // Session 2: reopen, confirm the data, search, delete one, close.
    {
        let db = Iqdb::open(&path, 3, DistanceMetric::Cosine)?;
        println!("session 2: reopened with {} records", db.len());
        let hits = db.search(&Vector::new(vec![1.0, 0.0, 0.0])?, 1)?;
        println!("  nearest to [1,0,0]: id={}", hits[0].id);
        db.delete(&VectorId::from(3u64))?;
        db.close()?;
    }

    // Session 3: reopen, confirm the delete persisted.
    {
        let db = Iqdb::open(&path, 3, DistanceMetric::Cosine)?;
        println!("session 3: now {} records", db.len());
        assert!(db.get(&VectorId::from(3u64))?.is_none());
        db.close()?;
    }

    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
