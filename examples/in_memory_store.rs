//! Walk-through of the v0.2.0 in-memory store: upsert records with
//! typed payloads, look them up, compare distances between them, and
//! delete an entry.
//!
//! Run with:
//! ```sh
//! cargo run --example in_memory_store --release
//! ```

use iqdb::{DistanceMetric, Iqdb, Payload, Record, RecordId, Result, Vector};

fn main() -> Result<()> {
    // An in-memory store never touches the filesystem.
    let db = Iqdb::open_in_memory();

    // Populate three short embeddings with metadata.
    db.upsert(record(1, [1.0, 0.0, 0.0], &[("topic", "rust")]))?;
    db.upsert(record(2, [0.0, 1.0, 0.0], &[("topic", "python")]))?;
    db.upsert(record(3, [0.0, 0.0, 1.0], &[("topic", "go")]))?;

    println!("stored {} records", db.len());

    // Look up record 2 and inspect its payload.
    if let Some(rec) = db.get(RecordId::new(2))? {
        let topic = rec
            .payload()
            .and_then(|p| p.get("topic"))
            .and_then(iqdb::PayloadValue::as_text)
            .unwrap_or("?");
        println!("id=2 → topic={topic}");
    }

    // Compare record 1 against the others under L2 and cosine.
    let r1 = db
        .get(RecordId::new(1))?
        .ok_or(iqdb::Error::InvalidConfig("missing seed record"))?;
    for id in [2_u64, 3] {
        if let Some(other) = db.get(RecordId::new(id))? {
            let l2 = DistanceMetric::L2.distance(r1.vector(), other.vector())?;
            let cos = DistanceMetric::Cosine.distance(r1.vector(), other.vector())?;
            println!("dist(1,{id}) → L2={l2:.4}  cosine={cos:.4}");
        }
    }

    // Remove record 3 and confirm.
    let removed = db.delete(RecordId::new(3))?;
    println!("delete(3) → removed={removed}, len={}", db.len());

    // Release the handle.
    db.close()?;
    Ok(())
}

fn record(id: u64, components: [f32; 3], meta: &[(&str, &str)]) -> Record {
    let vector = Vector::new(components.to_vec()).expect("finite components");
    if meta.is_empty() {
        Record::new(RecordId::new(id), vector)
    } else {
        let mut payload = Payload::new();
        for (k, v) in meta {
            let _ = payload.insert(*k, *v);
        }
        Record::with_payload(RecordId::new(id), vector, payload)
    }
}
