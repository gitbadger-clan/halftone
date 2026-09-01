//! JPEG quantization-table fingerprinting.
//!
//! Why it works: every JPEG writer emits DQT segments. Cameras, Photoshop, libjpeg
//! quality presets, and the PIL/OpenCV paths used by generation pipelines all leave
//! characteristic tables. A table set matching a camera firmware is strong evidence
//! the file came from that path; a libjpeg q=95 table on a file claiming to be a
//! straight-from-camera photo is a contradiction worth reporting.

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
use serde::Serialize;

/// Parsed DQT segments.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QuantSet {
    /// Tables in file order, each 64 entries in zigzag order. 8-bit only for now.
    pub tables: Vec<[u16; 64]>,
}

/// Parse all DQT segments from JPEG bytes. Stops at SOS. Never panics on garbage.
pub fn parse_dqt(b: &[u8]) -> Result<QuantSet, String> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return Err("not a JPEG".into());
    }
    let mut i = 2;
    let mut tables = Vec::new();
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return Err(format!("marker expected at {i}"));
        }
        let marker = b[i + 1];
        i += 2;
        match marker {
            0xD8 | 0x01 | 0xD0..=0xD7 => continue, // standalone markers
            0xDA | 0xD9 => break,                  // SOS / EOI
            _ => {}
        }
        let len = u16::from_be_bytes([b[i], b[i + 1]]) as usize;
        if len < 2 || i + len > b.len() {
            return Err("bad segment length".into());
        }
        if marker == 0xDB {
            let mut p = i + 2;
            let end = i + len;
            while p < end {
                let pq = b[p] >> 4;
                let width = if pq == 0 { 1 } else { 2 };
                p += 1;
                if p + 64 * width > end {
                    return Err("truncated DQT".into());
                }
                let mut t = [0u16; 64];
                for (k, slot) in t.iter_mut().enumerate() {
                    *slot = if width == 1 {
                        b[p + k] as u16
                    } else {
                        u16::from_be_bytes([b[p + 2 * k], b[p + 2 * k + 1]])
                    };
                }
                tables.push(t);
                p += 64 * width;
            }
        }
        i += len;
    }
    if tables.is_empty() {
        return Err("no DQT found".into());
    }
    Ok(QuantSet { tables })
}

/// Estimate libjpeg "quality" from a luma table (Annex K scaling). `None` if the
/// table isn't a scaled standard table.
pub fn estimate_libjpeg_quality(luma: &[u16; 64]) -> Option<u8> {
    const STD_LUMA_ZIGZAG: [u16; 64] = [
        16, 11, 12, 14, 12, 10, 16, 14, 13, 14, 18, 17, 16, 19, 24, 40, 26, 24, 22, 22, 24, 49,
        35, 37, 29, 40, 58, 51, 61, 60, 57, 51, 56, 55, 64, 72, 92, 78, 64, 68, 87, 69, 55, 56,
        80, 109, 81, 87, 95, 98, 103, 104, 103, 62, 77, 113, 121, 112, 100, 120, 92, 101, 103,
        99,
    ];
    for q in 1..=100u32 {
        let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
        let ok = luma.iter().zip(STD_LUMA_ZIGZAG.iter()).all(|(&got, &s)| {
            let v = ((s as u32 * scale + 50) / 100).clamp(1, 255) as u16;
            got == v
        });
        if ok {
            return Some(q as u8);
        }
    }
    None
}

/// Quant-table evidence source.
#[derive(Debug, Default)]
pub struct QuantTables;

impl EvidenceSource for QuantTables {
    fn id(&self) -> SourceId {
        SourceId { name: "jpeg_quant".into(), version: env!("CARGO_PKG_VERSION").into() }
    }
    fn layer(&self) -> Layer {
        Layer::Container
    }
    fn supports(&self, a: &Asset) -> bool {
        a.modality == Modality::Image && a.mime == "image/jpeg"
    }
    fn assess(&self, a: &Asset) -> halftone_core::Result<Evidence> {
        let qs = parse_dqt(&a.bytes).map_err(halftone_core::Error::Parse)?;
        let quality = qs.tables.first().and_then(estimate_libjpeg_quality);
        // TODO: match against fingerprint DB (camera / editor / generator export paths).
        let (status, rationale) = match quality {
            Some(q) => (
                Status::Present,
                format!("Standard libjpeg tables at quality {q}; consistent with software re-encoding rather than a camera writer."),
            ),
            None => (
                Status::Inconclusive,
                "Non-standard quantization tables; fingerprint DB lookup not yet implemented.".to_string(),
            ),
        };
        Ok(Evidence {
            layer: self.layer(),
            source: self.id(),
            status,
            statistic: None,
            calibration: None,
            rationale,
            details: serde_json::json!({ "tables": qs.tables.len(), "libjpeg_quality": quality }),
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal JPEG prefix: SOI + one 8-bit DQT (all ones) + SOS.
    fn synth_jpeg(table: [u8; 64]) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x43, 0x00];
        v.extend_from_slice(&table);
        v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
        v
    }

    #[test]
    fn parses_single_table() {
        let qs = parse_dqt(&synth_jpeg([1; 64])).unwrap();
        assert_eq!(qs.tables.len(), 1);
        assert!(qs.tables[0].iter().all(|&x| x == 1));
    }

    #[test]
    fn quality_100_is_all_ones() {
        assert_eq!(estimate_libjpeg_quality(&[1; 64]), Some(100));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_dqt(b"nope").is_err());
        assert!(parse_dqt(&[0xFF, 0xD8, 0xFF, 0xDB, 0xFF, 0xFF]).is_err());
    }
}
