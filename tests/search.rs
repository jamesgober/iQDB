//! Integration tests for the v0.3.0 search surface.
//!
//! Exercises the four public entry points — `Iqdb::search`,
//! `search_with`, `search_batch`, `search_batch_with` — through the
//! public re-exports. Living outside the unit tests guarantees the
//! external API actually compiles and behaves as documented.

use iqdb::{DistanceMetric, Error, Iqdb, Payload, PayloadValue, Record, RecordId, Vector};

fn record(id: u64, components: Vec<f32>, topic: Option<&str>) -> Record {
    let v = Vector::new(components).expect("finite components");
    match topic {
        None => Record::new(RecordId::new(id), v),
        Some(label) => {
            let mut payload = Payload::new();
            let _ = payload.insert("topic", label);
            Record::with_payload(RecordId::new(id), v, payload)
        }
    }
}

fn seed(db: &Iqdb) {
    db.upsert(record(1, vec![1.0, 0.0, 0.0], Some("rust")))
        .unwrap();
    db.upsert(record(2, vec![0.99, 0.10, 0.0], Some("rust")))
        .unwrap();
    db.upsert(record(3, vec![0.0, 1.0, 0.0], Some("python")))
        .unwrap();
    db.upsert(record(4, vec![0.71, 0.71, 0.0], Some("go")))
        .unwrap();
    db.upsert(record(5, vec![-1.0, 0.0, 0.0], Some("rust")))
        .unwrap();
}

#[test]
fn search_returns_top_k_sorted_ascending() {
    let db = Iqdb::open_in_memory();
    seed(&db);
    let probe = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
    let hits = db.search(&probe, 3, DistanceMetric::Cosine).unwrap();
    assert_eq!(hits.len(), 3);
    for pair in hits.windows(2) {
        assert!(pair[0].score <= pair[1].score, "results must be ascending");
    }
}

#[test]
fn search_caps_at_k_when_store_exceeds_k() {
    let db = Iqdb::open_in_memory();
    seed(&db);
    let probe = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
    let hits = db.search(&probe, 2, DistanceMetric::L2).unwrap();
    assert_eq!(hits.len(), 2);
}

#[test]
fn search_returns_all_when_k_exceeds_store() {
    let db = Iqdb::open_in_memory();
    seed(&db);
    let probe = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
    let hits = db.search(&probe, 100, DistanceMetric::L2).unwrap();
    assert_eq!(hits.len(), 5);
}

#[test]
fn search_returns_empty_when_k_is_zero() {
    let db = Iqdb::open_in_memory();
    seed(&db);
    let probe = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
    let hits = db.search(&probe, 0, DistanceMetric::Cosine).unwrap();
    assert!(hits.is_empty());
}

#[test]
fn search_returns_empty_on_empty_store() {
    let db = Iqdb::open_in_memory();
    let probe = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
    let hits = db.search(&probe, 5, DistanceMetric::Cosine).unwrap();
    assert!(hits.is_empty());
}

#[test]
fn search_with_filter_excludes_records_before_admission() {
    let db = Iqdb::open_in_memory();
    seed(&db);
    let probe = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
    let only_rust = db
        .search_with(&probe, 5, DistanceMetric::Cosine, |rec| {
            rec.payload()
                .and_then(|p| p.get("topic"))
                .and_then(PayloadValue::as_text)
                == Some("rust")
        })
        .unwrap();
    // Three rust records exist; all should make it through.
    assert_eq!(only_rust.len(), 3);
    for hit in &only_rust {
        let topic = hit
            .payload
            .as_ref()
            .and_then(|p| p.get("topic"))
            .and_then(PayloadValue::as_text);
        assert_eq!(topic, Some("rust"));
    }
}

#[test]
fn search_with_filter_returning_false_yields_empty() {
    let db = Iqdb::open_in_memory();
    seed(&db);
    let probe = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
    let none = db
        .search_with(&probe, 5, DistanceMetric::Cosine, |_| false)
        .unwrap();
    assert!(none.is_empty());
}

#[test]
fn search_propagates_dimension_mismatch() {
    let db = Iqdb::open_in_memory();
    seed(&db); // stored vectors are dim 3
    let probe = Vector::new(vec![1.0, 0.0]).unwrap(); // dim 2
    let err = db.search(&probe, 5, DistanceMetric::L2).unwrap_err();
    assert!(matches!(err, Error::DimensionMismatch { .. }));
}

#[test]
fn search_attaches_payload_when_present() {
    let db = Iqdb::open_in_memory();
    seed(&db);
    let probe = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
    let hits = db.search(&probe, 1, DistanceMetric::Cosine).unwrap();
    assert_eq!(hits.len(), 1);
    let payload = hits[0].payload.as_ref().expect("payload preserved");
    assert!(payload.contains_key("topic"));
}

#[test]
fn search_omits_payload_for_records_without_one() {
    let db = Iqdb::open_in_memory();
    db.upsert(record(99, vec![1.0, 0.0, 0.0], None)).unwrap();
    let probe = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
    let hits = db.search(&probe, 1, DistanceMetric::L2).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].payload.is_none());
}

#[test]
fn search_batch_preserves_input_order() {
    let db = Iqdb::open_in_memory();
    seed(&db);
    let probes = vec![
        Vector::new(vec![1.0, 0.0, 0.0]).unwrap(),
        Vector::new(vec![0.0, 1.0, 0.0]).unwrap(),
        Vector::new(vec![-1.0, 0.0, 0.0]).unwrap(),
    ];
    let batches = db.search_batch(&probes, 1, DistanceMetric::Cosine).unwrap();
    assert_eq!(batches.len(), 3);
    // Nearest to (1,0,0) is id 1 ("rust" perfect match).
    assert_eq!(batches[0][0].id, RecordId::new(1));
    // Nearest to (0,1,0) is id 3 ("python").
    assert_eq!(batches[1][0].id, RecordId::new(3));
    // Nearest to (-1,0,0) is id 5 ("rust" opposite direction).
    assert_eq!(batches[2][0].id, RecordId::new(5));
}

#[test]
fn search_batch_returns_empty_per_query_on_empty_store() {
    let db = Iqdb::open_in_memory();
    let probes = vec![
        Vector::new(vec![1.0, 0.0]).unwrap(),
        Vector::new(vec![0.0, 1.0]).unwrap(),
    ];
    let batches = db.search_batch(&probes, 3, DistanceMetric::L2).unwrap();
    assert_eq!(batches.len(), 2);
    assert!(batches.iter().all(Vec::is_empty));
}

#[test]
fn search_batch_with_shared_filter_applies_to_every_query() {
    let db = Iqdb::open_in_memory();
    seed(&db);
    let probes = vec![
        Vector::new(vec![1.0, 0.0, 0.0]).unwrap(),
        Vector::new(vec![0.0, 1.0, 0.0]).unwrap(),
    ];
    let batches = db
        .search_batch_with(&probes, 5, DistanceMetric::Cosine, |rec| {
            rec.payload()
                .and_then(|p| p.get("topic"))
                .and_then(PayloadValue::as_text)
                == Some("rust")
        })
        .unwrap();
    // Even the python-favouring probe is filtered down to rust records.
    for batch in &batches {
        for hit in batch {
            let topic = hit
                .payload
                .as_ref()
                .and_then(|p| p.get("topic"))
                .and_then(PayloadValue::as_text);
            assert_eq!(topic, Some("rust"));
        }
    }
}

#[test]
fn search_supports_concurrent_readers_through_arc() {
    use std::sync::Arc;
    use std::thread;

    let db = Arc::new(Iqdb::open_in_memory());
    for id in 0..16_u64 {
        db.upsert(Record::new(
            RecordId::new(id),
            Vector::new(vec![id as f32, 0.0]).unwrap(),
        ))
        .unwrap();
    }

    let mut handles = Vec::new();
    for _ in 0..8 {
        let db = Arc::clone(&db);
        handles.push(thread::spawn(move || {
            let probe = Vector::new(vec![5.0, 0.0]).unwrap();
            let hits = db.search(&probe, 3, DistanceMetric::L2).expect("ok");
            assert_eq!(hits.len(), 3);
            // id 5 is the perfect match; should always be first.
            assert_eq!(hits[0].id, RecordId::new(5));
        }));
    }
    for h in handles {
        h.join().expect("worker panicked");
    }
}
