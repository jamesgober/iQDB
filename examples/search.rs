// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Search variants: top-`k`, batch, and the effect of the distance metric.
//!
//! Run with `cargo run --example search`.

use iqdb::{DistanceMetric, Iqdb, Result, Vector, VectorId};

fn build(metric: DistanceMetric) -> Result<Iqdb> {
    let db = Iqdb::open_in_memory(2, metric)?;
    db.upsert(VectorId::from(1u64), Vector::new(vec![1.0, 0.0])?, None)?;
    db.upsert(VectorId::from(2u64), Vector::new(vec![0.0, 1.0])?, None)?;
    db.upsert(VectorId::from(3u64), Vector::new(vec![2.0, 0.0])?, None)?;
    Ok(db)
}

fn main() -> Result<()> {
    // Cosine treats [1,0] and [2,0] as identical in direction, so both rank
    // ahead of [0,1] for a query along the first axis.
    let cosine = build(DistanceMetric::Cosine)?;
    println!("-- cosine, top 3 --");
    for hit in cosine.search(&Vector::new(vec![1.0, 0.0])?, 3)? {
        println!("  id={} distance={:.4}", hit.id, hit.distance);
    }

    // Euclidean is magnitude-sensitive: [1,0] is closer to the query than
    // [2,0], so the order differs from cosine.
    let euclid = build(DistanceMetric::Euclidean)?;
    println!("-- euclidean, top 3 --");
    for hit in euclid.search(&Vector::new(vec![1.0, 0.0])?, 3)? {
        println!("  id={} distance={:.4}", hit.id, hit.distance);
    }

    // Batch search runs one query list per input, in order.
    let queries = vec![Vector::new(vec![1.0, 0.0])?, Vector::new(vec![0.0, 1.0])?];
    println!("-- batch (cosine), top 1 each --");
    for (i, hits) in cosine.search_batch(&queries, 1)?.into_iter().enumerate() {
        println!("  query {i}: nearest id={}", hits[0].id);
    }

    cosine.close()?;
    euclid.close()
}
