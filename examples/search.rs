//! Top-`k` similarity search walk-through.
//!
//! Builds a small in-memory store of labelled 2-D vectors, runs an
//! unfiltered cosine search, then narrows the candidate set with a
//! payload filter, then exercises the batch variant against three
//! probes at once.
//!
//! Run with:
//! ```sh
//! cargo run --example search --release
//! ```

use iqdb::{DistanceMetric, Iqdb, Payload, PayloadValue, Record, RecordId, Result, Vector};

fn main() -> Result<()> {
    let db = Iqdb::open_in_memory();

    // Three labelled embeddings.
    db.upsert(record(1, [1.0, 0.0], "rust"))?;
    db.upsert(record(2, [0.99, 0.10], "rust"))?;
    db.upsert(record(3, [0.0, 1.0], "python"))?;
    db.upsert(record(4, [0.71, 0.71], "go"))?;
    db.upsert(record(5, [-1.0, 0.0], "rust"))?;

    let probe = Vector::new(vec![1.0, 0.0])?;

    println!("== Unfiltered cosine top-3 ==");
    for hit in db.search(&probe, 3, DistanceMetric::Cosine)? {
        let topic = hit
            .payload
            .as_ref()
            .and_then(|p| p.get("topic"))
            .and_then(PayloadValue::as_text)
            .unwrap_or("?");
        println!(
            "  id={:<3} score={:>8.4}  topic={topic}",
            hit.id.get(),
            hit.score
        );
    }

    println!("\n== Filtered to topic=rust ==");
    let only_rust = db.search_with(&probe, 5, DistanceMetric::Cosine, |rec| {
        rec.payload()
            .and_then(|p| p.get("topic"))
            .and_then(PayloadValue::as_text)
            == Some("rust")
    })?;
    for hit in &only_rust {
        let topic = hit
            .payload
            .as_ref()
            .and_then(|p| p.get("topic"))
            .and_then(PayloadValue::as_text)
            .unwrap_or("?");
        println!(
            "  id={:<3} score={:>8.4}  topic={topic}",
            hit.id.get(),
            hit.score
        );
    }

    println!("\n== Batch of three probes ==");
    let probes = vec![
        Vector::new(vec![1.0, 0.0])?,
        Vector::new(vec![0.0, 1.0])?,
        Vector::new(vec![-1.0, 0.0])?,
    ];
    let batches = db.search_batch(&probes, 1, DistanceMetric::Cosine)?;
    for (probe, hits) in probes.iter().zip(batches.iter()) {
        let nearest = hits.first().map(|h| h.id.get()).unwrap_or(0);
        println!("  probe={:?} → nearest id={}", probe.as_slice(), nearest);
    }

    db.close()
}

fn record(id: u64, components: [f32; 2], topic: &str) -> Record {
    let vector = Vector::new(components.to_vec()).expect("finite components");
    let mut payload = Payload::new();
    let _ = payload.insert("topic", topic);
    Record::with_payload(RecordId::new(id), vector, payload)
}
