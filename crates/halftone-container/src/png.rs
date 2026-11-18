//! PNG structure forensics: chunk inventory and embedded generation metadata.
//!
//! Why it works: PNG is almost never a camera-native format — a PNG is a screenshot,
//! an export, or a render. The discriminative signals are (1) uncompressed text chunks
//! (`tEXt`/`iTXt`) that generation tools write verbatim — SD WebUI's `parameters`,
//! ComfyUI's `prompt`/`workflow`, a `Software` naming a generator — and (2) the chunk
//! inventory, which fingerprints the writer family (a bare IHDR/IDAT/IEND stream from a
//! minimal library export versus a full sRGB/gAMA/pHYs-decorated writer).
//!
//! The iCCP profile name is part of that inventory, and it is free text chosen by the
//! writer. Every writer measured so far puts a colour-space name or a fixed keyword of
//! its own there: Pillow and Apple ImageIO both write `ICC Profile`, libvips `icc`. On
//! macOS screenshots the display's name is the description inside the profile, not
//! the chunk's name ([`crate::icc`] reads it; `embedded_identifiers` lists it,
//! masked). Nothing stops another writer from copying that description into the
//! name, and for a display or a calibrated profile it names a monitor or whatever
//! the user typed. As a precaution, only known names ([`icc_name_is_known`]) are
//! printed or hashed as written; any other name is reported as present and no more.

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
    /// The EXIF `Software` tag from the `eXIf` chunk, if it has one: the program
    /// that says it wrote the file (`Android <build id>` on an Android screenshot).
    /// Unsigned text. A writer rule can ask for it next to a layout
    /// ([`png_rules::PngRule::exif_software_starts`]); it is not part of
    /// [`PngInfo::fingerprint`], which stays a hash of structure alone.
    pub exif_software: Option<String>,
    /// Whether a `caBX` (JUMBF / C2PA) chunk is present.
    pub has_c2pa: bool,
    /// iCCP profile name as written (e.g. `Display P3`, `sRGB IEC61966-2.1`), if
    /// present. Free text chosen by the writer: one that copied the profile's
    /// description would put a monitor's name or a person's own here. Print
    /// [`PngInfo::icc_known_name`] instead; serialized, the field holds a name only
    /// when it is a known one ([`icc_name_is_known`]).
    #[serde(serialize_with = "serialize_known_icc_name")]
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

/// iCCP profile names that are a writer's fixed keyword: the writer puts the same
/// name on every profile it embeds, so the name says which writer and nothing more.
/// With the names of standard colour spaces (`icc_standard_names.txt`, shared with
/// `embedded_identifiers` and the differential collector) these are the names this
/// crate prints: see [`icc_name_is_known`]. Compared without regard to case or
/// surrounding spaces.
///
/// A name on neither list is not printed and not hashed, because it is free text.
/// Names of display profiles are left out of both on purpose, the generic ones
/// (`Color LCD`, `Display`) included: what a display is called describes the machine
/// the file was made on. No file measured so far carries one as its iCCP name.
///
/// Adding a keyword is safe when every file from that writer carries it.
pub const ICC_CHUNK_KEYWORDS: &[&str] = &[
    "icc",         // libvips, ImageMagick
    "icm",         // ImageMagick, for an .icm source; ExifTool
    "ICC Profile", // Pillow; Apple ImageIO (macOS 27 screenshots)
    "ICC profile", // GIMP
    "Photoshop ICC profile",
    "Skia",
];

/// What stands in for a profile name that is not a known one when the structure is
/// hashed. iCCP names are printable Latin-1, so no real name starts with
/// this byte.
const OTHER_ICC_NAME: &[u8] = b"\x01other profile name";

/// Whether an iCCP profile name is one this crate prints: the name of a standard
/// colour space ([`crate::icc::is_standard_name`], the list `embedded_identifiers`
/// and the differential collector use too) or a writer's fixed keyword
/// ([`ICC_CHUNK_KEYWORDS`]).
pub fn icc_name_is_known(name: &str) -> bool {
    let name = name.trim();
    ICC_CHUNK_KEYWORDS
        .iter()
        .any(|k| k.eq_ignore_ascii_case(name))
        || crate::icc::is_standard_name(name)
}

/// Serialize the profile name only when it is a known one; `null` otherwise.
fn serialize_known_icc_name<S: serde::Serializer>(
    name: &Option<String>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match name.as_deref().filter(|n| icc_name_is_known(n)) {
        Some(n) => s.serialize_some(n),
        None => s.serialize_none(),
    }
}

impl PngInfo {
    /// The iCCP profile name, when it is one this crate prints (see
    /// [`icc_name_is_known`]).
    pub fn icc_known_name(&self) -> Option<&str> {
        self.icc_name.as_deref().filter(|n| icc_name_is_known(n))
    }

    /// Whether a profile is embedded under a name this crate does not print.
    pub fn icc_name_withheld(&self) -> bool {
        self.icc_name
            .as_deref()
            .is_some_and(|n| !icc_name_is_known(n))
    }

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
    ///
    /// A known profile name ([`icc_name_is_known`]) is hashed as written. Any other name is
    /// hashed as one fixed token: it is not known to be the writer's own (a writer
    /// that copied profile descriptions would give one structure a different hash
    /// per display), and a hash of a short name can be reversed by trying the names.
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
        match &self.icc_name {
            Some(n) if icc_name_is_known(n) => h.update(n.as_bytes()),
            Some(_) => h.update(OTHER_ICC_NAME),
            None => {}
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
            b"eXIf" => {
                info.has_exif = true;
                info.exif_software = exif_software(data);
            }
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

/// The EXIF `Software` tag of an `eXIf` chunk's payload (a TIFF block), bounded.
/// `None` when the block cannot be read or has no such tag.
fn exif_software(tiff: &[u8]) -> Option<String> {
    let mut reader = ::exif::Reader::new();
    reader.continue_on_error(true);
    let block = match reader.read_raw(tiff.to_vec()) {
        Ok(x) => x,
        Err(::exif::Error::PartialResult(part)) => part.into_inner().0,
        Err(_) => return None,
    };
    let field = block.get_field(::exif::Tag::Software, ::exif::In::PRIMARY)?;
    let ::exif::Value::Ascii(parts) = &field.value else {
        return None;
    };
    let text = clip(String::from_utf8_lossy(parts.first()?).trim(), 80);
    (!text.is_empty()).then_some(text)
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
        // What the EXIF block says wrote the file, if it says so: quoted, and worth
        // what an unsigned field is worth. With it, "no self-identifying metadata"
        // would be untrue. Whether the name is a generator's is exif_consistency's
        // question, not this source's.
        let exif_says = info.exif_software.as_deref().map(|s| {
            format!(
                "The EXIF block names a program (`Software` = `{s}`), an unsigned field \
                 that anyone can write"
            )
        });

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
                            "PNG structure matches a known writer fingerprint: {} ({}). {}; PNG \
                             is never camera-native, so this is an export-path fact, not \
                             evidence of origin.",
                            w.writer,
                            w.class.describe(),
                            exif_says
                                .as_deref()
                                .unwrap_or("No self-identifying metadata")
                        ),
                    ),
                    (None, None, Some(m)) => (
                        Status::Inconclusive,
                        format!(
                            "{} {} ({}{}). \
                             PNG is never camera-native, so this is an export-path hint, not \
                             evidence of origin.{}",
                            // A rule that asked for the EXIF Software tag found the
                            // name and the layout in agreement; any other rule read
                            // the layout alone.
                            match (&exif_says, m.exif_software) {
                                (Some(e), true) => format!(
                                    "{e}; the chunk layout agrees with it. It is consistent with"
                                ),
                                (Some(e), false) => format!(
                                    "{e}, and no rule ties that name to a layout. The chunk \
                                     layout is consistent with"
                                ),
                                (None, _) => "No self-identifying metadata. Chunk layout is \
                                              consistent with"
                                    .to_string(),
                            },
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
                            "{} no known writer layout. {} chunks \
                             ({} of image data, the largest {} bytes), colour type {}, \
                             {}interlaced{}.",
                            match &exif_says {
                                Some(e) => format!("{e};"),
                                None => "No self-identifying metadata and".to_string(),
                            },
                            info.chunks.len(),
                            info.idat_chunks,
                            info.idat_max,
                            info.color_type,
                            if info.interlace == 0 { "non-" } else { "" },
                            match info.icc_known_name() {
                                Some(n) => format!(", ICC `{n}`"),
                                None if info.icc_name_withheld() => ", an ICC profile under \
                                    a name that is not a standard one (not printed: such a \
                                    name can describe a display or a person's own profile)"
                                    .to_string(),
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
                // As written, bounded: a program's name for itself, which
                // exif_consistency reports too. Unsigned.
                "exif_software": info.exif_software,
                "has_c2pa_chunk": info.has_c2pa,
                // The name when it is a colour space or a writer's fixed keyword
                // (icc_name_is_known). Any other name is free text that can describe a
                // display or a person: said to be there, not repeated.
                "icc_name": info.icc_known_name(),
                "icc_name_withheld": info.icc_name_withheld(),
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

    /// A PNG with the given ancillary chunks and image data in 8 KiB blocks: the
    /// shape Skia's encoder writes.
    fn png_in_8k_blocks(ancillary: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend(ihdr());
        for (ty, data) in ancillary {
            v.extend(chunk(ty, data));
        }
        for n in [8_192, 8_192, 3_000] {
            let mut data = vec![0u8; n];
            data[..2].copy_from_slice(&[0x78, 0x9c]);
            v.extend(chunk(b"IDAT", &data));
        }
        v.extend(chunk(b"IEND", &[]));
        v
    }

    /// An `eXIf` chunk whose IFD0 holds this `Software` tag.
    fn exif_chunk(software: &str) -> (&'static [u8; 4], Vec<u8>) {
        let block = crate::embedded::testutil::exif(&[(0x0131, software)], &[]);
        (b"eXIf", block[6..].to_vec()) // without the JPEG segment's "Exif\0\0"
    }

    #[test]
    fn an_android_screenshot_is_named_from_its_exif_and_its_layout() {
        const BUILD: &str = "Android AB1C.230405.006";
        let skia = |software: Option<&str>| {
            let mut anc = vec![(b"sBIT", vec![8, 8, 8, 8]), (b"sRGB", vec![0])];
            anc.extend(software.map(exif_chunk));
            png_in_8k_blocks(&anc)
        };
        let assess = |bytes: Vec<u8>| {
            PngWriter::default()
                .assess(&Asset::from_bytes(bytes, None).unwrap())
                .unwrap()
        };

        let info = parse_png(&skia(Some(BUILD))).unwrap();
        assert!(info.has_exif);
        assert_eq!(info.exif_software.as_deref(), Some(BUILD));

        let ev = assess(skia(Some(BUILD)));
        assert_eq!(
            ev.status,
            Status::Inconclusive,
            "a screenshot is not a generator"
        );
        assert_eq!(ev.details["exif_software"], BUILD);
        assert_eq!(ev.details["writer_hint"], "screenshot_pipeline");
        assert_eq!(ev.details["writer_rule"]["exif_software"], true);
        let rule = ev.details["writer_rule"]["name"].as_str().unwrap();
        assert!(
            rule.starts_with("Android's system screenshot tool"),
            "{rule}"
        );
        // The rationale quotes the tag, says what it is worth, and no longer says
        // the file names nothing.
        assert!(ev.rationale.contains(&format!("`Software` = `{BUILD}`")));
        assert!(
            ev.rationale
                .contains("unsigned field that anyone can write")
        );
        assert!(ev.rationale.contains("; the chunk layout agrees with it."));
        assert!(!ev.rationale.contains("No self-identifying metadata"));
        assert!(!ev.rationale.contains("not yet verified"));

        // Skia's layout with another program's name, or with no EXIF, is Skia's.
        for other in [Some("Some Canvas App 2.1"), None] {
            let ev = assess(skia(other));
            let rule = ev.details["writer_rule"]["name"].as_str().unwrap();
            assert!(rule.starts_with("Skia's PNG encoder"), "{other:?}: {rule}");
            assert_eq!(ev.details["writer_rule"]["exif_software"], false);
            assert_eq!(ev.details["writer_hint"], "library");
        }
        // With a name the rule does not use, the rationale still quotes it and does
        // not claim the file names nothing; with no EXIF it says what it always did.
        let named = assess(skia(Some("Some Canvas App 2.1")));
        assert!(
            named.rationale.starts_with(
                "The EXIF block names a program (`Software` = `Some Canvas App 2.1`), an \
                 unsigned field that anyone can write, and no rule ties that name to a layout."
            ),
            "{}",
            named.rationale
        );
        assert!(
            assess(skia(None))
                .rationale
                .starts_with("No self-identifying metadata. Chunk layout is consistent with Skia")
        );
        // The same where no rule matches at all: 03-synthetic/id_png_exif.png.
        let bare = assess(png_with(&[exif_chunk("Android EXAMPLE.000000.000")]));
        assert_eq!(bare.details["writer_rule"], serde_json::Value::Null);
        assert!(
            bare.rationale.starts_with(
                "The EXIF block names a program (`Software` = `Android EXAMPLE.000000.000`), \
                 an unsigned field that anyone can write; no known writer layout."
            ),
            "{}",
            bare.rationale
        );
        // The name is reported even where no rule uses it.
        assert_eq!(
            assess(skia(Some("Some Canvas App 2.1"))).details["exif_software"],
            "Some Canvas App 2.1"
        );
        assert_eq!(
            assess(skia(None)).details["exif_software"],
            serde_json::Value::Null
        );

        // The build id is not structure: two builds, one fingerprint, and the same
        // one as before this field existed (the chunk list is all that changed it).
        let other_build = parse_png(&skia(Some("Android ZZ9Z.991231.001"))).unwrap();
        assert_eq!(info.fingerprint(), other_build.fingerprint());
    }

    #[test]
    fn an_exif_chunk_that_cannot_be_read_names_nobody() {
        for junk in [
            Vec::new(),
            b"II".to_vec(),
            b"not a tiff block at all".to_vec(),
            vec![0xFF; 64],
        ] {
            let info = parse_png(&png_with(&[(b"eXIf", junk)])).unwrap();
            assert!(info.has_exif);
            assert_eq!(info.exif_software, None);
        }
        // A block with other tags and no Software, and an empty Software.
        let artist = crate::embedded::testutil::exif(&[(0x013B, "A. Person")], &[]);
        let info = parse_png(&png_with(&[(b"eXIf", artist[6..].to_vec())])).unwrap();
        assert_eq!(info.exif_software, None);
        let info = parse_png(&png_with(&[exif_chunk("   ")])).unwrap();
        assert_eq!(info.exif_software, None);
        // Long values are cut.
        let info = parse_png(&png_with(&[exif_chunk(&"x".repeat(300))])).unwrap();
        assert_eq!(info.exif_software.map(|s| s.len()), Some(80));
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

    /// An `iCCP` payload: name, NUL, compression method, then (here empty) data.
    fn iccp(name: &str) -> (&'static [u8; 4], Vec<u8>) {
        let mut d = name.as_bytes().to_vec();
        d.extend_from_slice(&[0, 0]);
        (b"iCCP", d)
    }

    #[test]
    fn a_standard_profile_name_is_printed_and_any_other_is_not() {
        let assess = |name: &str| {
            let a = Asset::from_bytes(png_with(&[iccp(name)]), None).unwrap();
            PngWriter::default().assess(&a).unwrap()
        };
        for known in [
            "Display P3",
            "sRGB IEC61966-2.1",
            "ICC Profile",
            "icc profile",
            " icc ",
        ] {
            let ev = assess(known);
            assert_eq!(ev.details["icc_name"], known, "{known}");
            assert_eq!(ev.details["icc_name_withheld"], false, "{known}");
        }
        // What a writer that copied the profile's description could put there.
        for other in [
            "ACME UltraView 27Q",
            "Color LCD",
            "Some Person's laptop 2026-11",
        ] {
            let ev = assess(other);
            assert_eq!(ev.details["icc_name"], serde_json::Value::Null, "{other}");
            assert_eq!(ev.details["icc_name_withheld"], true, "{other}");
            let text = serde_json::to_string(&ev).unwrap();
            assert!(!text.contains(other), "{other} leaked: {text}");
            // The parsed structure keeps the name for the rules, and does not hand it on.
            let info = parse_png(&png_with(&[iccp(other)])).unwrap();
            assert_eq!(info.icc_name.as_deref(), Some(other));
            assert_eq!(info.icc_known_name(), None);
            assert!(!serde_json::to_string(&info).unwrap().contains(other));
        }
        let none = PngWriter::default()
            .assess(&Asset::from_bytes(png_with(&[]), None).unwrap())
            .unwrap();
        assert_eq!(none.details["icc_name"], serde_json::Value::Null);
        assert_eq!(none.details["icc_name_withheld"], false);
    }

    #[test]
    fn the_rationale_says_a_profile_is_there_without_naming_it() {
        // No rule covers this shape, so the rationale lists the structure.
        let with = |name: &str| {
            let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
            v.extend(ihdr());
            let (ty, data) = iccp(name);
            v.extend(chunk(ty, &data));
            for n in [16_384usize, 16_384, 100] {
                v.extend(chunk(b"IDAT", &vec![0u8; n]));
            }
            v.extend(chunk(b"IEND", &[]));
            PngWriter::default()
                .assess(&Asset::from_bytes(v, None).unwrap())
                .unwrap()
        };
        let known = with("Adobe RGB (1998)");
        assert!(
            known.rationale.contains("no known writer layout"),
            "{}",
            known.rationale
        );
        assert!(
            known.rationale.contains(", ICC `Adobe RGB (1998)`"),
            "{}",
            known.rationale
        );
        let other = with("ACME UltraView 27Q");
        assert!(
            other
                .rationale
                .contains("an ICC profile under a name that is not a standard one"),
            "{}",
            other.rationale
        );
        assert!(!other.rationale.contains("ACME"), "{}", other.rationale);
    }

    #[test]
    fn the_fingerprint_does_not_depend_on_an_unlisted_profile_name() {
        let fp = |name: Option<&str>| {
            let chunks: Vec<_> = name.map(iccp).into_iter().collect();
            parse_png(&png_with(&chunks)).unwrap().fingerprint()
        };
        // Two unlisted names, one structure.
        assert_eq!(fp(Some("ACME UltraView 27Q")), fp(Some("Other Brand 4K")));
        // Still told apart from a file with a named colour space, and from one with
        // no profile.
        assert_ne!(fp(Some("ACME UltraView 27Q")), fp(Some("Display P3")));
        assert_ne!(fp(Some("ACME UltraView 27Q")), fp(None));
        assert_ne!(fp(Some("Display P3")), fp(Some("sRGB IEC61966-2.1")));
        // A known name is hashed as it always was: these are the values from before
        // other names were replaced by a token.
        assert_eq!(
            fp(Some("Display P3")),
            "4b358d5895a41c3c36e8b213b672ce5708f74b511728ccad0da59fd19d4140da"
        );
        assert_eq!(
            fp(Some("ICC Profile")),
            "98eec946b58afebcea3b4a07ecdca8c4442f60da07898f8d7b8111b3d939d3e0"
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
