//! Marking metadata: the IPTC `DigitalSourceType` self-declaration and the legacy
//! IPTC IIM block, reported as explicit fields.
//!
//! Where the signal lives: the IPTC Photo Metadata Standard defines
//! `Iptc4xmpExt:DigitalSourceType` (namespace `http://iptc.org/std/Iptc4xmpExt/2008-02-29/`)
//! whose value is a term from the IPTC *Digital Source Type* NewsCodes vocabulary
//! (`http://cv.iptc.org/newscodes/digitalsourcetype/<term>`). The two terms
//! `trainedAlgorithmicMedia` and `compositeWithTrainedAlgorithmicMedia` are the
//! agreed way for a generator to state, in machine-readable metadata, that an image
//! was produced with or includes generative AI. C2PA reuses the same vocabulary inside
//! its actions assertion; that copy is signed and is Layer 1's business. This source
//! reads the *unsigned* copy in XMP, wherever the container puts it:
//! - JPEG: APP1 `http://ns.adobe.com/xap/1.0/` packet plus ExtendedXMP
//!   (`http://ns.adobe.com/xmp/extension/`) reassembled by offset; APP13 Photoshop IRB
//!   resource `0x0404` for the IIM block.
//! - PNG: `iTXt` with keyword `XML:com.adobe.xmp`.
//! - WebP: the `XMP ` chunk.
//!
//! Who holds the key: nobody. It is plain text written by whatever software last
//! saved the file; anything can add it, anything can remove it. What it survives: only
//! paths that preserve XMP, which most messaging and many CDN paths do not. How it is
//! scored: it is not; the field is either there or not, and this source reports which.
//!
//! Status semantics:
//! - `Present`: a generative term is declared. The file self-declares as (partly)
//!   AI-generated. Says nothing about whether that is true.
//! - `Absent`: no `DigitalSourceType` field, or only non-generative terms. Absence
//!   says nothing about origin; most files carry no marking.
//! - `Inconclusive`: a value outside the vocabulary snapshot this build knows, or
//!   conflicting values. Reported verbatim, never interpreted.
//!
//! The vocabulary and its version live in [`halftone_core::dst`], shared with the
//! manifest layer so both readings are judged against the same term list.
//!
//! The IIM block (record 2, datasets 2:65 *Originating Program* and 2:70 *Program
//! Version*) cannot carry `DigitalSourceType`; it is reported in `details` so a
//! report can show what the legacy block says, and never affects the status.
//!
//! No XML parser is used. XMP in the wild is written by hand-rolled serialisers,
//! is frequently not well-formed, and is often padded; a tolerant scan over the raw
//! packet finds the field in attribute form
//! (`Iptc4xmpExt:DigitalSourceType="…"`), element form
//! (`<Iptc4xmpExt:DigitalSourceType>…</…>`), and resource form
//! (`<… rdf:resource="…"/>`), with any namespace prefix. Whether the prefix is bound
//! to the IPTC Extension namespace is recorded, not required.

use halftone_core::dst::{code_of, lookup, Term, TermKind, IPTC_EXT_NS, VOCABULARY_VERSION};
use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
use serde::Serialize;

/// Upper bound on assembled XMP bytes per file, to keep hostile input cheap.
const MAX_XMP_BYTES: usize = 4 * 1024 * 1024;

/// Syntactic form a value was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    /// `prefix:DigitalSourceType="…"` on an element.
    Attribute,
    /// `<prefix:DigitalSourceType>…</prefix:DigitalSourceType>`.
    Element,
    /// `<prefix:DigitalSourceType rdf:resource="…"/>`.
    Resource,
}

/// One `DigitalSourceType` occurrence found in XMP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DstValue {
    /// Namespace prefix the property was written with (`Iptc4xmpExt` per the standard).
    pub prefix: String,
    /// The value exactly as written, trimmed.
    pub raw: String,
    /// Term code: the last path segment of `raw` (or `raw` itself if it has no `/`).
    pub code: String,
    /// Where the value sat.
    pub form: Form,
    /// Whether the prefix is bound to [`IPTC_EXT_NS`] somewhere in the packet.
    pub namespace_bound: bool,
}

/// Legacy IPTC IIM fields from the APP13 Photoshop resource block.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct IimInfo {
    /// Number of IIM datasets decoded.
    pub dataset_count: usize,
    /// 2:65 Originating Program.
    pub originating_program: Option<String>,
    /// 2:70 Program Version.
    pub program_version: Option<String>,
    /// 2:40 Special Instructions, bounded.
    pub special_instructions: Option<String>,
}

/// Everything this source extracted from one asset.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MarkingScan {
    /// Number of XMP packets found (JPEG standard + extension chunks count as one
    /// packet each after reassembly; PNG/WebP normally have one).
    pub xmp_packets: usize,
    /// Total XMP bytes examined.
    pub xmp_bytes: usize,
    /// JPEG only: an ExtendedXMP packet was present.
    pub xmp_extended: bool,
    /// JPEG only: ExtendedXMP was present but could not be fully reassembled.
    pub xmp_extended_incomplete: bool,
    /// Some packet binds a prefix to [`IPTC_EXT_NS`].
    pub iptc_ext_namespace_declared: bool,
    /// Every `DigitalSourceType` occurrence, in file order, duplicates kept.
    pub values: Vec<DstValue>,
    /// IIM block, if an APP13 IPTC resource was present.
    pub iim: Option<IimInfo>,
    /// A C2PA manifest container is present (JPEG APP11 JUMBF, PNG `caBX`, WebP
    /// `C2PA`). Any `digitalSourceType` inside it is signed and belongs to the
    /// manifest layer; recorded here so the rationale can point the reader there.
    pub manifest_container_present: bool,
}

/// Extract marking metadata from JPEG, PNG or WebP bytes.
pub fn scan(mime: &str, b: &[u8]) -> Result<MarkingScan, String> {
    let mut s = MarkingScan::default();
    let packets = match mime {
        "image/jpeg" => jpeg_packets(b, &mut s)?,
        "image/png" => png_packets(b, &mut s)?,
        "image/webp" => webp_packets(b, &mut s)?,
        _ => return Err(format!("unsupported container {mime}")),
    };
    for p in packets {
        s.xmp_packets += 1;
        s.xmp_bytes += p.len();
        let text = String::from_utf8_lossy(&p);
        let bound = text.contains(IPTC_EXT_NS);
        s.iptc_ext_namespace_declared |= bound;
        s.values.extend(find_dst(&text));
    }
    Ok(s)
}

/// ExtendedXMP under reassembly: one GUID, declared full length, (offset, data) chunks.
struct ExtendedXmp {
    guid: Vec<u8>,
    full: usize,
    chunks: Vec<(usize, Vec<u8>)>,
}

/// Walk JPEG segments up to SOS: collect the standard XMP packet, reassemble
/// ExtendedXMP, and decode the APP13 IPTC resource. Bounds-checked; never panics.
fn jpeg_packets(b: &[u8], s: &mut MarkingScan) -> Result<Vec<Vec<u8>>, String> {
    const XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
    const EXT: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
    const IRB: &[u8] = b"Photoshop 3.0\0";
    if !b.starts_with(&[0xFF, 0xD8]) {
        return Err("not a JPEG".into());
    }
    let mut packets = Vec::new();
    let mut ext: Option<ExtendedXmp> = None;
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
        match marker {
            0xD9 | 0xDA => break,
            0x01 | 0xD0..=0xD7 => continue,
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
            0xE1 => {
                if let Some(rest) = seg.strip_prefix(XMP) {
                    packets.push(rest.to_vec());
                } else if let Some(rest) = seg.strip_prefix(EXT) {
                    s.xmp_extended = true;
                    // GUID (32 ASCII bytes), full length u32, offset u32, data.
                    if rest.len() >= 40 {
                        let guid = rest[..32].to_vec();
                        let full =
                            u32::from_be_bytes([rest[32], rest[33], rest[34], rest[35]]) as usize;
                        let off =
                            u32::from_be_bytes([rest[36], rest[37], rest[38], rest[39]]) as usize;
                        let data = rest[40..].to_vec();
                        let e = ext.get_or_insert_with(|| ExtendedXmp {
                            guid: guid.clone(),
                            full,
                            chunks: Vec::new(),
                        });
                        if e.guid == guid && e.full == full && full <= MAX_XMP_BYTES {
                            e.chunks.push((off, data));
                        } else {
                            s.xmp_extended_incomplete = true;
                        }
                    } else {
                        s.xmp_extended_incomplete = true;
                    }
                }
            }
            0xEB => s.manifest_container_present |= seg.starts_with(b"JP"),
            0xED => {
                if let Some(rest) = seg.strip_prefix(IRB) {
                    if let Some(iim) = parse_irb_iptc(rest) {
                        s.iim = Some(iim);
                    }
                }
            }
            _ => {}
        }
        i += len;
    }
    if let Some(ExtendedXmp {
        full, mut chunks, ..
    }) = ext
    {
        chunks.sort_by_key(|c| c.0);
        let mut buf = vec![0u8; full];
        let mut covered = 0usize;
        let mut ok = true;
        for (off, data) in chunks {
            if off != covered || off + data.len() > full {
                ok = false;
                break;
            }
            buf[off..off + data.len()].copy_from_slice(&data);
            covered += data.len();
        }
        if ok && covered == full {
            packets.push(buf);
        } else {
            s.xmp_extended_incomplete = true;
        }
    }
    Ok(packets)
}

/// Decode the IPTC IIM datasets from the Photoshop image-resource block (resource
/// id `0x0404`). Returns `None` if no such resource exists.
fn parse_irb_iptc(b: &[u8]) -> Option<IimInfo> {
    let mut i = 0;
    while i + 12 <= b.len() {
        if &b[i..i + 4] != b"8BIM" {
            return None;
        }
        let id = u16::from_be_bytes([b[i + 4], b[i + 5]]);
        let name_len = b[i + 6] as usize;
        // Pascal string, padded so that (1 + name_len) is even.
        let mut p = i + 7 + name_len;
        if (1 + name_len) % 2 == 1 {
            p += 1;
        }
        if p + 4 > b.len() {
            return None;
        }
        let size = u32::from_be_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]) as usize;
        let start = p + 4;
        let end = start.checked_add(size)?;
        if end > b.len() {
            return None;
        }
        if id == 0x0404 {
            return Some(parse_iim(&b[start..end]));
        }
        i = end + (size & 1);
    }
    None
}

/// Decode IIM datasets: `0x1C record dataset size(u16) data`, extended sizes skipped.
fn parse_iim(b: &[u8]) -> IimInfo {
    let mut info = IimInfo::default();
    let mut i = 0;
    while i + 5 <= b.len() {
        if b[i] != 0x1C {
            break;
        }
        let record = b[i + 1];
        let dataset = b[i + 2];
        let size = u16::from_be_bytes([b[i + 3], b[i + 4]]);
        if size & 0x8000 != 0 {
            // Extended dataset length; nothing this source reads uses it.
            break;
        }
        let start = i + 5;
        let end = match start.checked_add(size as usize) {
            Some(e) if e <= b.len() => e,
            _ => break,
        };
        info.dataset_count += 1;
        let text = || clip(String::from_utf8_lossy(&b[start..end]).trim(), 256);
        if record == 2 {
            match dataset {
                65 => info.originating_program = Some(text()),
                70 => info.program_version = Some(text()),
                40 => info.special_instructions = Some(text()),
                _ => {}
            }
        }
        i = end;
    }
    info
}

/// Collect XMP packets from PNG `iTXt` chunks keyed `XML:com.adobe.xmp`
/// (uncompressed only, per the XMP specification). Bounds-checked; never panics.
fn png_packets(b: &[u8], s: &mut MarkingScan) -> Result<Vec<Vec<u8>>, String> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if !b.starts_with(&SIG) {
        return Err("not a PNG".into());
    }
    let mut packets = Vec::new();
    let mut i = SIG.len();
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let ty = &b[i + 4..i + 8];
        let start = i + 8;
        let end = start.checked_add(len).ok_or("chunk length overflow")?;
        if end + 4 > b.len() {
            return Err("truncated chunk".into());
        }
        if ty == b"iTXt" {
            let data = &b[start..end];
            if let Some(nul) = data.iter().position(|&c| c == 0) {
                if &data[..nul] == b"XML:com.adobe.xmp" {
                    // [compression flag, method, lang\0, translated keyword\0, text]
                    let rest = &data[nul + 1..];
                    if rest.first() == Some(&0) {
                        let mut p = 2;
                        let mut ok = true;
                        for _ in 0..2 {
                            match rest.get(p..).and_then(|r| r.iter().position(|&c| c == 0)) {
                                Some(off) => p += off + 1,
                                None => {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        if ok {
                            if let Some(text) = rest.get(p..) {
                                packets.push(text[..text.len().min(MAX_XMP_BYTES)].to_vec());
                            }
                        }
                    }
                }
            }
        }
        if ty == b"caBX" {
            s.manifest_container_present = true;
        }
        if ty == b"IEND" {
            break;
        }
        i = end + 4;
    }
    Ok(packets)
}

/// Collect the WebP `XMP ` chunk. Bounds-checked; never panics.
fn webp_packets(b: &[u8], s: &mut MarkingScan) -> Result<Vec<Vec<u8>>, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return Err("not a WebP".into());
    }
    let mut packets = Vec::new();
    let mut i = 12;
    while i + 8 <= b.len() {
        let fourcc = &b[i..i + 4];
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let start = i + 8;
        let end = start.checked_add(len).ok_or("chunk length overflow")?;
        if end > b.len() {
            return Err("truncated chunk".into());
        }
        if fourcc == b"XMP " {
            let data = &b[start..end];
            packets.push(data[..data.len().min(MAX_XMP_BYTES)].to_vec());
        }
        if fourcc == b"C2PA" {
            s.manifest_container_present = true;
        }
        i = end + (len & 1);
    }
    Ok(packets)
}

/// Find every `DigitalSourceType` property in an XMP packet, in any prefix and form.
pub fn find_dst(text: &str) -> Vec<DstValue> {
    const NAME: &str = "DigitalSourceType";
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(NAME) {
        let at = from + rel;
        from = at + NAME.len();
        // Must be `prefix:DigitalSourceType` and not a closing tag or a longer name.
        if at == 0 || bytes[at - 1] != b':' {
            continue;
        }
        let after = &text[at + NAME.len()..];
        if after
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            continue;
        }
        let prefix_start = text[..at - 1]
            .rfind(|c: char| c == '<' || c == '/' || c.is_whitespace())
            .map(|p| p + 1)
            .unwrap_or(0);
        if prefix_start > 0 && bytes[prefix_start - 1] == b'/' {
            continue; // `</prefix:DigitalSourceType>`
        }
        let prefix = &text[prefix_start..at - 1];
        if prefix.is_empty() {
            continue;
        }
        let opened_by_tag = prefix_start > 0 && bytes[prefix_start - 1] == b'<';
        let trimmed = after.trim_start();
        let (raw, form) = if let Some(rest) = trimmed.strip_prefix('=') {
            match quoted(rest) {
                Some(v) => (v, Form::Attribute),
                None => continue,
            }
        } else if opened_by_tag {
            // Element or resource form: read to the end of the start tag.
            let Some(gt) = after.find('>') else { continue };
            let head = &after[..gt];
            if let Some(pos) = head.find("rdf:resource") {
                let tail = head[pos + "rdf:resource".len()..].trim_start();
                match tail.strip_prefix('=').and_then(|t| quoted(t.trim_start())) {
                    Some(v) => (v, Form::Resource),
                    None => continue,
                }
            } else if head.trim_end().ends_with('/') {
                continue; // empty element, no value
            } else {
                let body = &after[gt + 1..];
                let Some(lt) = body.find('<') else { continue };
                (body[..lt].to_string(), Form::Element)
            }
        } else {
            continue;
        };
        let raw = clip(raw.trim(), 512);
        if raw.is_empty() {
            continue;
        }
        let code = code_of(&raw);
        let namespace_bound = text.contains(&format!("xmlns:{prefix}=\"{IPTC_EXT_NS}\""))
            || text.contains(&format!("xmlns:{prefix}='{IPTC_EXT_NS}'"));
        out.push(DstValue {
            prefix: prefix.to_string(),
            raw,
            code,
            form,
            namespace_bound,
        });
    }
    out
}

/// Read a single- or double-quoted string at the start of `s`.
fn quoted(s: &str) -> Option<String> {
    let q = s.chars().next()?;
    if q != '"' && q != '\'' {
        return None;
    }
    let inner = &s[1..];
    let end = inner.find(q)?;
    Some(inner[..end].to_string())
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// IPTC marking-metadata evidence source.
///
/// `Present` = XMP declares a generative `DigitalSourceType`; `Absent` = no such
/// field, or a declared non-generative source; `Inconclusive` = a value outside the
/// known vocabulary or conflicting values. An unsigned self-declaration: it records
/// what the writer stated, verifies nothing, and is removed by any XMP-stripping path.
#[derive(Debug, Default)]
pub struct MarkingMetadata;

impl EvidenceSource for MarkingMetadata {
    fn id(&self) -> SourceId {
        SourceId {
            name: "marking_metadata".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
    fn layer(&self) -> Layer {
        Layer::Container
    }
    fn supports(&self, a: &Asset) -> bool {
        a.modality == Modality::Image
            && matches!(a.mime.as_str(), "image/jpeg" | "image/png" | "image/webp")
    }
    fn assess(&self, a: &Asset) -> halftone_core::Result<Evidence> {
        let s = scan(&a.mime, &a.bytes).map_err(halftone_core::Error::Parse)?;

        // Distinct codes in file order.
        let mut codes: Vec<&str> = Vec::new();
        for v in &s.values {
            if !codes.contains(&v.code.as_str()) {
                codes.push(&v.code);
            }
        }
        let terms: Vec<Option<&'static Term>> = codes.iter().map(|c| lookup(c)).collect();
        let unknown: Vec<&str> = codes
            .iter()
            .zip(&terms)
            .filter(|(_, t)| t.is_none())
            .map(|(c, _)| *c)
            .collect();
        let generative = terms
            .iter()
            .flatten()
            .any(|t| t.kind == TermKind::Generative);
        let non_generative = terms
            .iter()
            .flatten()
            .any(|t| t.kind != TermKind::Generative);

        const CAVEAT: &str = "This is an unsigned self-declaration written by whatever \
            software last saved the file: it records what the writer chose to state, \
            verifies nothing, and is removed by any re-save or messaging path that strips \
            XMP.";

        let (status, rationale) = if codes.is_empty() {
            let where_ = if s.xmp_extended_incomplete {
                "XMP present, ExtendedXMP could not be fully reassembled"
            } else if s.xmp_packets > 0 {
                "XMP present but without the field"
            } else {
                "no XMP"
            };
            let iim = match &s.iim {
                Some(_) => "; an IPTC IIM block is present, which cannot carry it",
                None => "",
            };
            let manifest = if s.manifest_container_present {
                " A C2PA manifest container is present: any source-type declaration inside \
                 it is signed and is reported by the manifest layer, not here."
            } else {
                ""
            };
            let status = if s.xmp_extended_incomplete {
                Status::Inconclusive
            } else {
                Status::Absent
            };
            (
                status,
                format!(
                    "No IPTC DigitalSourceType marking found ({where_}{iim}). The file does \
                     not self-declare its source type in XMP. Absence says nothing about \
                     origin: most files carry no marking, and any marking is stripped by \
                     common re-save and messaging paths.{manifest}"
                ),
            )
        } else if !unknown.is_empty() {
            (
                Status::Inconclusive,
                format!(
                    "XMP carries a DigitalSourceType value outside the IPTC vocabulary snapshot \
                     this build knows ({}): reported verbatim, not interpreted.",
                    unknown.join(", ")
                ),
            )
        } else if generative && non_generative {
            (
                Status::Inconclusive,
                format!(
                    "XMP carries conflicting DigitalSourceType values ({}); not interpreted. \
                     {CAVEAT}",
                    codes.join(", ")
                ),
            )
        } else if generative {
            let t = terms.iter().flatten().next().map(|t| (t.code, t.label));
            let (code, label) = t.unwrap_or(("", ""));
            (
                Status::Present,
                format!(
                    "XMP declares IPTC DigitalSourceType = {code} ({label}): the file \
                     self-declares as AI-generated. {CAVEAT}"
                ),
            )
        } else {
            let t = terms
                .iter()
                .flatten()
                .next()
                .map(|t| (t.code, t.label, t.kind));
            let (code, label, kind) = t.unwrap_or(("", "", TermKind::NonGenerative));
            let retired = if kind == TermKind::Retired {
                " The term is retired from the vocabulary."
            } else {
                ""
            };
            (
                Status::Absent,
                format!(
                    "XMP declares IPTC DigitalSourceType = {code} ({label}), a self-declared \
                     non-generative source; no generative marking.{retired} {CAVEAT}"
                ),
            )
        };

        let unbound = s.values.iter().any(|v| !v.namespace_bound);
        Ok(Evidence {
            layer: self.layer(),
            source: self.id(),
            status,
            statistic: None,
            calibration: None,
            rationale,
            details: serde_json::json!({
                "vocabulary_version": VOCABULARY_VERSION,
                "digital_source_type": codes,
                "values": s.values,
                "prefix_not_bound_to_iptc_ext": unbound,
                "iptc_ext_namespace_declared": s.iptc_ext_namespace_declared,
                "xmp_packets": s.xmp_packets,
                "xmp_bytes": s.xmp_bytes,
                "xmp_extended": s.xmp_extended,
                "xmp_extended_incomplete": s.xmp_extended_incomplete,
                "iim": s.iim,
                "manifest_container_present": s.manifest_container_present,
            }),
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jpeg::testutil::synth_jpeg;

    const GEN: &str = "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia";

    fn xmp_attr(value: &str) -> String {
        format!(
            "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta \
             xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
             xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description \
             rdf:about=\"\" xmlns:Iptc4xmpExt=\"{IPTC_EXT_NS}\" \
             Iptc4xmpExt:DigitalSourceType=\"{value}\"/></rdf:RDF></x:xmpmeta><?xpacket \
             end=\"w\"?>"
        )
    }

    fn jpeg_with_segments(segments: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let base = synth_jpeg([1; 64]);
        let mut v = vec![0xFF, 0xD8];
        for (marker, payload) in segments {
            v.push(0xFF);
            v.push(*marker);
            v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
            v.extend_from_slice(payload);
        }
        v.extend_from_slice(&base[2..]);
        v
    }

    fn app1_xmp(packet: &str) -> (u8, Vec<u8>) {
        let mut p = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        p.extend_from_slice(packet.as_bytes());
        (0xE1, p)
    }

    fn assess(bytes: Vec<u8>) -> Evidence {
        let a = Asset::from_bytes(bytes, None).unwrap();
        MarkingMetadata.assess(&a).unwrap()
    }

    #[test]
    fn attribute_form_generative_is_present() {
        let ev = assess(jpeg_with_segments(&[app1_xmp(&xmp_attr(GEN))]));
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
        assert!(ev.rationale.contains("trainedAlgorithmicMedia"));
        assert!(ev.rationale.contains("self-declaration"));
        assert_eq!(
            ev.details["digital_source_type"][0],
            "trainedAlgorithmicMedia"
        );
        assert_eq!(ev.details["values"][0]["form"], "attribute");
        assert_eq!(ev.details["values"][0]["namespace_bound"], true);
        assert_eq!(ev.details["iptc_ext_namespace_declared"], true);
    }

    #[test]
    fn element_form_with_other_prefix_and_bare_term() {
        let packet =
            "<rdf:Description xmlns:iptcExt=\"http://iptc.org/std/Iptc4xmpExt/2008-02-29/\">\
                      <iptcExt:DigitalSourceType>compositeWithTrainedAlgorithmicMedia\
                      </iptcExt:DigitalSourceType></rdf:Description>";
        let ev = assess(jpeg_with_segments(&[app1_xmp(packet)]));
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
        let v = &ev.details["values"];
        assert_eq!(v.as_array().unwrap().len(), 1, "closing tag must not count");
        assert_eq!(v[0]["prefix"], "iptcExt");
        assert_eq!(v[0]["form"], "element");
        assert_eq!(v[0]["code"], "compositeWithTrainedAlgorithmicMedia");
    }

    #[test]
    fn resource_form_is_found() {
        let packet = format!(
            "<rdf:Description><Iptc4xmpExt:DigitalSourceType rdf:resource=\"{GEN}\"/>\
             </rdf:Description>"
        );
        let ev = assess(jpeg_with_segments(&[app1_xmp(&packet)]));
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
        assert_eq!(ev.details["values"][0]["form"], "resource");
        assert_eq!(ev.details["values"][0]["namespace_bound"], false);
        assert_eq!(ev.details["prefix_not_bound_to_iptc_ext"], true);
    }

    #[test]
    fn non_generative_term_is_absent_with_value_reported() {
        let ev = assess(jpeg_with_segments(&[app1_xmp(&xmp_attr(
            "http://cv.iptc.org/newscodes/digitalsourcetype/digitalCapture",
        ))]));
        assert_eq!(ev.status, Status::Absent, "{}", ev.rationale);
        assert!(ev.rationale.contains("digitalCapture"));
        assert!(ev.rationale.contains("non-generative"));
        assert_eq!(ev.details["digital_source_type"][0], "digitalCapture");
    }

    #[test]
    fn retired_term_is_absent_and_flagged() {
        let ev = assess(jpeg_with_segments(&[app1_xmp(&xmp_attr("digitalArt"))]));
        assert_eq!(ev.status, Status::Absent, "{}", ev.rationale);
        assert!(ev.rationale.contains("retired"));
    }

    #[test]
    fn unknown_term_is_inconclusive_verbatim() {
        let ev = assess(jpeg_with_segments(&[app1_xmp(&xmp_attr(
            "http://cv.iptc.org/newscodes/digitalsourcetype/TrainedAlgorithmicMedia",
        ))]));
        assert_eq!(ev.status, Status::Inconclusive, "{}", ev.rationale);
        assert!(ev.rationale.contains("TrainedAlgorithmicMedia"));
        assert!(ev.rationale.contains("not interpreted"));
    }

    #[test]
    fn conflicting_values_are_inconclusive() {
        let packet = format!(
            "<rdf:Description Iptc4xmpExt:DigitalSourceType=\"{GEN}\"/>\
             <rdf:Description Iptc4xmpExt:DigitalSourceType=\"digitalCapture\"/>"
        );
        let ev = assess(jpeg_with_segments(&[app1_xmp(&packet)]));
        assert_eq!(ev.status, Status::Inconclusive, "{}", ev.rationale);
        assert!(ev.rationale.contains("conflicting"));
    }

    #[test]
    fn xmp_without_field_is_absent_and_says_so() {
        let ev = assess(jpeg_with_segments(&[app1_xmp(
            "<rdf:Description xmp:CreatorTool=\"Adobe Photoshop 26.0\"/>",
        )]));
        assert_eq!(ev.status, Status::Absent, "{}", ev.rationale);
        assert!(ev.rationale.contains("XMP present but without the field"));
        assert!(ev.rationale.contains("Absence says nothing about origin"));
        assert_eq!(ev.details["xmp_packets"], 1);
    }

    #[test]
    fn no_xmp_is_absent() {
        let ev = assess(synth_jpeg([1; 64]));
        assert_eq!(ev.status, Status::Absent, "{}", ev.rationale);
        assert!(ev.rationale.contains("no XMP"));
        assert_eq!(ev.details["xmp_packets"], 0);
    }

    #[test]
    fn field_deep_in_a_large_packet_is_not_clipped() {
        // 40 KiB of padding before the field, larger than any bounded haystack.
        let pad = " ".repeat(40 * 1024);
        let packet = format!(
            "<rdf:Description xmlns:Iptc4xmpExt=\"{IPTC_EXT_NS}\">{pad}\
             <Iptc4xmpExt:DigitalSourceType>{GEN}</Iptc4xmpExt:DigitalSourceType>\
             </rdf:Description>"
        );
        let ev = assess(jpeg_with_segments(&[app1_xmp(&packet)]));
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
    }

    #[test]
    fn extended_xmp_is_reassembled_across_chunks() {
        let packet = xmp_attr(GEN);
        let bytes = packet.as_bytes();
        let guid = b"0123456789ABCDEF0123456789ABCDEF";
        let full = bytes.len() as u32;
        let mut chunks = Vec::new();
        let mut off = 0usize;
        // Deliver out of order to prove offsets are honoured.
        let mut pieces = Vec::new();
        while off < bytes.len() {
            let end = (off + 100).min(bytes.len());
            pieces.push((off, &bytes[off..end]));
            off = end;
        }
        pieces.reverse();
        for (off, data) in pieces {
            let mut p = b"http://ns.adobe.com/xmp/extension/\0".to_vec();
            p.extend_from_slice(guid);
            p.extend_from_slice(&full.to_be_bytes());
            p.extend_from_slice(&(off as u32).to_be_bytes());
            p.extend_from_slice(data);
            chunks.push((0xE1u8, p));
        }
        // Main packet carries only the pointer, as real writers do.
        let mut segs = vec![app1_xmp(
            "<rdf:Description xmpNote:HasExtendedXMP=\"0123456789ABCDEF0123456789ABCDEF\"/>",
        )];
        segs.extend(chunks);
        let ev = assess(jpeg_with_segments(&segs));
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
        assert_eq!(ev.details["xmp_extended"], true);
        assert_eq!(ev.details["xmp_extended_incomplete"], false);
        assert_eq!(ev.details["xmp_packets"], 2);
    }

    #[test]
    fn truncated_extended_xmp_is_inconclusive_not_absent() {
        let mut p = b"http://ns.adobe.com/xmp/extension/\0".to_vec();
        p.extend_from_slice(b"0123456789ABCDEF0123456789ABCDEF");
        p.extend_from_slice(&500u32.to_be_bytes());
        p.extend_from_slice(&0u32.to_be_bytes());
        p.extend_from_slice(&[b' '; 100]);
        let ev = assess(jpeg_with_segments(&[app1_xmp("<x/>"), (0xE1, p)]));
        assert_eq!(ev.status, Status::Inconclusive, "{}", ev.rationale);
        assert!(ev.rationale.contains("ExtendedXMP"));
    }

    #[test]
    fn app13_iim_block_is_reported_and_does_not_change_status() {
        // Photoshop IRB with one 8BIM 0x0404 resource holding three IIM datasets.
        let mut iim = Vec::new();
        for (ds, val) in [(0u8, &[0u8, 4][..]), (65, b"Midjourney"), (70, b"6.1")] {
            iim.push(0x1C);
            iim.push(2);
            iim.push(ds);
            iim.extend_from_slice(&(val.len() as u16).to_be_bytes());
            iim.extend_from_slice(val);
        }
        let mut irb = b"Photoshop 3.0\0".to_vec();
        irb.extend_from_slice(b"8BIM");
        irb.extend_from_slice(&0x0404u16.to_be_bytes());
        irb.extend_from_slice(&[0, 0]); // empty Pascal name, padded
        irb.extend_from_slice(&(iim.len() as u32).to_be_bytes());
        irb.extend_from_slice(&iim);
        if iim.len() % 2 == 1 {
            irb.push(0);
        }
        let ev = assess(jpeg_with_segments(&[(0xED, irb)]));
        assert_eq!(ev.status, Status::Absent, "{}", ev.rationale);
        assert!(ev.rationale.contains("IIM block is present"));
        assert_eq!(ev.details["iim"]["dataset_count"], 3);
        assert_eq!(ev.details["iim"]["originating_program"], "Midjourney");
        assert_eq!(ev.details["iim"]["program_version"], "6.1");
    }

    #[test]
    fn png_itxt_xmp_is_found() {
        let packet = xmp_attr(GEN);
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&16u32.to_be_bytes());
        ihdr.extend_from_slice(&16u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        v.extend(png_chunk(b"IHDR", &ihdr));
        let mut itxt = b"XML:com.adobe.xmp\0".to_vec();
        itxt.extend_from_slice(&[0, 0]); // uncompressed, method 0
        itxt.extend_from_slice(b"\0\0"); // empty language + translated keyword
        itxt.extend_from_slice(packet.as_bytes());
        v.extend(png_chunk(b"iTXt", &itxt));
        v.extend(png_chunk(b"IEND", &[]));
        let ev = assess(v);
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
    }

    #[test]
    fn png_with_manifest_but_no_xmp_points_to_manifest_layer() {
        // The ChatGPT PNG case: caBX (C2PA) present, no XMP at all.
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&16u32.to_be_bytes());
        ihdr.extend_from_slice(&16u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        v.extend(png_chunk(b"IHDR", &ihdr));
        v.extend(png_chunk(b"caBX", b"\0\0\0\x10jumb"));
        v.extend(png_chunk(b"IEND", &[]));
        let ev = assess(v);
        assert_eq!(ev.status, Status::Absent, "{}", ev.rationale);
        assert!(ev.rationale.contains("reported by the manifest layer"));
        assert_eq!(ev.details["manifest_container_present"], true);
    }

    #[test]
    fn webp_xmp_chunk_is_found() {
        let packet = xmp_attr(GEN);
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF\0\0\0\0WEBP");
        let mut vp8x = b"VP8X".to_vec();
        vp8x.extend_from_slice(&10u32.to_le_bytes());
        vp8x.extend_from_slice(&[0x04, 0, 0, 0, 15, 0, 0, 15, 0, 0]);
        v.extend(vp8x);
        v.extend_from_slice(b"XMP ");
        v.extend_from_slice(&(packet.len() as u32).to_le_bytes());
        v.extend_from_slice(packet.as_bytes());
        if packet.len() % 2 == 1 {
            v.push(0);
        }
        let ev = assess(v);
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
    }

    #[test]
    fn hostile_input_errors_instead_of_panicking() {
        // Segment length points past the end of the file.
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF];
        v.extend_from_slice(b"http://ns.adobe.com/xap/1.0/\0<x/>");
        let a = Asset::from_bytes(v, None).unwrap();
        assert!(MarkingMetadata.assess(&a).is_err());
        // 8BIM walker with garbage after a valid signature.
        assert!(parse_irb_iptc(b"8BIM\x04\x04\x00\xFF\xFF").is_none());
        // Empty and truncated IIM.
        assert_eq!(parse_iim(&[]).dataset_count, 0);
        assert_eq!(parse_iim(&[0x1C, 2, 65, 0x00, 0x10, b'x']).dataset_count, 0);
    }

    fn png_chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = (data.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(ty);
        v.extend_from_slice(data);
        v.extend_from_slice(&[0, 0, 0, 0]);
        v
    }
}
