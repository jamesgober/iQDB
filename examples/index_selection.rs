// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Choosing an index through `IqdbConfig`: the exact flat default, an HNSW
//! graph, and an IVF clustering — plus a result cache and `optimize`.
//!
//! Run with `cargo run --example index_selection`.

use iqdb::{
    CacheConfig, DistanceMetric, HnswConfig, IndexKind, Iqdb, IqdbConfig, IvfConfig, Result,
    Vector, VectorId,
};

/// Build a small corpus into whichever database is passed.
fn load(db: &Iqdb) -> Result<()> {
    for i in 0..50u64 {
        let x = (i as f32) * 0.1;
        db.upsert(VectorId::from(i), Vector::new(vec![x, 1.0 - x])?, None)?;
    }
    Ok(())
}

fn nearest(db: &Iqdb, label: &str) -> Result<()> {
    let hits = db.search(&Vector::new(vec![0.0, 1.0])?, 3)?;
    let ids: Vec<String> = hits.iter().map(|h| h.id.to_string()).collect();
    println!("{label:<22} top-3 ids: {ids:?}");
    Ok(())
}

fn main() -> Result<()> {
    let metric = DistanceMetric::Euclidean;

    // Tier 1: the exact flat index, zero configuration.
    let flat = Iqdb::open_in_memory(2, metric)?;
    load(&flat)?;
    nearest(&flat, "flat (exact)")?;

    // Tier 2: an HNSW graph, tuned for a wider search beam, with a cache.
    let hnsw = Iqdb::open_in_memory_with(
        IqdbConfig::new(2, metric)
            .index(IndexKind::Hnsw(HnswConfig::default().with_ef_search(128)))
            .cache(CacheConfig::new().capacity(1_024)),
    )?;
    load(&hnsw)?;
    nearest(&hnsw, "hnsw (approximate)")?;
    // The second identical query is served from the cache.
    let _ = hnsw.search(&Vector::new(vec![0.0, 1.0])?, 3)?;
    if let Some(stats) = hnsw.cache_stats() {
        println!("  cache hits={} misses={}", stats.hits, stats.misses);
    }

    // Tier 2: an IVF clustering. It trains lazily on the first search; after
    // many writes, `optimize` retrains the centroids over the current data.
    let ivf = Iqdb::open_in_memory_with(IqdbConfig::new(2, metric).index(IndexKind::Ivf(
        IvfConfig::default().with_n_clusters(8).with_n_probes(8),
    )))?;
    load(&ivf)?;
    nearest(&ivf, "ivf (approximate)")?;
    ivf.optimize()?;
    nearest(&ivf, "ivf (after optimize)")?;

    flat.close()?;
    hnsw.close()?;
    ivf.close()
}
