// Copyright 2026 James Gober. Licensed under Apache-2.0 OR MIT.

//! Binary codec for the file-backed store.
//!
//! Encodes [`Record`] aggregates and WAL/snapshot entries to a
//! length-prefixed, CRC32-checked binary stream. The format is
//! deliberately custom so the crate stays free of an external
//! serialization dependency on the default build (`serde_json`,
//! `bincode`, and friends are not pulled).
//!
//! ## Goals
//!
//! - **Self-describing**: every entry is length-prefixed so a corrupt
//!   tail can be detected and discarded without confusing the rest of
//!   the file.
//! - **Cross-platform**: every multi-byte value is written
//!   little-endian regardless of host byte order, so a database
//!   written on x86_64 reads back identically on aarch64.
//! - **Versioned**: every file starts with a 4-byte magic + 4-byte
//!   format version. Future format changes bump the version and either
//!   add a migration path or reject old versions explicitly.
//! - **Allocation-free on the write path**: encoders take a borrowed
//!   `&mut Vec<u8>` that the caller pools across operations.
//!
//! ## Frame format
//!
//! The on-disk layout for a single WAL/snap entry is:
//!
//! ```text
//! +-----------------+----------------+----------------+
//! | payload_len: u32 LE              | body bytes...  |
//! +-----------------+----------------+----------------+
//! | crc32 over body : u32 LE                          |
//! +---------------------------------------------------+
//! ```
//!
//! `payload_len` excludes its own four bytes and the trailing CRC.
//! Readers validate `payload_len` against the remaining file size and
//! the CRC against the body before deserialising — both guards
//! surface as [`Error::Corrupt`] with a static reason string.
//!
//! ## Body format
//!
//! ```text
//! op_kind: u8  (0 = upsert, 1 = delete)
//! id:      u64 LE
//!
//! upsert body: dim u32 LE | f32 × dim (each LE) | has_payload u8 | payload?
//! delete body: (no extra bytes)
//! ```
//!
//! See [`PayloadValue`] for the tagged-union encoding of payload
//! values.

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::payload::{Payload, PayloadValue};
use crate::record::{Record, RecordId};
use crate::vector::Vector;

/// Magic bytes at the head of every iqdb data file.
///
/// `IQDB` in ASCII. The same magic is shared by the snapshot file
/// and the WAL — readers distinguish by filename, not by content.
pub(crate) const MAGIC: [u8; 4] = *b"IQDB";

/// Format version. Bumped on any incompatible on-disk format change.
///
/// `1` covers the v0.4.0 encoding documented above. Unknown versions
/// are rejected at open time with `Error::Corrupt { reason: "unknown
/// format version" }` rather than silently degrading.
pub(crate) const FORMAT_VERSION: u32 = 1;

/// Op-kind tag for upsert frames.
pub(crate) const OP_UPSERT: u8 = 0;

/// Op-kind tag for delete frames.
pub(crate) const OP_DELETE: u8 = 1;

/// Op-kind decoded from a single WAL / snap entry.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Op {
    Upsert(Record),
    Delete(RecordId),
}

/// Encode the magic + version header into a buffer.
///
/// Always 8 bytes: `b"IQDB" || FORMAT_VERSION as u32 LE`.
pub(crate) fn write_header(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&MAGIC);
    buf.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
}

/// Decode the magic + version header from `bytes`, returning the
/// number of bytes consumed.
///
/// # Errors
///
/// Returns [`Error::Corrupt`] when the magic or version does not
/// match expectations.
pub(crate) fn read_header(bytes: &[u8]) -> Result<usize> {
    if bytes.len() < 8 {
        return Err(Error::corrupt("truncated header"));
    }
    if bytes[..4] != MAGIC {
        return Err(Error::corrupt("bad magic"));
    }
    let mut version_bytes = [0u8; 4];
    version_bytes.copy_from_slice(&bytes[4..8]);
    let version = u32::from_le_bytes(version_bytes);
    if version != FORMAT_VERSION {
        return Err(Error::corrupt("unknown format version"));
    }
    Ok(8)
}

/// Encode an `Op` into `buf`, framed with a length prefix and CRC32 tail.
///
/// The frame layout is the one documented at the module level — four
/// length bytes, the body, four CRC bytes. The caller is responsible
/// for pooling `buf` across operations to keep the WAL append path
/// allocation-free in steady state.
pub(crate) fn write_frame(buf: &mut Vec<u8>, op: &Op) {
    // Reserve length placeholder; we patch it once the body length
    // is known.
    let len_pos = buf.len();
    buf.extend_from_slice(&[0u8; 4]);
    let body_start = buf.len();

    match op {
        Op::Upsert(record) => {
            buf.push(OP_UPSERT);
            write_u64(buf, record.id().get());
            write_vector(buf, record.vector());
            match record.payload() {
                Some(payload) => {
                    buf.push(1u8);
                    write_payload(buf, payload);
                }
                None => buf.push(0u8),
            }
        }
        Op::Delete(id) => {
            buf.push(OP_DELETE);
            write_u64(buf, id.get());
        }
    }

    let body_len = (buf.len() - body_start) as u32;
    buf[len_pos..len_pos + 4].copy_from_slice(&body_len.to_le_bytes());

    let crc = crc32(&buf[body_start..body_start + body_len as usize]);
    buf.extend_from_slice(&crc.to_le_bytes());
}

/// Read a single framed `Op` from `bytes`, returning the op and the
/// number of bytes consumed.
///
/// Returns `Ok(None)` when `bytes` is empty (clean end-of-file).
///
/// # Errors
///
/// Returns [`Error::Corrupt`] when the frame is truncated, the CRC
/// does not match, the op-kind tag is unrecognised, or any inner
/// field fails its own integrity check.
pub(crate) fn read_frame(bytes: &[u8]) -> Result<Option<(Op, usize)>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() < 4 {
        return Err(Error::corrupt("truncated frame length"));
    }
    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(&bytes[..4]);
    let body_len = u32::from_le_bytes(len_bytes) as usize;
    if bytes.len() < 4 + body_len + 4 {
        return Err(Error::corrupt("truncated frame body"));
    }
    let body = &bytes[4..4 + body_len];
    let mut crc_bytes = [0u8; 4];
    crc_bytes.copy_from_slice(&bytes[4 + body_len..4 + body_len + 4]);
    let expected_crc = u32::from_le_bytes(crc_bytes);
    if crc32(body) != expected_crc {
        return Err(Error::corrupt("frame crc mismatch"));
    }

    let op = decode_body(body)?;
    Ok(Some((op, 4 + body_len + 4)))
}

/// Decode a single op body (no framing).
fn decode_body(body: &[u8]) -> Result<Op> {
    let mut cursor = Cursor::new(body);
    let kind = cursor.read_u8()?;
    let id = RecordId::new(cursor.read_u64()?);
    match kind {
        OP_UPSERT => {
            let vector = read_vector(&mut cursor)?;
            let has_payload = cursor.read_u8()?;
            let payload = match has_payload {
                0 => None,
                1 => Some(read_payload(&mut cursor)?),
                _ => return Err(Error::corrupt("bad payload tag")),
            };
            if !cursor.is_at_end() {
                return Err(Error::corrupt("trailing bytes in upsert body"));
            }
            match payload {
                Some(p) => Ok(Op::Upsert(Record::with_payload(id, vector, p))),
                None => Ok(Op::Upsert(Record::new(id, vector))),
            }
        }
        OP_DELETE => {
            if !cursor.is_at_end() {
                return Err(Error::corrupt("trailing bytes in delete body"));
            }
            Ok(Op::Delete(id))
        }
        _ => Err(Error::corrupt("unknown op kind")),
    }
}

fn write_u64(buf: &mut Vec<u8>, value: u64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn write_i64(buf: &mut Vec<u8>, value: i64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn write_f64(buf: &mut Vec<u8>, value: f64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn write_bytes(buf: &mut Vec<u8>, bytes: &[u8]) {
    let len = bytes.len() as u32;
    write_u32(buf, len);
    buf.extend_from_slice(bytes);
}

fn write_vector(buf: &mut Vec<u8>, vector: &Vector) {
    let slice = vector.as_slice();
    write_u32(buf, slice.len() as u32);
    for component in slice {
        buf.extend_from_slice(&component.to_le_bytes());
    }
}

fn write_payload(buf: &mut Vec<u8>, payload: &Payload) {
    let map = payload.as_map();
    write_u32(buf, map.len() as u32);
    for (key, value) in map {
        write_bytes(buf, key.as_bytes());
        write_payload_value(buf, value);
    }
}

const PV_NULL: u8 = 0;
const PV_BOOL: u8 = 1;
const PV_INT: u8 = 2;
const PV_FLOAT: u8 = 3;
const PV_TEXT: u8 = 4;
const PV_BYTES: u8 = 5;
const PV_ARRAY: u8 = 6;
const PV_OBJECT: u8 = 7;

fn write_payload_value(buf: &mut Vec<u8>, value: &PayloadValue) {
    match value {
        PayloadValue::Null => buf.push(PV_NULL),
        PayloadValue::Bool(b) => {
            buf.push(PV_BOOL);
            buf.push(u8::from(*b));
        }
        PayloadValue::Int(n) => {
            buf.push(PV_INT);
            write_i64(buf, *n);
        }
        PayloadValue::Float(f) => {
            buf.push(PV_FLOAT);
            write_f64(buf, *f);
        }
        PayloadValue::Text(s) => {
            buf.push(PV_TEXT);
            write_bytes(buf, s.as_bytes());
        }
        PayloadValue::Bytes(b) => {
            buf.push(PV_BYTES);
            write_bytes(buf, b);
        }
        PayloadValue::Array(items) => {
            buf.push(PV_ARRAY);
            write_u32(buf, items.len() as u32);
            for item in items {
                write_payload_value(buf, item);
            }
        }
        PayloadValue::Object(map) => {
            buf.push(PV_OBJECT);
            write_u32(buf, map.len() as u32);
            for (key, value) in map {
                write_bytes(buf, key.as_bytes());
                write_payload_value(buf, value);
            }
        }
    }
}

fn read_vector(cursor: &mut Cursor<'_>) -> Result<Vector> {
    let dim = cursor.read_u32()? as usize;
    if dim == 0 {
        return Err(Error::corrupt("zero-dim vector in store"));
    }
    let mut data = Vec::with_capacity(dim);
    for _ in 0..dim {
        let mut bytes = [0u8; 4];
        cursor.read_exact(&mut bytes)?;
        data.push(f32::from_le_bytes(bytes));
    }
    // Vector::new re-validates finiteness. A store containing a
    // non-finite vector is itself corrupt — the v0.2.0 boundary
    // guarantees rejection at upsert time, so this can only happen
    // if the file was hand-edited or corrupted in transit.
    Vector::new(data).map_err(|_| Error::corrupt("non-finite vector in store"))
}

fn read_payload(cursor: &mut Cursor<'_>) -> Result<Payload> {
    let field_count = cursor.read_u32()? as usize;
    let mut payload = Payload::new();
    for _ in 0..field_count {
        let key = read_string(cursor)?;
        let value = read_payload_value(cursor)?;
        let _previous = payload.insert(key, value);
    }
    Ok(payload)
}

fn read_payload_value(cursor: &mut Cursor<'_>) -> Result<PayloadValue> {
    let tag = cursor.read_u8()?;
    match tag {
        PV_NULL => Ok(PayloadValue::Null),
        PV_BOOL => Ok(PayloadValue::Bool(cursor.read_u8()? != 0)),
        PV_INT => Ok(PayloadValue::Int(cursor.read_i64()?)),
        PV_FLOAT => Ok(PayloadValue::Float(cursor.read_f64()?)),
        PV_TEXT => Ok(PayloadValue::Text(read_string(cursor)?)),
        PV_BYTES => {
            let len = cursor.read_u32()? as usize;
            let mut bytes = vec![0u8; len];
            cursor.read_exact(&mut bytes)?;
            Ok(PayloadValue::Bytes(bytes))
        }
        PV_ARRAY => {
            let len = cursor.read_u32()? as usize;
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(read_payload_value(cursor)?);
            }
            Ok(PayloadValue::Array(items))
        }
        PV_OBJECT => {
            let len = cursor.read_u32()? as usize;
            let mut map = BTreeMap::new();
            for _ in 0..len {
                let key = read_string(cursor)?;
                let value = read_payload_value(cursor)?;
                let _previous = map.insert(key, value);
            }
            Ok(PayloadValue::Object(map))
        }
        _ => Err(Error::corrupt("unknown payload tag")),
    }
}

fn read_string(cursor: &mut Cursor<'_>) -> Result<String> {
    let len = cursor.read_u32()? as usize;
    let mut bytes = vec![0u8; len];
    cursor.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| Error::corrupt("non-utf8 payload key"))
}

/// Forward-only borrowed cursor over a body buffer.
///
/// Wraps `&[u8]` with the bounds-checked reads the codec needs. Every
/// `read_*` method returns [`Error::Corrupt`] on a short read rather
/// than panicking — this is the on-disk boundary, and untrusted input
/// must not be allowed to violate Rust invariants.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn is_at_end(&self) -> bool {
        self.pos == self.bytes.len()
    }

    fn read_u8(&mut self) -> Result<u8> {
        if self.pos + 1 > self.bytes.len() {
            return Err(Error::corrupt("truncated u8"));
        }
        let v = self.bytes[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn read_u32(&mut self) -> Result<u32> {
        if self.pos + 4 > self.bytes.len() {
            return Err(Error::corrupt("truncated u32"));
        }
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&self.bytes[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_u64(&mut self) -> Result<u64> {
        if self.pos + 8 > self.bytes.len() {
            return Err(Error::corrupt("truncated u64"));
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.bytes[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(u64::from_le_bytes(buf))
    }

    fn read_i64(&mut self) -> Result<i64> {
        if self.pos + 8 > self.bytes.len() {
            return Err(Error::corrupt("truncated i64"));
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.bytes[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(i64::from_le_bytes(buf))
    }

    fn read_f64(&mut self) -> Result<f64> {
        if self.pos + 8 > self.bytes.len() {
            return Err(Error::corrupt("truncated f64"));
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.bytes[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(f64::from_le_bytes(buf))
    }

    fn read_exact(&mut self, out: &mut [u8]) -> Result<()> {
        if self.pos + out.len() > self.bytes.len() {
            return Err(Error::corrupt("truncated bytes"));
        }
        out.copy_from_slice(&self.bytes[self.pos..self.pos + out.len()]);
        self.pos += out.len();
        Ok(())
    }
}

/// CRC-32/IEEE-802.3 ("Ethernet" CRC) computed via the standard
/// polynomial `0xEDB88320` over the supplied bytes.
///
/// Used to validate every framed entry. Implemented as a small,
/// table-free reflected-bit-by-bit routine — the file-backed store
/// writes one CRC per upsert/delete, so the throughput cost is
/// dwarfed by the surrounding `write_all` and `fsync`. A table-based
/// implementation can land later if profiling shows the CRC dominating.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upsert_record(id: u64, components: Vec<f32>, payload: Option<Payload>) -> Record {
        let v = Vector::new(components).expect("finite");
        match payload {
            None => Record::new(RecordId::new(id), v),
            Some(p) => Record::with_payload(RecordId::new(id), v, p),
        }
    }

    #[test]
    fn header_round_trip() {
        let mut buf = Vec::new();
        write_header(&mut buf);
        let consumed = read_header(&buf).unwrap();
        assert_eq!(consumed, buf.len());
    }

    #[test]
    fn header_rejects_bad_magic() {
        let mut buf = vec![b'X', b'X', b'X', b'X'];
        buf.extend_from_slice(&1u32.to_le_bytes());
        let err = read_header(&buf).unwrap_err();
        assert!(matches!(err, Error::Corrupt { .. }));
    }

    #[test]
    fn header_rejects_unknown_version() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&MAGIC);
        buf.extend_from_slice(&999u32.to_le_bytes());
        let err = read_header(&buf).unwrap_err();
        assert!(matches!(err, Error::Corrupt { .. }));
    }

    #[test]
    fn upsert_frame_round_trip_no_payload() {
        let record = upsert_record(1, vec![0.1, 0.2, 0.3], None);
        let mut buf = Vec::new();
        write_frame(&mut buf, &Op::Upsert(record.clone()));
        let (op, consumed) = read_frame(&buf).unwrap().unwrap();
        assert_eq!(consumed, buf.len());
        match op {
            Op::Upsert(decoded) => {
                assert_eq!(decoded.id(), record.id());
                assert_eq!(decoded.vector().as_slice(), record.vector().as_slice());
                assert!(decoded.payload().is_none());
            }
            _ => panic!("expected upsert"),
        }
    }

    #[test]
    fn upsert_frame_round_trip_with_payload() {
        let mut p = Payload::new();
        let _ = p.insert("kind", "doc");
        let _ = p.insert("year", 2026_i64);
        let _ = p.insert("score", 0.97_f64);
        let _ = p.insert("verified", true);
        let _ = p.insert("blob", PayloadValue::Bytes(vec![1, 2, 3, 4]));
        let _ = p.insert(
            "tags",
            PayloadValue::Array(vec![PayloadValue::from("rust"), PayloadValue::from("db")]),
        );

        let record = upsert_record(7, vec![0.5, -0.25, 0.75], Some(p));
        let mut buf = Vec::new();
        write_frame(&mut buf, &Op::Upsert(record.clone()));
        let (op, consumed) = read_frame(&buf).unwrap().unwrap();
        assert_eq!(consumed, buf.len());
        match op {
            Op::Upsert(decoded) => {
                assert_eq!(decoded.id(), record.id());
                assert_eq!(decoded.vector().as_slice(), record.vector().as_slice());
                assert_eq!(decoded.payload(), record.payload());
            }
            _ => panic!("expected upsert"),
        }
    }

    #[test]
    fn delete_frame_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &Op::Delete(RecordId::new(42)));
        let (op, _) = read_frame(&buf).unwrap().unwrap();
        assert!(matches!(op, Op::Delete(id) if id.get() == 42));
    }

    #[test]
    fn multiple_frames_concatenated_decode_in_order() {
        let mut buf = Vec::new();
        write_frame(
            &mut buf,
            &Op::Upsert(upsert_record(1, vec![1.0, 0.0], None)),
        );
        write_frame(&mut buf, &Op::Delete(RecordId::new(2)));
        write_frame(
            &mut buf,
            &Op::Upsert(upsert_record(3, vec![0.0, 1.0], None)),
        );

        let mut offset = 0;
        let mut ops = Vec::new();
        while let Some((op, consumed)) = read_frame(&buf[offset..]).unwrap() {
            offset += consumed;
            ops.push(op);
        }
        assert_eq!(ops.len(), 3);
        assert!(matches!(ops[0], Op::Upsert(_)));
        assert!(matches!(ops[1], Op::Delete(_)));
        assert!(matches!(ops[2], Op::Upsert(_)));
    }

    #[test]
    fn empty_input_yields_none() {
        let out = read_frame(&[]).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn truncated_frame_returns_corrupt() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &Op::Delete(RecordId::new(1)));
        // Drop the last few bytes (CRC).
        buf.truncate(buf.len() - 2);
        let err = read_frame(&buf).unwrap_err();
        assert!(matches!(err, Error::Corrupt { .. }));
    }

    #[test]
    fn corrupted_body_fails_crc() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &Op::Delete(RecordId::new(1)));
        // Flip a body byte to break the CRC.
        buf[5] ^= 0xFF;
        let err = read_frame(&buf).unwrap_err();
        assert!(matches!(err, Error::Corrupt { reason } if reason.contains("crc")));
    }

    #[test]
    fn unknown_op_kind_is_rejected() {
        // Hand-craft a frame with body length 9 and op_kind = 255.
        let mut body = vec![255u8];
        body.extend_from_slice(&0u64.to_le_bytes());
        let mut buf = Vec::new();
        buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
        buf.extend_from_slice(&body);
        let crc = crc32(&body);
        buf.extend_from_slice(&crc.to_le_bytes());

        let err = read_frame(&buf).unwrap_err();
        assert!(matches!(err, Error::Corrupt { reason } if reason.contains("op")));
    }

    #[test]
    fn crc32_matches_known_vector() {
        // CRC-32/IEEE-802.3 of the empty string is 0.
        assert_eq!(crc32(&[]), 0);
        // CRC of ASCII "123456789" is the well-known 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn payload_nested_object_round_trip() {
        let mut inner = BTreeMap::new();
        let _ = inner.insert("a".to_string(), PayloadValue::Int(1));
        let _ = inner.insert("b".to_string(), PayloadValue::Text("two".to_string()));

        let mut p = Payload::new();
        let _ = p.insert("outer", PayloadValue::Object(inner.clone()));

        let record = upsert_record(1, vec![1.0], Some(p.clone()));
        let mut buf = Vec::new();
        write_frame(&mut buf, &Op::Upsert(record));
        let (op, _) = read_frame(&buf).unwrap().unwrap();
        match op {
            Op::Upsert(decoded) => {
                assert_eq!(decoded.payload(), Some(&p));
            }
            _ => panic!("expected upsert"),
        }
    }
}
