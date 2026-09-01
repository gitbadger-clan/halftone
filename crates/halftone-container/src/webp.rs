//! WebP container forensics: chunk inventory, lossy/lossless, embedded metadata.
//!
//! Why it works: WebP is never camera-native. It is a web/export format written by
//! libwebp (Pillow, browsers, CDNs) or a handful of services. The signals are the
//! chunk layout (`VP8 `/`VP8L`/`VP8X` + `ALPH`/`ICCP`/`EXIF`/`XMP `/`ANIM`) and any
//! self-identifying text in the EXIF/XMP chunks, which some generation services keep.

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
use serde::Serialize;

use crate::signatures;

/// Structural summary of a WebP file.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct WebpInfo {
    /// Chunk FourCCs in file order.
    pub chunks: Vec<String>,
    /// `lossy` (VP8), `lossless` (VP8L) or `unknown`.
    pub kind: String,
    /// VP8X extended container (required for alpha/ICC/EXIF/XMP/animation).
    pub extended: bool,
    /// VP8X feature flags, if present.
    pub has_icc: bool,
    /// VP8X: alpha flag.
    pub has_alpha: bool,
    /// VP8X: EXIF flag or EXIF chunk.
    pub has_exif: bool,
    /// VP8X: XMP flag or XMP chunk.
    pub has_xmp: bool,
    /// VP8X: animation flag.
    pub animated: bool,
    /// XMP text, bounded, if present.
    pub xmp: Option<String>,
}

/// Parse the RIFF/WebP chunk structure. Bounds-checked; never panics.
pub fn parse_webp(b: &[u8]) -> Result<WebpInfo, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return Err("not a WebP".into());
    }
    let mut info = WebpInfo {
        kind: "unknown".into(),
        ..Default::default()
    };
    let mut i = 12;
    while i + 8 <= b.len() {
        let fourcc = &b[i..i + 4];
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let start = i + 8;
        let end = start.checked_add(len).ok_or("chunk length overflow")?;
        if end > b.len() {
            return Err("truncated chunk".into());
        }
        let data = &b[start..end];
        info.chunks
            .push(String::from_utf8_lossy(fourcc).into_owned());
        match fourcc {
            b"VP8 " => info.kind = "lossy".into(),
            b"VP8L" => info.kind = "lossless".into(),
            b"VP8X" => {
                info.extended = true;
                if let Some(&flags) = data.first() {
                    info.has_icc |= flags & 0x20 != 0;
                    info.has_alpha |= flags & 0x10 != 0;
                    info.has_exif |= flags & 0x08 != 0;
                    info.has_xmp |= flags & 0x04 != 0;
                    info.animated |= flags & 0x02 != 0;
                }
            }
            b"ICCP" => info.has_icc = true,
            b"EXIF" => info.has_exif = true,
            b"XMP " => {
                info.has_xmp = true;
                info.xmp = Some(String::from_utf8_lossy(data).chars().take(8192).collect());
            }
            b"ANIM" => info.animated = true,
            _ => {}
        }
        i = end + (len & 1); // chunks are padded to even size
    }
    if info.chunks.is_empty() {
        return Err("no chunks".into());
    }
    Ok(info)
}

/// WebP writer / embedded-metadata evidence source.
///
/// `Present` = metadata names a generation tool; `Inconclusive` otherwise (chunk
/// inventory in `details`). Never `Absent`: absence of metadata is not evidence.
#[derive(Debug, Default)]
pub struct WebpWriter;

impl EvidenceSource for WebpWriter {
    fn id(&self) -> SourceId {
        SourceId {
            name: "webp_writer".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
    fn layer(&self) -> Layer {
        Layer::Container
    }
    fn supports(&self, a: &Asset) -> bool {
        a.modality == Modality::Image && a.mime == "image/webp"
    }
    fn assess(&self, a: &Asset) -> halftone_core::Result<Evidence> {
        let info = parse_webp(&a.bytes).map_err(halftone_core::Error::Parse)?;
        let generator = info.xmp.as_deref().and_then(signatures::find_generator);
        let editor = info.xmp.as_deref().and_then(signatures::find_editor);
        let (status, rationale) = match (generator, editor) {
            (Some(g), _) => (
                Status::Present,
                format!("WebP XMP names a generation tool: {g}."),
            ),
            (None, Some(e)) => (
                Status::Inconclusive,
                format!("WebP XMP names an editor ({e}); no generation metadata."),
            ),
            (None, None) => (
                Status::Inconclusive,
                format!(
                    "{} WebP with {} chunks{}{}; no self-identifying metadata. WebP is an export \
                     format, never camera-native; writer inferred from chunk inventory only.",
                    info.kind,
                    info.chunks.len(),
                    if info.has_exif || info.has_xmp {
                        ", carries metadata"
                    } else {
                        ", metadata stripped or never written"
                    },
                    if info.animated { ", animated" } else { "" }
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
                "chunks": info.chunks,
                "kind": info.kind,
                "extended": info.extended,
                "has_icc": info.has_icc,
                "has_alpha": info.has_alpha,
                "has_exif": info.has_exif,
                "has_xmp": info.has_xmp,
                "animated": info.animated,
                "generator": generator,
                "editor": editor,
            }),
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn riff(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        for &(cc, data) in chunks {
            body.extend_from_slice(cc);
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut v = b"RIFF".to_vec();
        v.extend_from_slice(&(body.len() as u32).to_le_bytes());
        v.extend(body);
        v
    }

    #[test]
    fn parses_extended_with_xmp() {
        let xmp = b"<x:xmpmeta><xmp:CreatorTool>Adobe Firefly</xmp:CreatorTool></x:xmpmeta>";
        let b = riff(&[
            (b"VP8X", &[0x0C, 0, 0, 0, 15, 0, 0, 15, 0, 0]),
            (b"XMP ", xmp),
            (b"VP8 ", &[0u8; 10]),
        ]);
        let info = parse_webp(&b).unwrap();
        assert!(info.extended);
        assert_eq!(info.kind, "lossy");
        assert!(info.has_xmp && info.has_exif);
        let a = Asset::from_bytes(b, None).unwrap();
        let ev = WebpWriter.assess(&a).unwrap();
        assert_eq!(ev.status, Status::Present);
        assert!(ev.rationale.contains("Firefly"));
    }

    #[test]
    fn plain_lossy_is_inconclusive() {
        let b = riff(&[(b"VP8 ", &[0u8; 10])]);
        let a = Asset::from_bytes(b, None).unwrap();
        assert_eq!(WebpWriter.assess(&a).unwrap().status, Status::Inconclusive);
    }
}
