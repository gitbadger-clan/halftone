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

use crate::signatures;

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
}

impl PngInfo {
    /// Whether a chunk type is present.
    pub fn has(&self, ty: &str) -> bool {
        self.chunks.iter().any(|c| c == ty)
    }

    /// Heuristic writer family from the ancillary-chunk inventory. A hint, not a
    /// fingerprint: this is what the layout is *consistent with*.
    pub fn writer_hint(&self) -> &'static str {
        let anc = |t: &str| self.has(t);
        let bare = !anc("pHYs") && !anc("gAMA") && !anc("sRGB") && !anc("cHRM") && !anc("iCCP");
        if anc("iDOT") {
            "apple_imageio" // Apple's private parallel-decode chunk: macOS/iOS screenshots & exports
        } else if self.text.iter().any(|t| t.keyword == "XML:com.adobe.xmp") && anc("pHYs") {
            "adobe"
        } else if bare {
            "minimal_library" // Pillow default, many generation pipelines, some web tools
        } else if anc("sRGB") && anc("gAMA") && anc("pHYs") {
            "libpng_full" // libpng-based apps, Windows imaging, GIMP, browsers
        } else {
            "other"
        }
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
pub struct PngWriter;

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

        let (status, rationale) = match (&hit, &param_kw) {
            (Some((tool, kw)), _) => (
                Status::Present,
                format!(
                    "Embedded metadata names a generation tool: {tool} (in PNG text chunk `{kw}`)."
                ),
            ),
            (None, Some(kw)) => (
                Status::Present,
                format!(
                    "PNG carries a generation-parameters text chunk (`{kw}`) of the kind written \
                     by diffusion pipelines; tool not in the known-signature list."
                ),
            ),
            (None, None) => {
                let editor = info
                    .text
                    .iter()
                    .find_map(|t| signatures::find_editor(&format!("{} {}", t.keyword, t.value)));
                match editor {
                    Some(ed) => (
                        Status::Inconclusive,
                        format!("Embedded metadata names an editor ({ed}); no generation metadata."),
                    ),
                    None => (
                        Status::Inconclusive,
                        format!(
                            "No self-identifying metadata. {} chunks, colour type {}, {}interlaced; \
                             layout consistent with a {} writer. PNG is never camera-native, so \
                             this is an export-path hint, not evidence of origin.",
                            info.chunks.len(),
                            info.color_type,
                            if info.interlace == 0 { "non-" } else { "" },
                            info.writer_hint().replace('_', " ")
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
        let ev = PngWriter.assess(&a).unwrap();
        assert_eq!(ev.status, Status::Present);
    }

    #[test]
    fn names_known_generator() {
        let mut data = b"Software\0".to_vec();
        data.extend_from_slice(b"ComfyUI");
        let png = png_with(&[(b"tEXt", data)]);
        let a = halftone_core::Asset::from_bytes(png, None).unwrap();
        let ev = PngWriter.assess(&a).unwrap();
        assert_eq!(ev.status, Status::Present);
        assert!(ev.rationale.contains("ComfyUI"));
    }

    #[test]
    fn plain_png_is_inconclusive() {
        let a = halftone_core::Asset::from_bytes(png_with(&[]), None).unwrap();
        let ev = PngWriter.assess(&a).unwrap();
        assert_eq!(ev.status, Status::Inconclusive);
    }
}
