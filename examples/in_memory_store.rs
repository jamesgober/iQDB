// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! A fuller in-memory walkthrough: metadata, replace-on-upsert, and a
//! metadata-filtered search.
//!
//! Run with `cargo run --example in_memory_store`.

use iqdb::{DistanceMetric, Filter, Iqdb, Metadata, Result, Value, Vector, VectorId};

fn meta(kind: &str, year: i64) -> Metadata {
    [
        ("kind".to_string(), Value::String(kind.to_string())),
        ("year".to_string(), Value::Int(year)),
    ]
    .into_iter()
    .collect()
}

fn main() -> Result<()> {
    let db = Iqdb::open_in_memory(4, DistanceMetric::Cosine)?;

    db.upsert(
        VectorId::from(1u64),
        Vector::new(vec![1.0, 0.0, 0.0, 0.0])?,
        Some(meta("doc", 2024)),
    )?;
    db.upsert(
        VectorId::from(2u64),
        Vector::new(vec![0.9, 0.1, 0.0, 0.0])?,
        Some(meta("doc", 2026)),
    )?;
    db.upsert(
        VectorId::from(3u64),
        Vector::new(vec![0.8, 0.2, 0.0, 0.0])?,
        Some(meta("image", 2026)),
    )?;

    // Upsert with an existing id replaces in place — the count is unchanged.
    db.upsert(
        VectorId::from(2u64),
        Vector::new(vec![0.95, 0.05, 0.0, 0.0])?,
        Some(meta("doc", 2027)),
    )?;
    println!("stored {} records", db.len());

    let (_, m) = db.get(&VectorId::from(2u64))?.expect("present");
    println!("record 2 metadata: {:?}", m);

    let query = Vector::new(vec![1.0, 0.0, 0.0, 0.0])?;

    // Unfiltered: the three nearest, any kind.
    println!("-- nearest, unfiltered --");
    for hit in db.search(&query, 3)? {
        println!("  id={} distance={:.4}", hit.id, hit.distance);
    }

    // Filtered: only documents published in 2026 or later.
    println!("-- nearest documents from 2026+ --");
    let filter = Filter::and(vec![
        Filter::eq("kind", Value::String("doc".into())),
        Filter::gte("year", Value::Int(2026)),
    ]);
    for hit in db.search_with(&query, 3, filter)? {
        println!("  id={} distance={:.4}", hit.id, hit.distance);
    }

    db.close()
}
