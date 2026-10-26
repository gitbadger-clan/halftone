//! Structured fuzzing of the C2PA 2.4 §15.12.4 procedure (DIFFERENTIAL.md D-014):
//! part tables shaped like real ones (mostly contiguous, with gaps, overlaps,
//! unreadable or unsupported part hashes) over file lengths derived from the table
//! (exact fit, cut at a part, cut inside a part, longer). Part hashes come as byte
//! arrays or as base64 text (what c2pa's detailed JSON writes), with or without
//! exclusion ranges; part starts may carry JPEG or MP4 magic. Every report must
//! pass `multi_asset::check_invariants`; a panic anywhere is a finding.
#![no_main]

use arbitrary::Arbitrary;
use halftone_c2pa::multi_asset::{check_invariants, evaluate, PartHash, PartHasher};
use libfuzzer_sys::fuzz_target;
use serde_json::{json, Map, Value};

#[derive(Arbitrary, Debug)]
struct Part {
    /// Offset relative to where the previous part ended: 0 is contiguous,
    /// negative overlaps, positive leaves a gap.
    gap: i8,
    length: u16,
    optional: Option<bool>,
    /// 0..=5 readable hash (0 = mismatch), 6 unreadable hash text, 7 not in the
    /// store, 8 a box-hash part, 9 a BMFF locator, 10 a hash the hasher rejects,
    /// 11 a remote data hash.
    kind: u8,
    /// Write the hash as base64 text instead of a byte array.
    base64: bool,
    /// None: no exclusions; Some((start, length)); start 0xFFFF: an unreadable range.
    exclusion: Option<(u16, u16)>,
    /// 0 none, 1 JPEG SOI at the part's start, 2 MP4 `ftyp` at offset 4.
    magic: u8,
}

#[derive(Arbitrary, Debug)]
struct Input {
    parts: Vec<Part>,
    cut: u8,
    extra: u16,
    duplicate_multi_asset: bool,
}

/// The part hash itself says the answer: `[1]` match, `[0]` mismatch, anything
/// else an error, as c2pa gives for an algorithm it cannot compute.
struct Oracle;

impl PartHasher for Oracle {
    fn verify(&self, part: &PartHash, _bytes: &[u8]) -> Result<bool, String> {
        match part.hash.as_slice() {
            [1] => Ok(true),
            [0] => Ok(false),
            _ => Err("unsupported".into()),
        }
    }
}

/// Standard base64 with padding, as c2pa's JSON writes byte strings.
fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            s.push(if i <= c.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    s
}

fuzz_target!(|input: Input| {
    let mut at: u64 = 0;
    let mut spans: Vec<(u64, u64, u8)> = Vec::new();
    let mut parts = Vec::new();
    let mut store = Map::new();
    for (i, p) in input.parts.iter().take(8).enumerate() {
        let offset = at.saturating_add_signed(i64::from(p.gap));
        let length = u64::from(p.length);
        at = offset + length;
        spans.push((offset, length, p.magic % 3));
        let data = if i == 0 {
            "c2pa.hash.data.part".to_string()
        } else {
            format!("c2pa.hash.data.part__{i}")
        };
        let kind = p.kind % 12;
        let label = if kind == 8 {
            data.replace("data", "boxes")
        } else {
            data
        };
        if kind != 7 {
            let raw: Vec<u8> = match kind {
                0 => vec![0],
                10 => vec![2],
                _ => vec![1],
            };
            let mut a = Map::new();
            a.insert(
                "hash".into(),
                match (kind, p.base64) {
                    (6, _) => json!("%%"),
                    (_, true) => json!(b64(&raw)),
                    (_, false) => json!(raw),
                },
            );
            match p.exclusion {
                Some((0xFFFF, _)) => {
                    a.insert("exclusions".into(), json!([{"start": "x"}]));
                }
                Some((s, l)) => {
                    a.insert("exclusions".into(), json!([{"start": s, "length": l}]));
                }
                None => {}
            }
            if kind == 11 {
                a.insert("url".into(), json!("https://example.com/part"));
            }
            store.insert(label.clone(), Value::Object(a));
        }
        let location = if kind == 9 {
            json!({"bmffBox": "/mpvd"})
        } else {
            json!({"byteOffset": offset, "length": length})
        };
        let mut part = json!({"location": location,
            "hashAssertion": {"url": format!("self#jumbf=c2pa.assertions/{label}")}});
        if let Some(o) = p.optional {
            part["optional"] = json!(o);
        }
        parts.push(part);
    }
    if input.duplicate_multi_asset {
        store.insert("c2pa.hash.multi-asset__1".into(), json!({}));
    }
    let pick = spans
        .get(usize::from(input.cut) % spans.len().max(1))
        .copied();
    let len = match (input.cut % 5, pick) {
        (0 | 1, _) => at,
        (2, Some((o, _, _))) => o,
        (3, Some((o, l, _))) => o + u64::from(input.extra) % (l + 1),
        _ => at + u64::from(input.extra % 64),
    }
    .min(1 << 20);
    let mut file = vec![0u8; len as usize];
    for &(o, _, magic) in &spans {
        let (o, bytes): (usize, &[u8]) = match magic {
            1 => (o as usize, &[0xFF, 0xD8, 0xFF]),
            2 => (o as usize + 4, b"ftyp"),
            _ => continue,
        };
        if let Some(dst) = file.get_mut(o..o + bytes.len()) {
            dst.copy_from_slice(bytes);
        }
    }
    let report = evaluate(
        &json!({ "parts": parts }),
        &Value::Object(store),
        &file,
        &Oracle,
    );
    if let Err(e) = check_invariants(&report, len) {
        panic!("{e}\nfile length {len}\n{report:?}");
    }
});
