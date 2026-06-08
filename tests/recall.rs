// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Recall validation for the approximate indices.
//!
//! The exact flat index is the ground truth. These tests build the same
//! corpus under flat and under each approximate index, then measure the
//! mean overlap of their top-`k` results across many queries. They guard the
//! composition: that routing a query through HNSW or IVF inside the `iqdb`
//! handle returns very nearly the same neighbours the exact path would.
//!
//! Data is deterministic (seeded `fastrand`), so the thresholds are stable.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;

use iqdb::{
    DistanceMetric, Hit, HnswConfig, IndexKind, Iqdb, IqdbConfig, IvfConfig, Vector, VectorId,
};

const DIM: usize = 16;
const N: usize = 400;
const QUERIES: usize = 40;
const K: usize = 10;

fn id_u64(hit: &Hit) -> u64 {
    match hit.id {
        VectorId::U64(n) => n,
        VectorId::Bytes(_) => unreachable!("synthetic corpus uses u64 ids"),
    }
}

/// A small clustered corpus plus a query set drawn from the same clusters.
fn corpus() -> (Vec<Vec<f32>>, Vec<Vec<f32>>) {
    let mut rng = fastrand::Rng::with_seed(0xC0FF_EE12);
    let clusters = 8;
    let centers: Vec<Vec<f32>> = (0..clusters)
        .map(|_| (0..DIM).map(|_| rng.f32() * 10.0).collect())
        .collect();
    let jitter = |rng: &mut fastrand::Rng, c: &[f32]| -> Vec<f32> {
        c.iter().map(|&x| x + (rng.f32() - 0.5) * 0.5).collect()
    };
    let base = (0..N)
        .map(|i| jitter(&mut rng, &centers[i % clusters]))
        .collect();
    let queries = (0..QUERIES)
        .map(|i| jitter(&mut rng, &centers[i % clusters]))
        .collect();
    (base, queries)
}

fn load(db: &Iqdb, base: &[Vec<f32>]) {
    for (i, row) in base.iter().enumerate() {
        db.upsert(
            VectorId::from(i as u64),
            Vector::new(row.clone()).unwrap(),
            None,
        )
        .unwrap();
    }
}

/// Mean recall@k of `approx` measured against the exact `oracle`.
fn mean_recall(oracle: &Iqdb, approx: &Iqdb, queries: &[Vec<f32>]) -> f64 {
    let mut sum = 0.0;
    for q in queries {
        let query = Vector::new(q.clone()).unwrap();
        let truth: HashSet<u64> = oracle
            .search(&query, K)
            .unwrap()
            .iter()
            .map(id_u64)
            .collect();
        let got = approx.search(&query, K).unwrap();
        let hits = got.iter().filter(|h| truth.contains(&id_u64(h))).count();
        sum += hits as f64 / K as f64;
    }
    sum / queries.len() as f64
}

#[test]
fn hnsw_recall_tracks_the_flat_oracle() {
    let (base, queries) = corpus();

    let flat = Iqdb::open_in_memory(DIM, DistanceMetric::Euclidean).unwrap();
    load(&flat, &base);

    let hnsw = Iqdb::open_in_memory_with(
        IqdbConfig::new(DIM, DistanceMetric::Euclidean)
            .index(IndexKind::Hnsw(HnswConfig::default().with_ef_search(128))),
    )
    .unwrap();
    load(&hnsw, &base);

    let recall = mean_recall(&flat, &hnsw, &queries);
    assert!(recall >= 0.9, "HNSW recall@{K} too low: {recall:.3}");
}

#[test]
fn ivf_recall_tracks_the_flat_oracle_when_probing_all_clusters() {
    let (base, queries) = corpus();

    let flat = Iqdb::open_in_memory(DIM, DistanceMetric::Euclidean).unwrap();
    load(&flat, &base);

    // Probe every cluster: IVF then visits all candidates, so recall is
    // effectively exact — this validates the clustering + scoring path.
    let ivf = Iqdb::open_in_memory_with(
        IqdbConfig::new(DIM, DistanceMetric::Euclidean).index(IndexKind::Ivf(
            IvfConfig::default()
                .with_n_clusters(16)
                .with_n_probes(16)
                .with_seed(7),
        )),
    )
    .unwrap();
    load(&ivf, &base);

    let recall = mean_recall(&flat, &ivf, &queries);
    assert!(recall >= 0.95, "IVF recall@{K} too low: {recall:.3}");
}

#[test]
fn flat_recall_against_itself_is_perfect() {
    // Sanity check on the harness: the oracle vs itself must be 1.0.
    let (base, queries) = corpus();
    let flat = Iqdb::open_in_memory(DIM, DistanceMetric::Euclidean).unwrap();
    load(&flat, &base);
    let recall = mean_recall(&flat, &flat, &queries);
    assert!(
        (recall - 1.0).abs() < 1e-9,
        "self-recall must be 1.0, got {recall:.3}"
    );
}
