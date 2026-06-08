// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! The on-disk payload codec.
//!
//! [`iqdb_persist`] owns the snapshot framing — file header, CRC32, atomic
//! write, optional compression. This module owns the bytes *inside* that
//! frame: a self-describing record of the index kind + its config, the
//! database `dim` and `metric`, and every authoritative row. It is the half
//! of the durable format that belongs to `iqdb`, and the half that must stay
//! little-endian so a database written on one architecture reads back
//! identically on another.
//!
//! Layout (all multi-byte integers little-endian):
//!
//! ```text
//! magic "IQDC" (4) | version u32 | index-kind block | dim u64 | metric u8 | n_rows u64 | rows...
//! ```
//!
//! Each row is `VectorId | dim × f32 | metadata`. A [`VectorId`] is a tag
//! byte (`0` = `U64`, `1` = `Bytes`) followed by its body; metadata is a
//! presence byte, then an entry count and `key | Value` pairs. Each
//! [`Value`] is a tag byte plus its native little-endian body.

use std::io::{Read, Write};
use std::sync::Arc;

use iqdb_persist::{PersistError, Result};
use iqdb_types::{DistanceMetric, Metadata, Value, VectorId};

use crate::config::{HnswConfig, IndexKind, IvfConfig};
use crate::engine::store::{Row, RowStore};

/// Magic prefix identifying an `iqdb` core payload (distinct from the
/// `iqdb-persist` file magic that wraps it).
const MAGIC: [u8; 4] = *b"IQDC";

/// The payload format version. Bumped on any incompatible layout change.
const VERSION: u32 = 1;

/// A decoded payload: everything [`crate::engine::IqdbCore::load_from`] needs
/// to reconstruct the database.
#[derive(Debug)]
pub(crate) struct Decoded {
    pub(crate) kind: IndexKind,
    pub(crate) dim: usize,
    pub(crate) metric: DistanceMetric,
    pub(crate) rows: Vec<Row>,
}

/// Serialize the index kind, `dim`, `metric`, and every row in `store`.
pub(crate) fn encode(
    w: &mut dyn Write,
    kind: IndexKind,
    dim: usize,
    metric: DistanceMetric,
    store: &RowStore,
) -> Result<()> {
    w.write_all(&MAGIC).map_err(io)?;
    w.write_all(&VERSION.to_le_bytes()).map_err(io)?;
    encode_kind(w, kind)?;
    w.write_all(&to_u64(dim, "dim")?.to_le_bytes())
        .map_err(io)?;
    w.write_all(&[metric_tag(metric)?]).map_err(io)?;
    w.write_all(&to_u64(store.len(), "n_rows")?.to_le_bytes())
        .map_err(io)?;
    for row in store.iter() {
        encode_id(w, &row.id)?;
        for component in row.vector.iter() {
            w.write_all(&component.to_le_bytes()).map_err(io)?;
        }
        encode_meta(w, row.meta.as_ref())?;
    }
    Ok(())
}

/// Read a payload produced by [`encode`].
pub(crate) fn decode(r: &mut dyn Read) -> Result<Decoded> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).map_err(io)?;
    if magic != MAGIC {
        return Err(PersistError::InvalidPayload {
            reason: "iqdb core payload magic mismatch",
        });
    }
    if read_u32(r)? != VERSION {
        return Err(PersistError::InvalidPayload {
            reason: "unsupported iqdb core payload version",
        });
    }
    let kind = decode_kind(r)?;
    let dim = read_usize(r, "dim")?;
    let metric = metric_from_tag(read_u8(r)?)?;
    let n_rows = read_usize(r, "n_rows")?;

    let mut rows = Vec::with_capacity(n_rows);
    for _ in 0..n_rows {
        let id = decode_id(r)?;
        let mut buf = vec![0f32; dim];
        let mut b = [0u8; 4];
        for slot in buf.iter_mut() {
            r.read_exact(&mut b).map_err(io)?;
            *slot = f32::from_le_bytes(b);
        }
        let meta = decode_meta(r)?;
        rows.push(Row {
            id,
            vector: Arc::from(buf.into_boxed_slice()),
            meta,
        });
    }
    Ok(Decoded {
        kind,
        dim,
        metric,
        rows,
    })
}

// -- index kind ---------------------------------------------------------

fn encode_kind(w: &mut dyn Write, kind: IndexKind) -> Result<()> {
    w.write_all(&[kind.tag()]).map_err(io)?;
    match kind {
        IndexKind::Flat => {}
        IndexKind::Hnsw(c) => {
            for field in [c.m, c.ef_construction, c.ef_search, c.filter_widen] {
                w.write_all(&to_u64(field, "hnsw field")?.to_le_bytes())
                    .map_err(io)?;
            }
            w.write_all(&c.seed.to_le_bytes()).map_err(io)?;
        }
        IndexKind::Ivf(c) => {
            for field in [c.n_clusters, c.n_probes, c.training_sample_size] {
                w.write_all(&to_u64(field, "ivf field")?.to_le_bytes())
                    .map_err(io)?;
            }
            w.write_all(&[u8::from(c.use_pq)]).map_err(io)?;
            match c.pq_subvectors {
                Some(m) => {
                    w.write_all(&[1]).map_err(io)?;
                    w.write_all(&to_u64(m, "pq_subvectors")?.to_le_bytes())
                        .map_err(io)?;
                }
                None => w.write_all(&[0]).map_err(io)?,
            }
            w.write_all(&c.pq_refine_factor.to_le_bytes()).map_err(io)?;
            w.write_all(&c.seed.to_le_bytes()).map_err(io)?;
        }
    }
    Ok(())
}

fn decode_kind(r: &mut dyn Read) -> Result<IndexKind> {
    match read_u8(r)? {
        0 => Ok(IndexKind::Flat),
        1 => Ok(IndexKind::Hnsw(HnswConfig {
            m: read_usize(r, "hnsw.m")?,
            ef_construction: read_usize(r, "hnsw.ef_construction")?,
            ef_search: read_usize(r, "hnsw.ef_search")?,
            filter_widen: read_usize(r, "hnsw.filter_widen")?,
            seed: read_u64(r)?,
        })),
        2 => {
            let n_clusters = read_usize(r, "ivf.n_clusters")?;
            let n_probes = read_usize(r, "ivf.n_probes")?;
            let training_sample_size = read_usize(r, "ivf.training_sample_size")?;
            let use_pq = read_u8(r)? != 0;
            let pq_subvectors = match read_u8(r)? {
                0 => None,
                1 => Some(read_usize(r, "ivf.pq_subvectors")?),
                _ => {
                    return Err(PersistError::InvalidPayload {
                        reason: "ivf.pq_subvectors option tag",
                    });
                }
            };
            let pq_refine_factor = read_u32(r)?;
            let seed = read_u64(r)?;
            Ok(IndexKind::Ivf(IvfConfig {
                n_clusters,
                n_probes,
                training_sample_size,
                use_pq,
                pq_subvectors,
                pq_refine_factor,
                seed,
            }))
        }
        _ => Err(PersistError::InvalidPayload {
            reason: "unknown index-kind tag",
        }),
    }
}

// -- ids ----------------------------------------------------------------

fn encode_id(w: &mut dyn Write, id: &VectorId) -> Result<()> {
    match id {
        VectorId::U64(n) => {
            w.write_all(&[0]).map_err(io)?;
            w.write_all(&n.to_le_bytes()).map_err(io)?;
        }
        VectorId::Bytes(bytes) => {
            w.write_all(&[1]).map_err(io)?;
            w.write_all(&to_u32(bytes.len(), "id bytes len")?.to_le_bytes())
                .map_err(io)?;
            w.write_all(bytes).map_err(io)?;
        }
    }
    Ok(())
}

fn decode_id(r: &mut dyn Read) -> Result<VectorId> {
    match read_u8(r)? {
        0 => Ok(VectorId::U64(read_u64(r)?)),
        1 => {
            let len = read_u32(r)? as usize;
            if len == 0 {
                return Err(PersistError::InvalidPayload {
                    reason: "VectorId::Bytes key must not be empty",
                });
            }
            let bytes = read_vec(r, len)?;
            Ok(VectorId::Bytes(bytes.into_boxed_slice()))
        }
        _ => Err(PersistError::InvalidPayload {
            reason: "unknown VectorId tag",
        }),
    }
}

// -- metadata -----------------------------------------------------------

fn encode_meta(w: &mut dyn Write, meta: Option<&Metadata>) -> Result<()> {
    match meta {
        None => w.write_all(&[0]).map_err(io)?,
        Some(m) => {
            w.write_all(&[1]).map_err(io)?;
            w.write_all(&to_u32(m.len(), "metadata entries")?.to_le_bytes())
                .map_err(io)?;
            for (key, value) in m.iter() {
                encode_str(w, key)?;
                encode_value(w, value)?;
            }
        }
    }
    Ok(())
}

fn decode_meta(r: &mut dyn Read) -> Result<Option<Metadata>> {
    match read_u8(r)? {
        0 => Ok(None),
        1 => {
            let count = read_u32(r)? as usize;
            let mut entries = Vec::with_capacity(count);
            for _ in 0..count {
                let key = decode_str(r)?;
                let value = decode_value(r)?;
                entries.push((key, value));
            }
            Ok(Some(entries.into_iter().collect()))
        }
        _ => Err(PersistError::InvalidPayload {
            reason: "unknown metadata presence tag",
        }),
    }
}

fn encode_value(w: &mut dyn Write, value: &Value) -> Result<()> {
    match value {
        Value::String(s) => {
            w.write_all(&[0]).map_err(io)?;
            encode_str(w, s)?;
        }
        Value::Int(i) => {
            w.write_all(&[1]).map_err(io)?;
            w.write_all(&i.to_le_bytes()).map_err(io)?;
        }
        Value::Float(f) => {
            w.write_all(&[2]).map_err(io)?;
            w.write_all(&f.to_le_bytes()).map_err(io)?;
        }
        Value::Bool(b) => {
            w.write_all(&[3]).map_err(io)?;
            w.write_all(&[u8::from(*b)]).map_err(io)?;
        }
        Value::Null => w.write_all(&[4]).map_err(io)?,
    }
    Ok(())
}

fn decode_value(r: &mut dyn Read) -> Result<Value> {
    match read_u8(r)? {
        0 => Ok(Value::String(decode_str(r)?)),
        1 => Ok(Value::Int(i64::from_le_bytes(read_array(r)?))),
        2 => Ok(Value::Float(f64::from_le_bytes(read_array(r)?))),
        3 => Ok(Value::Bool(read_u8(r)? != 0)),
        4 => Ok(Value::Null),
        _ => Err(PersistError::InvalidPayload {
            reason: "unknown metadata Value tag",
        }),
    }
}

fn encode_str(w: &mut dyn Write, s: &str) -> Result<()> {
    w.write_all(&to_u32(s.len(), "string length")?.to_le_bytes())
        .map_err(io)?;
    w.write_all(s.as_bytes()).map_err(io)?;
    Ok(())
}

fn decode_str(r: &mut dyn Read) -> Result<String> {
    let len = read_u32(r)? as usize;
    let bytes = read_vec(r, len)?;
    String::from_utf8(bytes).map_err(|_| PersistError::InvalidPayload {
        reason: "metadata string is not valid UTF-8",
    })
}

// -- metric tags --------------------------------------------------------

fn metric_tag(metric: DistanceMetric) -> Result<u8> {
    match metric {
        DistanceMetric::Cosine => Ok(0),
        DistanceMetric::DotProduct => Ok(1),
        DistanceMetric::Euclidean => Ok(2),
        DistanceMetric::Manhattan => Ok(3),
        DistanceMetric::Hamming => Ok(4),
        _ => Err(PersistError::UnsupportedMetric { metric }),
    }
}

fn metric_from_tag(tag: u8) -> Result<DistanceMetric> {
    match tag {
        0 => Ok(DistanceMetric::Cosine),
        1 => Ok(DistanceMetric::DotProduct),
        2 => Ok(DistanceMetric::Euclidean),
        3 => Ok(DistanceMetric::Manhattan),
        4 => Ok(DistanceMetric::Hamming),
        _ => Err(PersistError::InvalidMetric { tag }),
    }
}

// -- low-level read/write helpers --------------------------------------

fn io(source: std::io::Error) -> PersistError {
    PersistError::Io {
        path: std::path::PathBuf::new(),
        source,
    }
}

fn to_u64(value: usize, what: &'static str) -> Result<u64> {
    u64::try_from(value).map_err(|_| PersistError::InvalidPayload { reason: what })
}

fn to_u32(value: usize, what: &'static str) -> Result<u32> {
    u32::try_from(value).map_err(|_| PersistError::InvalidPayload { reason: what })
}

fn read_array<const N: usize>(r: &mut dyn Read) -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    r.read_exact(&mut buf).map_err(io)?;
    Ok(buf)
}

fn read_u8(r: &mut dyn Read) -> Result<u8> {
    Ok(read_array::<1>(r)?[0])
}

fn read_u32(r: &mut dyn Read) -> Result<u32> {
    Ok(u32::from_le_bytes(read_array(r)?))
}

fn read_u64(r: &mut dyn Read) -> Result<u64> {
    Ok(u64::from_le_bytes(read_array(r)?))
}

fn read_usize(r: &mut dyn Read, what: &'static str) -> Result<usize> {
    usize::try_from(read_u64(r)?).map_err(|_| PersistError::InvalidPayload { reason: what })
}

fn read_vec(r: &mut dyn Read, len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).map_err(io)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn round_trip(
        kind: IndexKind,
        dim: usize,
        metric: DistanceMetric,
        store: &RowStore,
    ) -> Decoded {
        let mut bytes = Vec::new();
        encode(&mut bytes, kind, dim, metric, store).unwrap();
        decode(&mut &bytes[..]).unwrap()
    }

    #[test]
    fn empty_store_round_trips() {
        let store = RowStore::new();
        let d = round_trip(IndexKind::Flat, 4, DistanceMetric::Cosine, &store);
        assert_eq!(d.dim, 4);
        assert_eq!(d.metric, DistanceMetric::Cosine);
        assert_eq!(d.kind, IndexKind::Flat);
        assert!(d.rows.is_empty());
    }

    #[test]
    fn hnsw_and_ivf_kinds_round_trip() {
        let store = RowStore::new();
        let hnsw = IndexKind::Hnsw(HnswConfig::default().with_m(24).with_ef_search(80));
        assert_eq!(
            round_trip(hnsw, 8, DistanceMetric::Euclidean, &store).kind,
            hnsw
        );

        let ivf = IndexKind::Ivf(
            IvfConfig::default()
                .with_n_clusters(64)
                .with_use_pq(true)
                .with_pq_subvectors(Some(8)),
        );
        assert_eq!(
            round_trip(ivf, 8, DistanceMetric::Euclidean, &store).kind,
            ivf
        );
    }

    #[test]
    fn bytes_id_and_full_metadata_round_trip() {
        let mut store = RowStore::new();
        let meta: Metadata = [
            ("title".to_string(), Value::String("intro".to_string())),
            ("year".to_string(), Value::Int(2026)),
            ("score".to_string(), Value::Float(0.5)),
            ("ok".to_string(), Value::Bool(true)),
            ("nil".to_string(), Value::Null),
        ]
        .into_iter()
        .collect();
        let bytes_id = VectorId::try_from(vec![0xde, 0xad, 0xbe, 0xef]).unwrap();
        assert!(store.upsert(
            bytes_id.clone(),
            Arc::from(&[1.0f32, 2.0, 3.0][..]),
            Some(meta.clone())
        ));

        let d = round_trip(IndexKind::Flat, 3, DistanceMetric::Cosine, &store);
        assert_eq!(d.rows.len(), 1);
        assert_eq!(d.rows[0].id, bytes_id);
        assert_eq!(d.rows[0].vector.as_ref(), &[1.0, 2.0, 3.0]);
        assert_eq!(d.rows[0].meta.as_ref(), Some(&meta));
    }

    #[test]
    fn bad_magic_is_rejected() {
        let bytes = [0u8; 32];
        let err = decode(&mut &bytes[..]).unwrap_err();
        assert!(matches!(err, PersistError::InvalidPayload { .. }));
    }

    proptest! {
        #[test]
        fn arbitrary_rows_round_trip(
            dim in 1usize..6,
            rows in proptest::collection::vec(
                (0u64..1000, proptest::collection::vec(-1.0e6f32..1.0e6, 1..6)),
                0..20,
            ),
        ) {
            // Build a store where every row has exactly `dim` components.
            let mut store = RowStore::new();
            for (id, raw) in rows {
                let mut comps = raw;
                comps.resize(dim, 0.0);
                let _ = store.upsert(VectorId::from(id), Arc::from(comps.into_boxed_slice()), None);
            }
            let decoded = round_trip(IndexKind::Flat, dim, DistanceMetric::Cosine, &store);
            prop_assert_eq!(decoded.rows.len(), store.len());
            for (got, want) in decoded.rows.iter().zip(store.iter()) {
                prop_assert_eq!(&got.id, &want.id);
                prop_assert_eq!(got.vector.as_ref(), want.vector.as_ref());
            }
        }
    }
}
