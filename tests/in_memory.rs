//! Integration tests for the v0.2.0 in-memory store surface.
//!
//! Exercises the public CRUD API through the [`Iqdb`] handle and the
//! optional `serde` feature gates. Lives outside the unit tests so it
//! also validates re-export correctness — the public API is what
//! external crates compile against.

use iqdb::{DistanceMetric, Error, Iqdb, Payload, PayloadValue, Record, RecordId, Vector};

#[test]
fn lifecycle_upsert_get_delete_round_trip() {
    let db = Iqdb::open_in_memory();
    assert!(db.is_empty());

    let v = Vector::new(vec![0.1, 0.2, 0.3]).expect("finite");
    db.upsert(Record::new(RecordId::new(1), v.clone()))
        .expect("upsert ok");
    assert_eq!(db.len(), 1);

    let hit = db
        .get(RecordId::new(1))
        .expect("get ok")
        .expect("record present");
    assert_eq!(hit.vector().as_slice(), v.as_slice());

    let miss = db.get(RecordId::new(99)).expect("get ok");
    assert!(miss.is_none());

    let removed = db.delete(RecordId::new(1)).expect("delete ok");
    assert!(removed);
    assert!(db.is_empty());

    let removed_again = db.delete(RecordId::new(1)).expect("delete ok");
    assert!(!removed_again);
}

#[test]
fn upsert_replaces_existing_record() {
    let db = Iqdb::open_in_memory();
    db.upsert(Record::new(
        RecordId::new(1),
        Vector::new(vec![1.0, 0.0]).unwrap(),
    ))
    .unwrap();
    db.upsert(Record::new(
        RecordId::new(1),
        Vector::new(vec![0.0, 1.0]).unwrap(),
    ))
    .unwrap();
    assert_eq!(db.len(), 1);
    let hit = db.get(RecordId::new(1)).unwrap().expect("record present");
    assert_eq!(hit.vector().as_slice(), &[0.0, 1.0]);
}

#[test]
fn payload_is_preserved_across_upsert_and_get() {
    let db = Iqdb::open_in_memory();

    let mut payload = Payload::new();
    let _ = payload.insert("source", "wikipedia");
    let _ = payload.insert("year", 2026_i64);
    let _ = payload.insert("verified", true);

    db.upsert(Record::with_payload(
        RecordId::new(42),
        Vector::new(vec![0.5, 0.5, 0.5]).unwrap(),
        payload,
    ))
    .unwrap();

    let hit = db.get(RecordId::new(42)).unwrap().expect("present");
    let payload = hit.payload().expect("payload preserved");

    assert_eq!(
        payload.get("source").and_then(PayloadValue::as_text),
        Some("wikipedia")
    );
    assert_eq!(
        payload.get("year").and_then(PayloadValue::as_int),
        Some(2026)
    );
    assert_eq!(
        payload.get("verified").and_then(PayloadValue::as_bool),
        Some(true)
    );
}

#[test]
fn distance_metric_dispatch_returns_consistent_ordering() {
    let a = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
    let b = Vector::new(vec![1.0, 0.0, 0.0]).unwrap();
    let c = Vector::new(vec![0.0, 1.0, 0.0]).unwrap();

    // Identical → 0; orthogonal → larger. Holds for all three metrics
    // because Dot is negated to satisfy the smaller-is-closer rule.
    for metric in [
        DistanceMetric::L2,
        DistanceMetric::Cosine,
        DistanceMetric::Dot,
    ] {
        let identical = metric.distance(&a, &b).unwrap();
        let orthogonal = metric.distance(&a, &c).unwrap();
        assert!(
            identical <= orthogonal,
            "{metric:?}: identical={identical} should be ≤ orthogonal={orthogonal}",
        );
    }
}

#[test]
fn distance_rejects_dimension_mismatch_through_public_api() {
    let a = Vector::new(vec![1.0, 2.0]).unwrap();
    let b = Vector::new(vec![1.0, 2.0, 3.0]).unwrap();
    let err = DistanceMetric::L2.distance(&a, &b).unwrap_err();
    assert!(matches!(
        err,
        Error::DimensionMismatch { left: 2, right: 3 }
    ));
}

#[test]
fn vector_validation_rejects_empty_and_non_finite() {
    assert!(matches!(
        Vector::new(Vec::<f32>::new()),
        Err(Error::InvalidVector { .. })
    ));
    assert!(matches!(
        Vector::new(vec![1.0, f32::NAN]),
        Err(Error::InvalidVector { .. })
    ));
    assert!(matches!(
        Vector::new(vec![f32::INFINITY]),
        Err(Error::InvalidVector { .. })
    ));
}

#[test]
fn flush_and_open_path_still_not_implemented_in_v0_2_0() {
    let db = Iqdb::open_in_memory();
    assert!(matches!(db.flush(), Err(Error::NotImplemented)));
    assert!(matches!(
        Iqdb::open("/tmp/iqdb-test"),
        Err(Error::NotImplemented)
    ));
    // Closing the in-memory handle is always Ok.
    assert!(db.close().is_ok());
}

#[test]
fn handle_is_shareable_via_arc() {
    use std::sync::Arc;
    use std::thread;

    let db = Arc::new(Iqdb::open_in_memory());
    let mut handles = Vec::new();
    for id in 0..32_u64 {
        let db = Arc::clone(&db);
        handles.push(thread::spawn(move || {
            let v = Vector::new(vec![id as f32; 4]).unwrap();
            db.upsert(Record::new(RecordId::new(id), v)).unwrap();
        }));
    }
    for h in handles {
        h.join().expect("worker panicked");
    }
    assert_eq!(db.len(), 32);
    for id in 0..32 {
        let hit = db.get(RecordId::new(id)).unwrap();
        assert!(hit.is_some());
    }
}

#[cfg(feature = "serde")]
mod serde_round_trip {
    use super::*;

    #[test]
    fn vector_round_trips_through_json() {
        let v = Vector::new(vec![1.0, 2.0, 3.0]).unwrap();
        let json = serde_json::to_string(&v).expect("serialize");
        let decoded: Vector = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(v.as_slice(), decoded.as_slice());
    }

    #[test]
    fn record_with_payload_round_trips_through_json() {
        let mut payload = Payload::new();
        let _ = payload.insert("kind", "doc");
        let _ = payload.insert("count", 3_i64);

        let original = Record::with_payload(
            RecordId::new(77),
            Vector::new(vec![0.4, 0.5, 0.6]).unwrap(),
            payload,
        );

        let json = serde_json::to_string(&original).expect("serialize");
        let decoded: Record = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(decoded.id(), original.id());
        assert_eq!(decoded.vector().as_slice(), original.vector().as_slice());
        let payload = decoded.payload().expect("payload preserved");
        assert_eq!(
            payload.get("kind").and_then(PayloadValue::as_text),
            Some("doc")
        );
        assert_eq!(payload.get("count").and_then(PayloadValue::as_int), Some(3));
    }

    #[test]
    fn distance_metric_round_trips_through_json() {
        for metric in [
            DistanceMetric::L2,
            DistanceMetric::Cosine,
            DistanceMetric::Dot,
        ] {
            let json = serde_json::to_string(&metric).expect("serialize");
            let decoded: DistanceMetric = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(decoded, metric);
        }
    }
}
