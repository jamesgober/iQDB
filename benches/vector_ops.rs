//! Criterion micro-benchmarks for the v0.2.0 vector primitives and
//! in-memory store hot path.
//!
//! Groups:
//!
//! - `vector_new` — construction-time validation cost across small,
//!   medium, and large dimensionalities (32 / 128 / 1024).
//! - `distance` — single-shot distance computation under each of the
//!   three [`DistanceMetric`] variants at dim 128.
//! - `store` — `upsert` and `get` throughput against a populated
//!   in-memory store at 1 000 records, dim 128.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use iqdb::{DistanceMetric, Iqdb, Payload, Record, RecordId, Vector};

fn random_vec(dim: usize, rng: &mut fastrand::Rng) -> Vec<f32> {
    (0..dim).map(|_| rng.f32() - 0.5).collect()
}

fn bench_vector_new(c: &mut Criterion) {
    let mut group = c.benchmark_group("vector_new");
    let mut rng = fastrand::Rng::with_seed(42);
    for &dim in &[32_usize, 128, 1024] {
        group.throughput(Throughput::Elements(dim as u64));
        let data = random_vec(dim, &mut rng);
        group.bench_with_input(BenchmarkId::from_parameter(dim), &data, |b, data| {
            b.iter(|| Vector::new(black_box(data.clone())).expect("finite"));
        });
    }
    group.finish();
}

fn bench_distance(c: &mut Criterion) {
    let mut group = c.benchmark_group("distance");
    let mut rng = fastrand::Rng::with_seed(7);
    let a = Vector::new(random_vec(128, &mut rng)).expect("finite");
    let b = Vector::new(random_vec(128, &mut rng)).expect("finite");
    for metric in [
        DistanceMetric::L2,
        DistanceMetric::Cosine,
        DistanceMetric::Dot,
    ] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{metric:?}")),
            &metric,
            |bencher, metric| {
                bencher.iter(|| {
                    let _ = metric
                        .distance(black_box(&a), black_box(&b))
                        .expect("equal dims");
                });
            },
        );
    }
    group.finish();
}

fn populate_db(n: usize, dim: usize, seed: u64) -> Iqdb {
    let mut rng = fastrand::Rng::with_seed(seed);
    let db = Iqdb::open_in_memory();
    for id in 0..n {
        let v = Vector::new(random_vec(dim, &mut rng)).expect("finite");
        db.upsert(Record::new(RecordId::new(id as u64), v))
            .expect("upsert");
    }
    db
}

fn bench_store(c: &mut Criterion) {
    let mut group = c.benchmark_group("store");

    // get against a populated store
    let populated = populate_db(1_000, 128, 11);
    group.bench_function("get_hit_1k_dim128", |b| {
        let mut next: u64 = 0;
        b.iter(|| {
            let id = RecordId::new(next % 1_000);
            next = next.wrapping_add(1);
            let _ = populated.get(black_box(id)).expect("ok");
        });
    });

    // upsert into a fresh store each time, so the bench measures
    // single-record write throughput rather than the rehash cost
    // of a growing HashMap.
    group.bench_function("upsert_fresh_dim128", |b| {
        let mut rng = fastrand::Rng::with_seed(99);
        let mut next: u64 = 0;
        b.iter_batched(
            || (Iqdb::open_in_memory(), random_vec(128, &mut rng)),
            |(db, components)| {
                let v = Vector::new(components).expect("finite");
                let id = RecordId::new(next);
                next = next.wrapping_add(1);
                db.upsert(Record::new(id, v)).expect("upsert");
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn populate_db_with_payloads(n: usize, dim: usize, seed: u64) -> Iqdb {
    let mut rng = fastrand::Rng::with_seed(seed);
    let db = Iqdb::open_in_memory();
    for id in 0..n {
        let v = Vector::new(random_vec(dim, &mut rng)).expect("finite");
        let mut payload = Payload::new();
        // Alternate between two labels so payload filters can prune
        // roughly half the corpus.
        let label = if id % 2 == 0 { "doc" } else { "image" };
        let _ = payload.insert("kind", label);
        db.upsert(Record::with_payload(RecordId::new(id as u64), v, payload))
            .expect("upsert");
    }
    db
}

fn bench_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("search");

    let mut rng = fastrand::Rng::with_seed(31);
    let probe128 = Vector::new(random_vec(128, &mut rng)).expect("finite");

    for &n in &[1_000_usize, 10_000] {
        let db = populate_db_with_payloads(n, 128, n as u64);
        let probe = probe128.clone();

        group.bench_with_input(BenchmarkId::new("flat_k10_dim128", n), &n, |b, &_n| {
            b.iter(|| {
                let _ = db
                    .search(black_box(&probe), 10, DistanceMetric::L2)
                    .expect("ok");
            });
        });

        let probe_for_filter = probe.clone();
        group.bench_with_input(
            BenchmarkId::new("flat_k10_dim128_filter_half", n),
            &n,
            |b, &_n| {
                b.iter(|| {
                    let _ = db
                        .search_with(
                            black_box(&probe_for_filter),
                            10,
                            DistanceMetric::L2,
                            |rec| {
                                rec.payload()
                                    .and_then(|p| p.get("kind"))
                                    .and_then(iqdb::PayloadValue::as_text)
                                    == Some("doc")
                            },
                        )
                        .expect("ok");
                });
            },
        );

        let probe_for_batch = probe.clone();
        group.bench_with_input(BenchmarkId::new("batch4_k10_dim128", n), &n, |b, &_n| {
            let probes = vec![
                probe_for_batch.clone(),
                probe_for_batch.clone(),
                probe_for_batch.clone(),
                probe_for_batch.clone(),
            ];
            b.iter(|| {
                let _ = db
                    .search_batch(black_box(&probes), 10, DistanceMetric::L2)
                    .expect("ok");
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_vector_new,
    bench_distance,
    bench_store,
    bench_search,
);
criterion_main!(benches);
