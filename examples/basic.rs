// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! The smallest useful program: open, write, read, search, delete.
//!
//! Run with `cargo run --example basic`.

use iqdb::{DistanceMetric, Iqdb, Result, Vector, VectorId};

fn main() -> Result<()> {
    // A 3-dimensional, in-memory database compared under cosine distance.
    let db = Iqdb::open_in_memory(3, DistanceMetric::Cosine)?;

    db.upsert(
        VectorId::from(1u64),
        Vector::new(vec![1.0, 0.0, 0.0])?,
        None,
    )?;
    db.upsert(
        VectorId::from(2u64),
        Vector::new(vec![0.0, 1.0, 0.0])?,
        None,
    )?;
    db.upsert(
        VectorId::from(3u64),
        Vector::new(vec![0.9, 0.1, 0.0])?,
        None,
    )?;
    println!("stored {} vectors", db.len());

    // Look one up by id.
    if let Some((vector, _meta)) = db.get(&VectorId::from(3u64))? {
        println!("record 3 = {:?}", vector.as_slice());
    }

    // Nearest two to a query pointing along the first axis.
    let query = Vector::new(vec![1.0, 0.0, 0.0])?;
    for hit in db.search(&query, 2)? {
        println!("hit id={} distance={:.4}", hit.id, hit.distance);
    }

    // Idempotent delete: the boolean says whether anything was removed.
    println!("deleted 2? {}", db.delete(&VectorId::from(2u64))?);
    println!("remaining: {}", db.len());

    db.close()
}
