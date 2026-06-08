// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! The async surface: build a database and run concurrent searches off the
//! Tokio executor. Requires the `async` feature.
//!
//! Run with `cargo run --example async_search --features async`.

use std::sync::Arc;

use iqdb::{AsyncIqdb, DistanceMetric, Result, Vector, VectorId};

#[tokio::main]
async fn main() -> Result<()> {
    let db = AsyncIqdb::open_in_memory(2, DistanceMetric::Euclidean).await?;

    // Writes are awaited; each is offloaded to the blocking pool.
    for i in 0..10u64 {
        let x = i as f32;
        db.upsert(VectorId::from(i), Vector::new(vec![x, 0.0])?, None)
            .await?;
    }
    println!("stored {} vectors", db.len());

    // Fan out several searches concurrently — they run on the blocking pool
    // without stalling the executor. `AsyncIqdb` is `Clone` (shared `Arc`).
    let db = Arc::new(db);
    let mut tasks = Vec::new();
    for target in [0.0f32, 4.0, 9.0] {
        let db = Arc::clone(&db);
        tasks.push(tokio::spawn(async move {
            let hits = db
                .search(Vector::new(vec![target, 0.0]).unwrap(), 1)
                .await
                .unwrap();
            (target, hits[0].id.clone())
        }));
    }
    for task in tasks {
        let (target, id) = task.await.expect("task joined");
        println!("nearest to [{target}, 0] = id {id}");
    }

    // Unwrap the Arc to close cleanly (sole owner once tasks have joined).
    if let Ok(db) = Arc::try_unwrap(db) {
        db.close().await?;
    }
    Ok(())
}
