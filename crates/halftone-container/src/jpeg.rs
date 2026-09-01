//! JPEG structure forensics: quantization tables, chroma subsampling, encoder class.
//!
//! Why it works: every JPEG writer emits DQT segments and a Start-Of-Frame that fix
//! the quantization tables and the chroma subsampling. Cameras use firmware-specific,
//! non-standard tables; the libjpeg/PIL/OpenCV paths that most editors, web pipelines
//! and image generators encode through use Annex-K *scaled standard* tables. A file
//! whose tables are a plain libjpeg re-encode is not a camera-native capture — a fact
//! that is true of edited photos, screenshots, web downloads and generator exports
//! alike, so on its own it is an encoder-path signal, never a claim of authorship.

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
use serde::Serialize;

/// Parsed DQT segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantSet {
    /// Tables in file order, each 64 entries in zigzag order. 8-bit only for now.
    pub tables: Vec<[u16; 64]>,
}

/// One frame component from the SOF segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Component {
    /// Component identifier (1 = Y, 2 = Cb, 3 = Cr, by convention).
    pub id: u8,
    /// Horizontal sampling factor.
    pub h: u8,
    /// Vertical sampling factor.
    pub v: u8,
    /// Quantization-table selector for this component.
    pub tq: u8,
}

/// Structural summary of a JPEG, parsed from the header segments only (up to SOS).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JpegStructure {
    /// Quantization tables in file order.
    pub tables: Vec<[u16; 64]>,
    /// Frame components (from SOF).
    pub components: Vec<Component>,
    /// Sample precision in bits (usually 8).
    pub precision: u8,
    /// Image width in pixels.
    pub width: u16,
    /// Image height in pixels.
    pub height: u16,
    /// Progressive (SOF2) rather than baseline.
    pub progressive: bool,
    /// APP0 JFIF marker present.
    pub has_jfif: bool,
    /// APP1 Exif marker present.
    pub has_exif: bool,
    /// APP1 XMP packet present.
    pub has_xmp: bool,
    /// APP14 Adobe marker present (Photoshop and other Adobe encoders).
    pub has_adobe: bool,
    /// Adobe colour transform code, if the Adobe marker was present.
    pub adobe_transform: Option<u8>,
    /// Bytes after the final EOI marker (a mild tampering/appended-data signal).
    pub trailing_bytes: usize,
    /// The XMP packet, bounded, if present.
    pub xmp: Option<String>,
    /// The JPEG comment (COM) segment, bounded, if present.
    pub comment: Option<String>,
    /// Marker byte sequence, for order fingerprinting.
    pub markers: Vec<u8>,
}

impl JpegStructure {
    /// Human-readable chroma subsampling, e.g. `4:2:0`. `grayscale` for one component.
    pub fn subsampling(&self) -> &'static str {
        match self.components.as_slice() {
            [_] => "grayscale",
            [y, ..] => match (y.h, y.v) {
                (1, 1) => "4:4:4",
                (2, 1) => "4:2:2",
                (1, 2) => "4:4:0",
                (2, 2) => "4:2:0",
                _ => "other",
            },
            [] => "unknown",
        }
    }
}

/// Encoder class inferred from structure. Deliberately coarse and honest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum EncoderClass {
    /// Annex-K scaled standard tables: the libjpeg / PIL / OpenCV re-encode family.
    LibjpegStandard {
        /// Recovered libjpeg quality (1..=100).
        quality: u8,
    },
    /// Adobe APP14 present: saved or exported by an Adobe application.
    Adobe,
    /// Non-standard tables: consistent with a camera or proprietary encoder.
    NonStandard,
}

/// Parse all DQT segments from JPEG bytes. Stops at SOS. Never panics on garbage.
pub fn parse_dqt(b: &[u8]) -> Result<QuantSet, String> {
    let s = parse_structure(b)?;
    Ok(QuantSet { tables: s.tables })
}

/// Parse header structure (DQT, SOF, APPn, COM) up to the first SOS. Bounds-checked.
pub fn parse_structure(b: &[u8]) -> Result<JpegStructure, String> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return Err("not a JPEG".into());
    }
    let mut s = JpegStructure::default();
    let mut i = 2;
    while i + 1 < b.len() {
        if b[i] != 0xFF {
            return Err(format!("marker expected at {i}"));
        }
        // Skip any fill 0xFF bytes.
        let mut m = i + 1;
        while m < b.len() && b[m] == 0xFF {
            m += 1;
        }
        if m >= b.len() {
            break;
        }
        let marker = b[m];
        i = m + 1;
        s.markers.push(marker);
        match marker {
            0xD9 => break,                          // EOI
            0xDA => break,                          // SOS: entropy data follows
            0x01 | 0xD0..=0xD7 => continue,         // standalone markers, no payload
            _ => {}
        }
        if i + 2 > b.len() {
            return Err("truncated segment header".into());
        }
        let len = u16::from_be_bytes([b[i], b[i + 1]]) as usize;
        if len < 2 || i + len > b.len() {
            return Err("bad segment length".into());
        }
        let seg = &b[i + 2..i + len];
        match marker {
            0xDB => parse_dqt_segment(seg, &mut s.tables)?,
            // SOF0/1 baseline, SOF2 progressive, and the other Huffman SOFs.
            0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD
            | 0xCE | 0xCF => {
                s.progressive = matches!(marker, 0xC2 | 0xC6 | 0xCA | 0xCE);
                parse_sof(seg, &mut s)?;
            }
            0xE0 => s.has_jfif = seg.starts_with(b"JFIF\0"),
            0xE1 => {
                if seg.starts_with(b"Exif\0\0") {
                    s.has_exif = true;
                } else if let Some(rest) = seg.strip_prefix(b"http://ns.adobe.com/xap/1.0/\0") {
                    s.has_xmp = true;
                    s.xmp = Some(clip(&String::from_utf8_lossy(rest), 8192));
                }
            }
            0xEE => {
                if seg.starts_with(b"Adobe") {
                    s.has_adobe = true;
                    s.adobe_transform = seg.get(11).copied();
                }
            }
            0xFE => s.comment = Some(clip(&String::from_utf8_lossy(seg), 1024)),
            _ => {}
        }
        i += len;
    }
    if let Some(pos) = find_last_eoi(b) {
        s.trailing_bytes = b.len().saturating_sub(pos + 2);
    }
    if s.tables.is_empty() {
        return Err("no DQT found".into());
    }
    Ok(s)
}

fn parse_dqt_segment(seg: &[u8], out: &mut Vec<[u16; 64]>) -> Result<(), String> {
    let mut p = 0;
    while p < seg.len() {
        let pq = seg[p] >> 4;
        let width = if pq == 0 { 1 } else { 2 };
        p += 1;
        if p + 64 * width > seg.len() {
            return Err("truncated DQT".into());
        }
        let mut t = [0u16; 64];
        for (k, slot) in t.iter_mut().enumerate() {
            *slot = if width == 1 {
                seg[p + k] as u16
            } else {
                u16::from_be_bytes([seg[p + 2 * k], seg[p + 2 * k + 1]])
            };
        }
        out.push(t);
        p += 64 * width;
    }
    Ok(())
}

fn parse_sof(seg: &[u8], s: &mut JpegStructure) -> Result<(), String> {
    if seg.len() < 6 {
        return Err("truncated SOF".into());
    }
    s.precision = seg[0];
    s.height = u16::from_be_bytes([seg[1], seg[2]]);
    s.width = u16::from_be_bytes([seg[3], seg[4]]);
    let ncomp = seg[5] as usize;
    if seg.len() < 6 + ncomp * 3 {
        return Err("truncated SOF components".into());
    }
    s.components = (0..ncomp)
        .map(|c| {
            let o = 6 + c * 3;
            Component { id: seg[o], h: seg[o + 1] >> 4, v: seg[o + 1] & 0x0F, tq: seg[o + 2] }
        })
        .collect();
    Ok(())
}

fn find_last_eoi(b: &[u8]) -> Option<usize> {
    (0..b.len().saturating_sub(1))
        .rev()
        .find(|&i| b[i] == 0xFF && b[i + 1] == 0xD9)
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
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

/// Classify the encoder from parsed structure.
pub fn classify_encoder(s: &JpegStructure) -> EncoderClass {
    if s.has_adobe {
        return EncoderClass::Adobe;
    }
    match s.tables.first().and_then(estimate_libjpeg_quality) {
        Some(quality) => EncoderClass::LibjpegStandard { quality },
        None => EncoderClass::NonStandard,
    }
}

/// Quant-table / structure evidence source.
///
/// Signal: the file is a libjpeg-family (or Adobe) software re-encode rather than a
/// camera-native JPEG. `Present` = software re-encode recognised; `Absent` = tables
/// look camera-native/proprietary; `Inconclusive` on unparseable input. This is an
/// encoder-path fact and is never, on its own, evidence of AI generation.
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
        let s = parse_structure(&a.bytes).map_err(halftone_core::Error::Parse)?;
        let class = classify_encoder(&s);
        let sub = s.subsampling();
        let (status, rationale) = match &class {
            EncoderClass::LibjpegStandard { quality } => (
                Status::Present,
                format!(
                    "Standard libjpeg tables at quality {quality} with {sub} subsampling: a \
                     software (re-)encode from the libjpeg/PIL/OpenCV family, not a camera-native \
                     JPEG. Common to edited photos, screenshots, web downloads and generator \
                     exports alike; not evidence of origin on its own."
                ),
            ),
            EncoderClass::Adobe => (
                Status::Present,
                format!(
                    "Adobe APP14 marker with {sub} subsampling: saved or exported by an Adobe \
                     application. Indicates editing/re-encoding, not origin."
                ),
            ),
            EncoderClass::NonStandard => (
                Status::Absent,
                format!(
                    "Non-standard quantization tables with {sub} subsampling: consistent with a \
                     camera or proprietary encoder rather than a libjpeg-family re-encode."
                ),
            ),
        };
        Ok(Evidence {
            layer: self.layer(),
            source: self.id(),
            status,
            statistic: None,
            calibration: None,
            rationale,
            details: serde_json::json!({
                "encoder": class,
                "subsampling": sub,
                "tables": s.tables.len(),
                "progressive": s.progressive,
                "width": s.width,
                "height": s.height,
                "has_jfif": s.has_jfif,
                "has_exif": s.has_exif,
                "has_xmp": s.has_xmp,
                "has_adobe": s.has_adobe,
                "trailing_bytes": s.trailing_bytes,
            }),
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal JPEG: SOI + one 8-bit DQT + a baseline SOF0 (3 components, 4:2:0) + SOS + EOI.
    fn synth_jpeg(table: [u8; 64]) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        // DQT
        v.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
        v.extend_from_slice(&table);
        // SOF0: len=17, prec=8, h=0x0010, w=0x0010, 3 comps (Y 2x2 tq0, Cb 1x1 tq1, Cr 1x1 tq1)
        v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x10, 0x00, 0x10, 0x03]);
        v.extend_from_slice(&[0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        // SOS + EOI
        v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
        v
    }

    fn std_luma_q(q: u8) -> [u8; 64] {
        const STD: [u16; 64] = [
            16, 11, 12, 14, 12, 10, 16, 14, 13, 14, 18, 17, 16, 19, 24, 40, 26, 24, 22, 22, 24,
            49, 35, 37, 29, 40, 58, 51, 61, 60, 57, 51, 56, 55, 64, 72, 92, 78, 64, 68, 87, 69,
            55, 56, 80, 109, 81, 87, 95, 98, 103, 104, 103, 62, 77, 113, 121, 112, 100, 120, 92,
            101, 103, 99,
        ];
        let scale = if q < 50 { 5000 / q as u32 } else { 200 - 2 * q as u32 };
        let mut out = [0u8; 64];
        for (o, &s) in out.iter_mut().zip(STD.iter()) {
            *o = ((s as u32 * scale + 50) / 100).clamp(1, 255) as u8;
        }
        out
    }

    #[test]
    fn parses_structure_and_subsampling() {
        let s = parse_structure(&synth_jpeg([1; 64])).unwrap();
        assert_eq!(s.tables.len(), 1);
        assert_eq!(s.components.len(), 3);
        assert_eq!(s.subsampling(), "4:2:0");
        assert_eq!(s.width, 16);
        assert_eq!(s.height, 16);
        assert!(!s.progressive);
    }

    #[test]
    fn classifies_libjpeg_quality() {
        let s = parse_structure(&synth_jpeg(std_luma_q(85))).unwrap();
        assert_eq!(classify_encoder(&s), EncoderClass::LibjpegStandard { quality: 85 });
    }

    #[test]
    fn quality_100_is_all_ones() {
        assert_eq!(estimate_libjpeg_quality(&[1; 64]), Some(100));
    }

    #[test]
    fn nonstandard_tables_are_not_libjpeg() {
        // A deliberately irregular table that is not any Annex-K scaling.
        let mut t = [20u8; 64];
        t[0] = 3;
        t[7] = 200;
        let s = parse_structure(&synth_jpeg(t)).unwrap();
        assert_eq!(classify_encoder(&s), EncoderClass::NonStandard);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_structure(b"nope").is_err());
        assert!(parse_structure(&[0xFF, 0xD8, 0xFF, 0xDB, 0xFF, 0xFF]).is_err());
    }
}
