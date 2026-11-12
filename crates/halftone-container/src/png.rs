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
    /// Number of IDAT chunks the image data is split into.
    pub idat_chunks: usize,
    /// Data length of the largest IDAT chunk, in bytes.
    pub idat_max: usize,
    /// When there are at least two IDAT chunks and all but the last have the same
    /// length (the last being no longer): that length. An encoder that flushes a
    /// fixed-size buffer writes this pattern; one that compresses the whole image and
    /// writes it at once produces a single chunk.
    pub idat_block: Option<usize>,
    /// The two-byte zlib header that opens the image data (compression method and
    /// level hint), from the first IDAT chunk.
    pub zlib_header: Option<[u8; 2]>,
    /// Up to 16 bytes of the compressed stream right after the zlib header: the
    /// start of the first deflate block. Its low bits say how the block is coded,
    /// and an encoder with a hard-coded Huffman table writes the same bytes here in
    /// every file.
    pub deflate_head: Vec<u8>,
}

impl PngInfo {
    /// Whether a chunk type is present.
    pub fn has(&self, ty: &str) -> bool {
        self.chunks.iter().any(|c| c == ty)
    }

    /// How the first deflate block is coded: `stored`, `fixed` or `dynamic` Huffman.
    /// `None` when the stream is too short to say.
    pub fn deflate_block(&self) -> Option<&'static str> {
        match (self.deflate_head.first()? >> 1) & 3 {
            0 => Some("stored"),
            1 => Some("fixed"),
            2 => Some("dynamic"),
            _ => None,
        }
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
    // IDAT shape: length of the first chunk, of the latest one, and whether every
    // chunk before the latest had the first one's length.
    let (mut idat_first, mut idat_last, mut idat_even) = (0usize, 0usize, true);
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
            b"IDAT" => {
                if info.idat_chunks == 0 {
                    idat_first = len;
                    info.zlib_header = data.get(..2).map(|h| [h[0], h[1]]);
                    info.deflate_head = data.iter().skip(2).take(16).copied().collect();
                } else {
                    idat_even &= idat_last == idat_first;
                }
                idat_last = len;
                info.idat_chunks += 1;
                info.idat_max = info.idat_max.max(len);
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
    if info.idat_chunks >= 2 && idat_even && idat_last <= idat_first {
        info.idat_block = Some(idat_first);
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
                        format!(
                            "Embedded metadata names an editor ({ed}); no generation metadata."
                        ),
                    ),
                    (None, Some(w), _) => (
                        if w.class == WriterClass::Generator {
                            Status::Present
                        } else {
                            Status::Inconclusive
                        },
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
                            "No self-identifying metadata and no known writer layout. {} chunks \
                             ({} of image data, the largest {} bytes), colour type {}, \
                             {}interlaced{}.",
                            info.chunks.len(),
                            info.idat_chunks,
                            info.idat_max,
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
                // Whether an XMP packet is there, not what it says: a packet can name
                // people and accounts, and the sources that list such fields mask them.
                "xmp_present": info.text.iter().any(|t| t.keyword == "XML:com.adobe.xmp"),
                "idat_chunks": info.idat_chunks,
                "idat_max": info.idat_max,
                "idat_block": info.idat_block,
                "zlib_header": info.zlib_header.map(hex::encode),
                "deflate_block": info.deflate_block(),
                "deflate_head": hex::encode(&info.deflate_head),
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

    /// A bare PNG whose image data is split into IDAT chunks of the given lengths; the
    /// first starts with the zlib header `78 01`.
    fn png_with_idat(lens: &[usize]) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend(ihdr());
        for (k, &n) in lens.iter().enumerate() {
            let mut data = vec![0u8; n];
            if k == 0 && n >= 2 {
                data[..2].copy_from_slice(&[0x78, 0x01]);
            }
            v.extend(chunk(b"IDAT", &data));
        }
        v.extend(chunk(b"IEND", &[]));
        v
    }

    #[test]
    fn the_idat_shape_is_recorded() {
        let shape = |lens: &[usize]| {
            let i = parse_png(&png_with_idat(lens)).unwrap();
            (i.idat_chunks, i.idat_max, i.idat_block)
        };
        assert_eq!(shape(&[70_000]), (1, 70_000, None));
        assert_eq!(
            shape(&[65_536, 65_536, 65_536, 100]),
            (4, 65_536, Some(65_536))
        );
        assert_eq!(shape(&[8_192, 8_192]), (2, 8_192, Some(8_192)));
        // Uneven chunks, or a last one longer than the rest, are not a block pattern.
        assert_eq!(shape(&[100, 200, 100]), (3, 200, None));
        assert_eq!(shape(&[100, 100, 300]), (3, 300, None));
        assert_eq!(shape(&[]), (0, 0, None));
        assert_eq!(
            parse_png(&png_with_idat(&[70_000])).unwrap().zlib_header,
            Some([0x78, 0x01])
        );
        assert_eq!(parse_png(&png_with_idat(&[1])).unwrap().zlib_header, None);
    }

    #[test]
    fn a_bare_stream_names_a_writer_only_as_far_as_the_shape_allows() {
        let assess = |lens: &[usize]| {
            let a = Asset::from_bytes(png_with_idat(lens), None).unwrap();
            PngWriter::default().assess(&a).unwrap()
        };
        // One large chunk: what candle's Stable Diffusion example writes. Not Pillow.
        let one = assess(&[440_932]);
        assert_eq!(one.status, Status::Inconclusive);
        assert!(!one.rationale.contains("Pillow"), "{}", one.rationale);
        assert!(
            one.rationale.contains("a one-shot PNG encoder"),
            "{}",
            one.rationale
        );
        assert_eq!(one.details["deflate_block"], "stored");
        assert_eq!(one.details["idat_chunks"], 1);
        assert_eq!(one.details["idat_max"], 440_932);
        assert_eq!(one.details["zlib_header"], "7801");
        // 64 KiB blocks: Pillow.
        let blocks = assess(&[65_536, 65_536, 1_000]);
        assert!(
            blocks.rationale.contains("Pillow default"),
            "{}",
            blocks.rationale
        );
        assert_eq!(blocks.details["idat_block"], 65_536);
        // Small: no writer named.
        let small = assess(&[4_851]);
        assert!(
            small.rationale.contains("writer cannot be told"),
            "{}",
            small.rationale
        );
        // 8 KiB blocks: libpng's default buffer, as OpenCV writes a bare file.
        let libpng = assess(&[8_192, 8_192, 100]);
        assert!(
            libpng.rationale.contains("OpenCV writes this"),
            "{}",
            libpng.rationale
        );
        // A shape nobody was measured to write: no known layout, with the facts.
        let other = assess(&[16_384, 16_384, 100]);
        assert!(
            other.rationale.contains(
                "no known writer layout. 5 chunks (3 of image data, the largest 16384 bytes)"
            ),
            "{}",
            other.rationale
        );
    }

    #[test]
    fn details_say_that_xmp_is_there_not_what_it_says() {
        let mut itxt = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
        itxt.extend_from_slice(b"<x:xmpmeta><pdf:Author>Some Person</pdf:Author></x:xmpmeta>");
        let a = Asset::from_bytes(png_with(&[(b"iTXt", itxt)]), None).unwrap();
        let ev = PngWriter::default().assess(&a).unwrap();
        assert_eq!(ev.details["xmp_present"], true);
        assert!(!serde_json::to_string(&ev).unwrap().contains("Some Person"));
        let plain = Asset::from_bytes(png_with(&[]), None).unwrap();
        assert_eq!(
            PngWriter::default().assess(&plain).unwrap().details["xmp_present"],
            false
        );
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
