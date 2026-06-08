// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Hot-path benchmarks: top-`k` search throughput on the exact flat index
//! and the approximate HNSW index, plus the durable write path.
//!
//! Run with `cargo bench --bench search`. Baselines are tracked across
//! releases; a regression beyond the documented threshold blocks a release.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use iqdb::{DistanceMetric, HnswConfig, IndexKind, Iqdb, IqdbConfig, Vector, VectorId};

const DIM: usize = 64;
const N: usize = 1_000;
const K: usize = 10;

fn data(seed: u64) -> Vec<Vec<f32>> {
    let mut rng = fastrand::Rng::with_seed(seed);
    (0..N)
        .map(|_| (0..DIM).map(|_| rng.f32()).collect())
        .collect()
}

fn load(db: &Iqdb, base: &[Vec<f32>]) {
    for (i, row) in base.iter().enumerate() {
        db.upsert(
            VectorId::from(i as u64),
            Vector::new(row.clone()).expect("valid vector"),
            None,
        )
        .expect("upsert");
    }
}

fn bench_search(c: &mut Criterion) {
    let base = data(1);
    let query = Vector::new(base[0].clone()).expect("valid query");

    let flat = Iqdb::open_in_memory(DIM, DistanceMetric::Cosine).expect("open flat");
    load(&flat, &base);
    let _ = c.bench_function("flat/search_dim64_n1000_k10", |b| {
        b.iter(|| flat.search(black_box(&query), K).expect("search"));
    });

    let hnsw = Iqdb::open_in_memory_with(
        IqdbConfig::new(DIM, DistanceMetric::Cosine).index(IndexKind::Hnsw(HnswConfig::default())),
    )
    .expect("open hnsw");
    load(&hnsw, &base);
    let _ = c.bench_function("hnsw/search_dim64_n1000_k10", |b| {
        b.iter(|| hnsw.search(black_box(&query), K).expect("search"));
    });
}

fn bench_upsert(c: &mut Criterion) {
    let base = data(2);
    let _ = c.bench_function("flat/upsert_dim64", |b| {
        b.iter_with_setup(
            || Iqdb::open_in_memory(DIM, DistanceMetric::Cosine).expect("open"),
            |db| {
                for (i, row) in base.iter().enumerate() {
                    db.upsert(
                        VectorId::from(i as u64),
                        Vector::new(row.clone()).expect("valid"),
                        None,
                    )
                    .expect("upsert");
                }
                black_box(db.len())
            },
        );
    });
}

criterion_group!(benches, bench_search, bench_upsert);
criterion_main!(benches);
