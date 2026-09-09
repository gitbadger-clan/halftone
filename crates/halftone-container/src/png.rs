//! PNG structure forensics: chunk inventory and embedded generation metadata.
//!
//! Why it works: PNG is almost never a camera-native format — a PNG is a screenshot,
//! an export, or a render. The discriminative signals are (1) uncompressed text chunks
//! (`tEXt`/`iTXt`) that generation tools write verbatim — SD WebUI's `parameters`,
//! ComfyUI's `prompt`/`workflow`, a `Software` naming a generator — and (2) the chunk
//! inventory, which fingerprints the writer family (a bare IHDR/IDAT/IEND stream from a
//! minimal library export versus a full sRGB/gAMA/pHYs-decorated writer).

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::fingerprints::{FingerprintDb, WriterClass};
use crate::{png_rules, signatures};

/// A decoded PNG text entry (uncompressed `tEXt` or uncompressed `iTXt`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TextEntry {
    /// The keyword (e.g. `parameters`, `prompt`, `Software`, `Comment`).
    pub keyword: String,
    /// The value, bounded in length.
    pub value: String,
}

/// Structural summary of a PNG.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct PngInfo {
    /// Chunk types in file order.
    pub chunks: Vec<String>,
    /// Image width.
    pub width: u32,
    /// Image height.
    pub height: u32,
    /// Bit depth.
    pub bit_depth: u8,
    /// Colour type (0 gray, 2 rgb, 3 palette, 4 gray+a, 6 rgba).
    pub color_type: u8,
    /// Interlace method.
    pub interlace: u8,
    /// Decoded uncompressed text entries.
    pub text: Vec<TextEntry>,
    /// Whether an `eXIf` chunk is present.
    pub has_exif: bool,
    /// Whether a `caBX` (JUMBF / C2PA) chunk is present.
    pub has_c2pa: bool,
    /// iCCP profile name (e.g. `Display P3`, `sRGB IEC61966-2.1`), if present.
    pub icc_name: Option<String>,
    /// pHYs pixels-per-unit (x, y, unit), if present.
    pub phys: Option<(u32, u32, u8)>,
}

impl PngInfo {
    /// Whether a chunk type is present.
    pub fn has(&self, ty: &str) -> bool {
        self.chunks.iter().any(|c| c == ty)
    }

    /// Writer family from the built-in rules (see [`crate::png_rules`]); a short
    /// machine label for `details`. `other` when no rule matches.
    pub fn writer_hint(&self) -> &'static str {
        match png_rules::match_rules(self) {
            Some(m) => match m.class {
                WriterClass::Screenshot => "screenshot_pipeline",
                WriterClass::Editor => "editor",
                WriterClass::Generator => "generator_pipeline",
                WriterClass::Library => "library",
                _ => "known_writer",
            },
            None => "other",
        }
    }

    /// Stable fingerprint of the writer-determined structure: chunk order with the IDAT
    /// run collapsed, IHDR depth/colour/interlace, text keywords, ICC profile name and
    /// pHYs. Image size and pixel data are excluded.
    pub fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        let mut prev_idat = false;
        for c in &self.chunks {
            let idat = c == "IDAT";
            if idat && prev_idat {
                continue;
            }
            prev_idat = idat;
            h.update(c.as_bytes());
            h.update([0]);
        }
        h.update([self.bit_depth, self.color_type, self.interlace]);
        let mut keys: Vec<&str> = self.text.iter().map(|t| t.keyword.as_str()).collect();
        keys.sort_unstable();
        for k in keys {
            h.update(k.as_bytes());
            h.update([1]);
        }
        if let Some(n) = &self.icc_name {
            h.update(n.as_bytes());
        }
        if let Some((x, y, u)) = self.phys {
            h.update(x.to_be_bytes());
            h.update(y.to_be_bytes());
            h.update([u]);
        }
        hex::encode(h.finalize())
    }
}

/// Parse PNG chunk structure and uncompressed text. Bounds-checked; never panics.
pub fn parse_png(b: &[u8]) -> Result<PngInfo, String> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if !b.starts_with(&SIG) {
        return Err("not a PNG".into());
    }
    let mut info = PngInfo::default();
    let mut i = SIG.len();
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let ty = &b[i + 4..i + 8];
        let ty_str = String::from_utf8_lossy(ty).into_owned();
        let data_start = i + 8;
        let data_end = data_start.checked_add(len).ok_or("chunk length overflow")?;
        if data_end + 4 > b.len() {
            return Err("truncated chunk".into());
        }
        let data = &b[data_start..data_end];
        match ty {
            b"IHDR" if data.len() >= 13 => {
                info.width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                info.height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                info.bit_depth = data[8];
                info.color_type = data[9];
                info.interlace = data[12];
            }
            b"tEXt" => {
                if let Some((k, v)) = split_keyword(data) {
                    info.text.push(TextEntry {
                        keyword: k,
                        value: clip(&String::from_utf8_lossy(v), 4096),
                    });
                }
            }
            b"iTXt" => {
                if let Some(entry) = parse_itxt(data) {
                    info.text.push(entry);
                }
            }
            b"eXIf" => info.has_exif = true,
            b"caBX" => info.has_c2pa = true,
            b"iCCP" => {
                if let Some(nul) = data.iter().position(|&c| c == 0) {
                    info.icc_name = Some(clip(&String::from_utf8_lossy(&data[..nul]), 80));
                }
            }
            b"pHYs" if data.len() >= 9 => {
                info.phys = Some((
                    u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
                    u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
                    data[8],
                ));
            }
            _ => {}
        }
        info.chunks.push(ty_str);
        if ty == b"IEND" {
            break;
        }
        i = data_end + 4; // skip CRC
    }
    if info.chunks.first().map(String::as_str) != Some("IHDR") {
        return Err("missing IHDR".into());
    }
    Ok(info)
}

/// Split a `keyword\0value` payload (as in `tEXt`).
fn split_keyword(data: &[u8]) -> Option<(String, &[u8])> {
    let nul = data.iter().position(|&c| c == 0)?;
    let keyword = String::from_utf8_lossy(&data[..nul]).into_owned();
    Some((keyword, &data[nul + 1..]))
}

/// Parse an `iTXt` chunk. Only the uncompressed case (compression flag 0) yields a value.
fn parse_itxt(data: &[u8]) -> Option<TextEntry> {
    let nul = data.iter().position(|&c| c == 0)?;
    let keyword = String::from_utf8_lossy(&data[..nul]).into_owned();
    let rest = data.get(nul + 1..)?;
    // rest = [compression_flag, compression_method, lang\0, translated_keyword\0, text...]
    let comp_flag = *rest.first()?;
    let mut p = 2; // skip compression flag + method
                   // Skip the language tag and translated keyword (both NUL-terminated).
    for _ in 0..2 {
        let off = rest.get(p..)?.iter().position(|&c| c == 0)?;
        p += off + 1;
    }
    let text = rest.get(p..)?;
    if comp_flag == 0 {
        Some(TextEntry {
            keyword,
            value: clip(&String::from_utf8_lossy(text), 4096),
        })
    } else {
        // Compressed (zlib) — record the keyword only; we don't pull in an inflate dep.
        Some(TextEntry {
            keyword,
            value: "<compressed>".into(),
        })
    }
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// PNG writer / embedded-metadata evidence source.
///
/// Signal: the PNG announces a generation tool in a text chunk. `Present` = a generator
/// signature or a known generation-parameters keyword was found; `Inconclusive` = an
/// editor was named or no self-identifying metadata exists (chunk inventory reported in
/// `details`); `Absent` is not used — absence of embedded metadata is not evidence.
#[derive(Debug, Default)]
pub struct PngWriter {
    /// Writer fingerprint database (exact-hash entries); rules are always consulted too.
    pub db: FingerprintDb,
}

/// Keywords that generation pipelines use to store their parameters verbatim.
const GEN_KEYWORDS: &[&str] = &["parameters", "prompt", "workflow", "sd-metadata", "dream"];

impl EvidenceSource for PngWriter {
    fn id(&self) -> SourceId {
        SourceId {
            name: "png_writer".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
    fn layer(&self) -> Layer {
        Layer::Container
    }
    fn supports(&self, a: &Asset) -> bool {
        a.modality == Modality::Image && a.mime == "image/png"
    }
    fn assess(&self, a: &Asset) -> halftone_core::Result<Evidence> {
        let info = parse_png(&a.bytes).map_err(halftone_core::Error::Parse)?;

        // 1) Explicit generator signature anywhere in text values or keywords.
        let mut hit: Option<(String, String)> = None; // (tool, evidence-keyword)
        for t in &info.text {
            if let Some(tool) = signatures::find_generator(&format!("{} {}", t.keyword, t.value)) {
                hit = Some((tool.to_string(), t.keyword.clone()));
                break;
            }
        }
        // 2) A generation-parameters keyword with substantive content, even if the tool
        //    name isn't in our list.
        let param_kw = info
            .text
            .iter()
            .find(|t| {
                GEN_KEYWORDS.contains(&t.keyword.to_lowercase().as_str()) && t.value.len() > 8
            })
            .map(|t| t.keyword.clone());

        let fp = info.fingerprint();
        let db_hit = self.db.lookup(&fp);
        let rule = png_rules::match_rules(&info);

        let (status, rationale) = match (&hit, &param_kw) {
            (Some((tool, kw)), _) => (
                Status::Present,
                format!(
                    "Embedded metadata names a generation tool: {tool} (in PNG text chunk `{kw}`)."
                ),
            ),
            (None, Some(kw)) => (
                Status::Present,
                match &rule {
                    Some(m) if m.class == WriterClass::Generator => format!(
                        "PNG carries a generation-parameters text chunk (`{kw}`); chunk layout \
                         matches {}.",
                        m.name
                    ),
                    _ => format!(
                        "PNG carries a generation-parameters text chunk (`{kw}`) of the kind \
                         written by diffusion pipelines; tool not in the known-signature list."
                    ),
                },
            ),
            (None, None) => {
                let editor = info
                    .text
                    .iter()
                    .find_map(|t| signatures::find_editor(&format!("{} {}", t.keyword, t.value)));
                match (editor, db_hit, &rule) {
                    (Some(ed), _, _) => (
                        Status::Inconclusive,
                        format!("Embedded metadata names an editor ({ed}); no generation metadata."),
                    ),
                    (None, Some(w), _) => (
                        if w.class == WriterClass::Generator { Status::Present } else { Status::Inconclusive },
                        format!(
                            "PNG structure matches a known writer fingerprint: {} ({}). No \
                             self-identifying metadata; PNG is never camera-native, so this is an \
                             export-path fact, not evidence of origin.",
                            w.writer,
                            w.class.describe()
                        ),
                    ),
                    (None, None, Some(m)) => (
                        Status::Inconclusive,
                        format!(
                            "No self-identifying metadata. Chunk layout is consistent with {} ({}{}). \
                             PNG is never camera-native, so this is an export-path hint, not \
                             evidence of origin.{}",
                            m.name,
                            m.class.describe(),
                            if m.confidence == png_rules::Confidence::Documented {
                                "; rule from documented layout, not yet verified"
                            } else {
                                ""
                            },
                            if info.has_c2pa {
                                " A caBX (C2PA) chunk is also present; the layout rule ignores it \
                                 because provenance libraries append it after the writer has finished."
                            } else {
                                ""
                            }
                        ),
                    ),
                    (None, None, None) => (
                        Status::Inconclusive,
                        format!(
                            "No self-identifying metadata and no known writer layout. {} chunks, \
                             colour type {}, {}interlaced{}.",
                            info.chunks.len(),
                            info.color_type,
                            if info.interlace == 0 { "non-" } else { "" },
                            match &info.icc_name {
                                Some(n) => format!(", ICC `{n}`"),
                                None => String::new(),
                            }
                        ),
                    ),
                }
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
                "chunks": info.chunks,
                "width": info.width,
                "height": info.height,
                "bit_depth": info.bit_depth,
                "color_type": info.color_type,
                "interlace": info.interlace,
                "has_exif_chunk": info.has_exif,
                "has_c2pa_chunk": info.has_c2pa,
                "icc_name": info.icc_name,
                "xmp_excerpt": info.text.iter().find(|t| t.keyword == "XML:com.adobe.xmp").map(|t| t.value.chars().take(600).collect::<String>()),
                "phys": info.phys,
                "fingerprint": fp,
                "writer": db_hit.map(|w| &w.writer),
                "writer_class": db_hit.map(|w| w.class),
                "writer_rule": rule,
                "writer_hint": info.writer_hint(),
                "text_keywords": info.text.iter().map(|t| &t.keyword).collect::<Vec<_>>(),
            }),
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crc_placeholder() -> [u8; 4] {
        [0, 0, 0, 0] // parser does not verify CRC
    }

    fn chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = (data.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(ty);
        v.extend_from_slice(data);
        v.extend_from_slice(&crc_placeholder());
        v
    }

    fn ihdr() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&16u32.to_be_bytes()); // width
        d.extend_from_slice(&16u32.to_be_bytes()); // height
        d.extend_from_slice(&[8, 6, 0, 0, 0]); // depth, color, compress, filter, interlace
        chunk(b"IHDR", &d)
    }

    fn png_with(text_chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend(ihdr());
        for (ty, data) in text_chunks {
            v.extend(chunk(ty, data));
        }
        v.extend(chunk(b"IDAT", &[0u8; 4]));
        v.extend(chunk(b"IEND", &[]));
        v
    }

    #[test]
    fn parses_ihdr_and_chunk_order() {
        let png = png_with(&[]);
        let info = parse_png(&png).unwrap();
        assert_eq!(info.width, 16);
        assert_eq!(info.color_type, 6);
        assert_eq!(info.chunks, ["IHDR", "IDAT", "IEND"]);
    }

    #[test]
    fn detects_sd_parameters_chunk() {
        let mut data = b"parameters\0".to_vec();
        data.extend_from_slice(b"masterpiece, Steps: 20, Sampler: Euler a, Model: sd_xl_base");
        let png = png_with(&[(b"tEXt", data)]);
        let a = halftone_core::Asset::from_bytes(png, None).unwrap();
        let ev = PngWriter::default().assess(&a).unwrap();
        assert_eq!(ev.status, Status::Present);
    }

    #[test]
    fn names_known_generator() {
        let mut data = b"Software\0".to_vec();
        data.extend_from_slice(b"ComfyUI");
        let png = png_with(&[(b"tEXt", data)]);
        let a = halftone_core::Asset::from_bytes(png, None).unwrap();
        let ev = PngWriter::default().assess(&a).unwrap();
        assert_eq!(ev.status, Status::Present);
        assert!(ev.rationale.contains("ComfyUI"));
    }

    #[test]
    fn plain_png_is_inconclusive() {
        let a = halftone_core::Asset::from_bytes(png_with(&[]), None).unwrap();
        let ev = PngWriter::default().assess(&a).unwrap();
        assert_eq!(ev.status, Status::Inconclusive);
    }
}
