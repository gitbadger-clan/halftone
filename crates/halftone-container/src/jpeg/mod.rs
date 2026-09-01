//! JPEG structure forensics: quantization tables, Huffman tables, chroma subsampling,
//! marker inventory, encoder class, and a writer fingerprint.
//!
//! Why it works: every JPEG writer emits DQT, DHT and SOF segments that fix the
//! quantization tables, the entropy tables and the chroma subsampling. Cameras use
//! firmware-specific tables and usually optimised Huffman codes; the libjpeg/PIL/OpenCV
//! paths that most editors, web pipelines and image generators encode through use
//! Annex-K *scaled standard* quantization tables and (by default) the Annex-K standard
//! Huffman tables. A file whose tables are a plain libjpeg re-encode is not a
//! camera-native capture — a fact that is equally true of edited photos, screenshots,
//! web downloads and generator exports, so on its own it is an encoder-path signal,
//! never a claim of authorship. Kee, Johnson & Farid, *Digital image forensics of JPEG
//! images* (2011) is the reference for treating DQT + DHT + metadata as a signature.

pub mod coeffs;
pub mod tables;

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::fingerprints::{FingerprintDb, WriterClass};
use tables::HuffSpec;

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

/// A Huffman table from a DHT segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HuffTable {
    /// 0 = DC, 1 = AC.
    pub class: u8,
    /// Table destination id (0..=3).
    pub id: u8,
    /// The table.
    pub spec: HuffSpec,
}

/// Structural summary of a JPEG, parsed from the header segments only (up to SOS).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JpegStructure {
    /// Quantization tables in file order.
    pub tables: Vec<[u16; 64]>,
    /// Huffman tables in file order.
    pub huffman: Vec<HuffTable>,
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
    /// Arithmetic-coded (SOF9+) rather than Huffman.
    pub arithmetic: bool,
    /// Restart interval (DRI), 0 if none.
    pub restart_interval: u16,
    /// APP0 JFIF marker present.
    pub has_jfif: bool,
    /// APP1 Exif marker present.
    pub has_exif: bool,
    /// APP1 XMP packet present.
    pub has_xmp: bool,
    /// APP2 ICC profile present.
    pub has_icc: bool,
    /// APP11 JUMBF box present (where C2PA manifests live).
    pub has_jumbf: bool,
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
    /// Marker byte sequence up to SOS, for order fingerprinting.
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

    /// Component-id convention: JFIF writers use 1,2,3; Adobe writes 'R','G','B' for RGB;
    /// some encoders use 0,1,2. Part of the writer signature.
    pub fn component_id_style(&self) -> &'static str {
        let ids: Vec<u8> = self.components.iter().map(|c| c.id).collect();
        match ids.as_slice() {
            [1, 2, 3] | [1] => "jfif",
            [0, 1, 2] | [0] => "zero_based",
            [b'R', b'G', b'B'] => "rgb",
            _ => "other",
        }
    }

    /// Quant table used by the luma component (or the first table).
    pub fn luma_table(&self) -> Option<&[u16; 64]> {
        let tq = self.components.first().map(|c| c.tq as usize).unwrap_or(0);
        self.tables.get(tq).or_else(|| self.tables.first())
    }

    /// Quant table used by the first chroma component, if any.
    pub fn chroma_table(&self) -> Option<&[u16; 64]> {
        let tq = self.components.get(1)?.tq as usize;
        self.tables.get(tq)
    }

    /// Whether every DHT table equals its Annex-K standard counterpart.
    pub fn huffman_class(&self) -> HuffmanClass {
        if self.arithmetic {
            return HuffmanClass::Arithmetic;
        }
        if self.huffman.is_empty() {
            return HuffmanClass::Missing;
        }
        let all_std = self.huffman.iter().all(|t| match (t.class, t.id) {
            (0, 0) => t.spec == tables::std_dc_luma(),
            (0, 1) => t.spec == tables::std_dc_chroma(),
            (1, 0) => t.spec == tables::std_ac_luma(),
            (1, 1) => t.spec == tables::std_ac_chroma(),
            _ => false,
        });
        if all_std {
            HuffmanClass::Standard
        } else {
            HuffmanClass::Optimized
        }
    }

    /// Stable fingerprint of the writer-determined structure: all quant tables, the
    /// Huffman specs, sampling factors, component ids, and the pre-SOS marker order.
    /// Two files from the same writer at the same settings share it.
    pub fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        for t in &self.tables {
            for v in t {
                h.update(v.to_be_bytes());
            }
            h.update([0xFF]);
        }
        for t in &self.huffman {
            h.update([t.class, t.id]);
            h.update(t.spec.bits);
            h.update(&t.spec.values);
            h.update([0xFE]);
        }
        for c in &self.components {
            h.update([c.id, c.h, c.v, c.tq]);
        }
        h.update([
            u8::from(self.progressive),
            u8::from(self.has_jfif),
            u8::from(self.has_adobe),
        ]);
        h.update(&self.markers);
        hex::encode(h.finalize())
    }
}

/// Entropy-table class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HuffmanClass {
    /// All DHT tables are the Annex-K defaults (libjpeg without `optimize_coding`, PIL,
    /// OpenCV defaults).
    Standard,
    /// Custom/optimised tables (cameras, Photoshop, mozjpeg, libjpeg with optimize).
    Optimized,
    /// Arithmetic coding, no Huffman tables.
    Arithmetic,
    /// No DHT before SOS (malformed, or tables in a later scan).
    Missing,
}

/// Encoder class inferred from quantization structure. Deliberately coarse and honest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum EncoderClass {
    /// Annex-K scaled standard tables: the libjpeg / PIL / OpenCV re-encode family.
    LibjpegStandard {
        /// Recovered libjpeg quality (1..=100) from the luma table.
        quality: u8,
        /// Whether the chroma table is the Annex-K chroma table at the same quality
        /// (false for grayscale-only files is reported as true).
        chroma_matches: bool,
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

/// Parse header structure (DQT, DHT, SOF, DRI, APPn, COM) up to the first SOS.
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
            0xD9 | 0xDA => break,           // EOI, or SOS: entropy data follows
            0x01 | 0xD0..=0xD7 => continue, // standalone markers, no payload
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
            0xC4 => parse_dht_segment(seg, &mut s.huffman)?,
            0xDD => {
                if seg.len() >= 2 {
                    s.restart_interval = u16::from_be_bytes([seg[0], seg[1]]);
                }
            }
            0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE
            | 0xCF => {
                s.progressive = matches!(marker, 0xC2 | 0xC6 | 0xCA | 0xCE);
                s.arithmetic = marker >= 0xC9;
                parse_sof(seg, &mut s)?;
            }
            0xE0 => s.has_jfif |= seg.starts_with(b"JFIF\0"),
            0xE1 => {
                if seg.starts_with(b"Exif\0\0") {
                    s.has_exif = true;
                } else if let Some(rest) = seg.strip_prefix(b"http://ns.adobe.com/xap/1.0/\0") {
                    s.has_xmp = true;
                    s.xmp = Some(clip(&String::from_utf8_lossy(rest), 8192));
                }
            }
            0xE2 => s.has_icc |= seg.starts_with(b"ICC_PROFILE\0"),
            0xEB => s.has_jumbf |= seg.starts_with(b"JP"),
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

fn parse_dht_segment(seg: &[u8], out: &mut Vec<HuffTable>) -> Result<(), String> {
    let mut p = 0;
    while p + 17 <= seg.len() {
        let tc = seg[p] >> 4;
        let th = seg[p] & 0x0F;
        let mut bits = [0u8; 16];
        bits.copy_from_slice(&seg[p + 1..p + 17]);
        let n: usize = bits.iter().map(|&b| b as usize).sum();
        p += 17;
        if n > 256 || p + n > seg.len() {
            return Err("truncated DHT".into());
        }
        out.push(HuffTable {
            class: tc,
            id: th,
            spec: HuffSpec {
                bits,
                values: seg[p..p + n].to_vec(),
            },
        });
        p += n;
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
            Component {
                id: seg[o],
                h: seg[o + 1] >> 4,
                v: seg[o + 1] & 0x0F,
                tq: seg[o + 2],
            }
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
    tables::quality_of(luma, &tables::STD_LUMA_NATURAL)
}

/// Classify the encoder from parsed structure.
pub fn classify_encoder(s: &JpegStructure) -> EncoderClass {
    if s.has_adobe {
        return EncoderClass::Adobe;
    }
    match s.luma_table().and_then(estimate_libjpeg_quality) {
        Some(quality) => {
            let chroma_matches = match s.chroma_table() {
                Some(c) => tables::quality_of(c, &tables::STD_CHROMA_NATURAL) == Some(quality),
                None => true,
            };
            EncoderClass::LibjpegStandard {
                quality,
                chroma_matches,
            }
        }
        None => EncoderClass::NonStandard,
    }
}

/// Quant-table / structure evidence source.
///
/// Signal: the file is a libjpeg-family (or Adobe) software re-encode rather than a
/// camera-native JPEG, or its structure matches a known writer in the fingerprint DB.
/// `Present` = software re-encode or DB match; `Absent` = tables look
/// camera-native/proprietary and no DB match; `Inconclusive` on unparseable input.
/// This is an encoder-path fact and is never, on its own, evidence of AI generation.
#[derive(Debug, Default)]
pub struct QuantTables {
    /// Writer fingerprint database. Default is the built-in one.
    pub db: FingerprintDb,
}

impl EvidenceSource for QuantTables {
    fn id(&self) -> SourceId {
        SourceId {
            name: "jpeg_quant".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
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
        let huff = s.huffman_class();
        let sub = s.subsampling();
        let fp = s.fingerprint();
        let db_hit = self.db.lookup(&fp);

        let (status, rationale) =
            match (db_hit, &class) {
                (Some(w), _) => (
                    Status::Present,
                    format!(
                        "Structure matches a known writer fingerprint: {} ({}). {}",
                        w.writer,
                        w.class.describe(),
                        if w.class == WriterClass::Generator {
                            "Indicates the export path of a generation tool; the rest of the \
                         layers should be consulted before treating this as origin."
                        } else {
                            "Indicates the encoder path, not authorship."
                        }
                    ),
                ),
                (
                    None,
                    EncoderClass::LibjpegStandard {
                        quality,
                        chroma_matches,
                    },
                ) => (
                    Status::Present,
                    format!(
                    "Standard libjpeg tables at quality {quality}{} with {sub} subsampling and \
                     {} Huffman tables: a software (re-)encode from the libjpeg/PIL/OpenCV \
                     family, not a camera-native JPEG. Common to edited photos, screenshots, \
                     web downloads and generator exports alike; not evidence of origin on its own.",
                    if *chroma_matches { "" } else { " (chroma table modified)" },
                    match huff {
                        HuffmanClass::Standard => "default",
                        HuffmanClass::Optimized => "optimised",
                        HuffmanClass::Arithmetic => "arithmetic (no)",
                        HuffmanClass::Missing => "missing",
                    }
                ),
                ),
                (None, EncoderClass::Adobe) => (
                    Status::Present,
                    format!(
                        "Adobe APP14 marker with {sub} subsampling: saved or exported by an Adobe \
                     application. Indicates editing/re-encoding, not origin."
                    ),
                ),
                (None, EncoderClass::NonStandard) => {
                    (
                        Status::Absent,
                        format!(
                    "Non-standard quantization tables with {sub} subsampling and {} Huffman \
                     tables: consistent with a camera or proprietary encoder rather than a \
                     libjpeg-family re-encode. Not in the writer fingerprint DB.",
                    if huff == HuffmanClass::Standard { "default" } else { "optimised" }
                ),
                    )
                }
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
                "huffman": huff,
                "subsampling": sub,
                "component_ids": s.component_id_style(),
                "fingerprint": fp,
                "writer": db_hit.map(|w| &w.writer),
                "writer_class": db_hit.map(|w| w.class),
                "tables": s.tables.len(),
                "luma_quality": s.luma_table().and_then(estimate_libjpeg_quality),
                "chroma_quality": s.chroma_table().and_then(|c| tables::quality_of(c, &tables::STD_CHROMA_NATURAL)),
                "progressive": s.progressive,
                "restart_interval": s.restart_interval,
                "width": s.width,
                "height": s.height,
                "has_jfif": s.has_jfif,
                "has_exif": s.has_exif,
                "has_xmp": s.has_xmp,
                "has_icc": s.has_icc,
                "has_jumbf": s.has_jumbf,
                "has_adobe": s.has_adobe,
                "trailing_bytes": s.trailing_bytes,
                "markers": s.markers.iter().map(|m| format!("{m:02X}")).collect::<Vec<_>>(),
            }),
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    //! Minimal synthetic JPEG headers shared by the container tests.

    /// SOI + one 8-bit DQT + baseline SOF0 (3 components, 4:2:0) + SOS + EOI.
    pub fn synth_jpeg(table: [u8; 64]) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        v.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
        v.extend_from_slice(&table);
        v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x10, 0x00, 0x10, 0x03]);
        v.extend_from_slice(&[0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::synth_jpeg;
    use super::*;

    fn std_luma_q(q: u8) -> [u8; 64] {
        let t = tables::scale_annex_k(&tables::STD_LUMA_NATURAL, q);
        let mut out = [0u8; 64];
        for (o, &v) in out.iter_mut().zip(t.iter()) {
            *o = v as u8;
        }
        out
    }

    #[test]
    fn parses_structure_and_subsampling() {
        let s = parse_structure(&synth_jpeg([1; 64])).unwrap();
        assert_eq!(s.tables.len(), 1);
        assert_eq!(s.components.len(), 3);
        assert_eq!(s.subsampling(), "4:2:0");
        assert_eq!(s.component_id_style(), "jfif");
        assert_eq!(s.width, 16);
        assert!(!s.progressive);
        assert_eq!(s.huffman_class(), HuffmanClass::Missing);
    }

    #[test]
    fn classifies_libjpeg_quality() {
        let s = parse_structure(&synth_jpeg(std_luma_q(85))).unwrap();
        // Single table → chroma selector points at the same table, which is not the
        // chroma standard, so chroma_matches is false. Luma quality still recovered.
        assert!(matches!(
            classify_encoder(&s),
            EncoderClass::LibjpegStandard { quality: 85, .. }
        ));
    }

    #[test]
    fn quality_100_is_all_ones() {
        assert_eq!(estimate_libjpeg_quality(&[1; 64]), Some(100));
    }

    #[test]
    fn nonstandard_tables_are_not_libjpeg() {
        let mut t = [20u8; 64];
        t[0] = 3;
        t[7] = 200;
        let s = parse_structure(&synth_jpeg(t)).unwrap();
        assert_eq!(classify_encoder(&s), EncoderClass::NonStandard);
    }

    #[test]
    fn fingerprint_is_stable_and_sensitive() {
        let a = parse_structure(&synth_jpeg(std_luma_q(85)))
            .unwrap()
            .fingerprint();
        let b = parse_structure(&synth_jpeg(std_luma_q(85)))
            .unwrap()
            .fingerprint();
        let c = parse_structure(&synth_jpeg(std_luma_q(86)))
            .unwrap()
            .fingerprint();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_structure(b"nope").is_err());
        assert!(parse_structure(&[0xFF, 0xD8, 0xFF, 0xDB, 0xFF, 0xFF]).is_err());
    }
}
