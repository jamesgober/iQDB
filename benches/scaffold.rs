//! Criterion benchmark scaffold.
//!
//! The single placeholder bench below proves the harness wires up
//! correctly. Replace with real workloads as the engine comes online.
//! Rename this file (and the `[[bench]] name` entry in `Cargo.toml`)
//! to match the crate's bench naming convention once it's settled.

use criterion::{criterion_group, criterion_main, Criterion};

fn placeholder(c: &mut Criterion) {
    c.bench_function("noop", |b| b.iter(|| 0u64));
}

criterion_group!(benches, placeholder);
criterion_main!(benches);
