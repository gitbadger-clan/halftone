//! The metadata a file carries in its own fields, read once for the sources that
//! list such fields (`rights_statements`, `embedded_identifiers`, `vendor_ai_flag`):
//!
//! - XMP, from the same packets `marking_metadata` reads ([`crate::marking::packets`]);
//! - EXIF, through `kamadak-exif` (JPEG, PNG `eXIf`, WebP, HEIF);
//! - IPTC IIM record 2, from the Photoshop image-resource block of a JPEG;
//! - PNG text chunks (`tEXt`, uncompressed `iTXt`).
//!
//! Reading only: nothing here judges a value. Every value is returned as written,
//! trimmed and bounded. Not read, and the sources say so: EXIF MakerNotes, values
//! inside a C2PA manifest, compressed PNG text (`zTXt`), and XMP or IIM in
//! containers that keep them somewhere this module does not look.

use std::io::Cursor;

use exif::{In, Tag};
use halftone_core::Asset;

use crate::marking::{Form, find_property, packets, prefix_bound};

/// Longest value kept, in characters. A longer one is cut and ends in `…`.
pub const MAX_VALUE: usize = 256;

/// One XMP property occurrence with the namespace its prefix is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmpValue {
    /// Prefix as written.
    pub prefix: String,
    /// Namespace URI the prefix is bound to in the packet, when declared.
    pub namespace: Option<String>,
    /// The value, trimmed and bounded.
    pub value: String,
    /// Where the value sat.
    pub form: Form,
}

/// Everything read from one asset.
#[derive(Default)]
pub struct Embedded {
    /// XMP packets as text, in file order.
    pub xmp: Vec<String>,
    /// JPEG only: ExtendedXMP was present but could not be fully reassembled.
    pub xmp_extended_incomplete: bool,
    /// HEIF only: XMP items whose bytes could not be read.
    pub xmp_items_unreadable: Vec<String>,
    /// A C2PA manifest container is present (not read here).
    pub manifest_container_present: bool,
    /// EXIF was found and parsed (possibly partially).
    pub has_exif: bool,
    exif: Option<exif::Exif>,
    /// IIM record-2 datasets: (dataset number, text), in file order.
    pub iim: Vec<(u8, String)>,
    /// PNG text chunks: (keyword, text), in file order.
    pub png_text: Vec<(String, String)>,
}

impl std::fmt::Debug for Embedded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Embedded")
            .field("xmp_packets", &self.xmp.len())
            .field("has_exif", &self.has_exif)
            .field("iim", &self.iim)
            .field("png_text", &self.png_text)
            .finish_non_exhaustive()
    }
}

/// Trim, collapse inner whitespace runs, and bound a value.
pub fn tidy(s: &str) -> String {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > MAX_VALUE {
        let mut cut: String = t.chars().take(MAX_VALUE).collect();
        cut.push('…');
        cut
    } else {
        t
    }
}

/// The namespace URI bound to `prefix` in an XMP packet (`xmlns:prefix="…"`).
pub fn namespace_of(text: &str, prefix: &str) -> Option<String> {
    let key = format!("xmlns:{prefix}=");
    let at = text.find(&key)? + key.len();
    let rest = &text[at..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let end = rest[1..].find(quote)?;
    Some(rest[1..1 + end].to_string())
}

impl Embedded {
    /// Read an asset. `Err` only when the container itself cannot be walked; a
    /// format this module does not know yields an empty result.
    pub fn read(a: &Asset) -> Result<Self, String> {
        let mut e = Embedded::default();
        if matches!(
            a.mime.as_str(),
            "image/jpeg" | "image/png" | "image/webp" | "image/heif"
        ) {
            let (pk, scan) = packets(&a.mime, &a.bytes)?;
            e.xmp = pk
                .iter()
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .collect();
            e.xmp_extended_incomplete = scan.xmp_extended_incomplete;
            e.xmp_items_unreadable = scan.xmp_items_unreadable;
            e.manifest_container_present = scan.manifest_container_present;
        }
        let mut reader = exif::Reader::new();
        reader.continue_on_error(true);
        e.exif = match reader.read_from_container(&mut Cursor::new(a.bytes.as_slice())) {
            Ok(x) => Some(x),
            Err(exif::Error::PartialResult(pr)) => Some(pr.into_inner().0),
            Err(_) => None,
        };
        e.has_exif = e.exif.is_some();
        match a.mime.as_str() {
            "image/jpeg" => {
                if let Ok(m) = halftone_formats::jpeg::metadata(&a.bytes) {
                    // A later block with IIM replaces an earlier one.
                    for irb in &m.photoshop_irb {
                        if let Some(sets) = irb_iim(irb) {
                            e.iim = sets;
                        }
                    }
                }
            }
            "image/png" => e.png_text = png_text(&a.bytes),
            _ => {}
        }
        Ok(e)
    }

    /// True when some XMP could not be read completely, so "not found" is not
    /// "not there".
    pub fn xmp_incomplete(&self) -> bool {
        self.xmp_extended_incomplete || !self.xmp_items_unreadable.is_empty()
    }

    /// Every occurrence of the XMP property `name` whose prefix is bound to `ns`, or
    /// is the conventional `prefix` when the packet binds it to nothing else known.
    /// `nested` reads values held in child elements (language alternatives, bags).
    pub fn xmp_values(&self, ns: &str, prefix: &str, name: &str, nested: bool) -> Vec<XmpValue> {
        let mut out = Vec::new();
        for text in &self.xmp {
            for v in find_property(text, name, nested) {
                if prefix_bound(text, &v.prefix, ns) || v.prefix == prefix {
                    out.push(XmpValue {
                        namespace: namespace_of(text, &v.prefix),
                        prefix: v.prefix,
                        value: tidy(&v.raw),
                        form: v.form,
                    });
                }
            }
        }
        out
    }

    /// Every occurrence of the XMP property `name` under any prefix.
    pub fn xmp_any(&self, name: &str) -> Vec<XmpValue> {
        let mut out = Vec::new();
        for text in &self.xmp {
            for v in find_property(text, name, false) {
                out.push(XmpValue {
                    namespace: namespace_of(text, &v.prefix),
                    prefix: v.prefix,
                    value: tidy(&v.raw),
                    form: v.form,
                });
            }
        }
        out
    }

    /// An EXIF text field of the primary image, trimmed; `None` when absent or empty.
    pub fn exif_text(&self, tag: Tag) -> Option<String> {
        let f = self.exif.as_ref()?.get_field(tag, In::PRIMARY)?;
        let s = f.display_value().to_string();
        let t = tidy(s.trim().trim_matches('"'));
        (!t.is_empty()).then_some(t)
    }

    /// The GPS position as EXIF states it (`lat ref, lon ref`), when both are present.
    pub fn exif_gps(&self) -> Option<String> {
        let x = self.exif.as_ref()?;
        let get = |t: Tag| {
            x.get_field(t, In::PRIMARY)
                .map(|f| f.display_value().to_string())
        };
        let lat = get(Tag::GPSLatitude)?;
        let lon = get(Tag::GPSLongitude)?;
        let (lr, nr) = (
            get(Tag::GPSLatitudeRef).unwrap_or_default(),
            get(Tag::GPSLongitudeRef).unwrap_or_default(),
        );
        Some(tidy(&format!("{lat} {lr}, {lon} {nr}")))
    }

    /// Every non-empty value of IIM dataset `2:<dataset>`, in file order.
    pub fn iim_values(&self, dataset: u8) -> Vec<String> {
        self.iim
            .iter()
            .filter(|(d, v)| *d == dataset && !v.is_empty())
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// Every non-empty PNG text chunk with this keyword (ASCII case-insensitive).
    pub fn png_values(&self, keyword: &str) -> Vec<String> {
        self.png_text
            .iter()
            .filter(|(k, v)| k.eq_ignore_ascii_case(keyword) && !v.is_empty())
            .map(|(_, v)| v.clone())
            .collect()
    }
}

/// The listing behind `ht inspect --metadata`: every field the listing sources found
/// (`rights_statements`, `embedded_identifiers`, `vendor_ai_flag`,
/// `data_mining_reservation`), one per line with its value as written. Reads the
/// sources' `details`; adds no judgement. Empty when none of them ran.
pub fn render_listing(evidence: &[halftone_core::Evidence]) -> String {
    let details = |name: &str| {
        evidence
            .iter()
            .find(|e| e.source.name == name)
            .map(|e| &e.details)
    };
    let text = |v: &serde_json::Value, key: &str| v[key].as_str().unwrap_or("").to_string();
    let mut sections: Vec<(&str, Vec<(String, String)>)> = Vec::new();
    let mut ran = false;
    let mut section = |title: &'static str,
                       source: &str,
                       list: &str,
                       row: &dyn Fn(&serde_json::Value) -> (String, String)| {
        if let Some(d) = details(source) {
            ran = true;
            let rows: Vec<(String, String)> =
                d[list].as_array().into_iter().flatten().map(row).collect();
            if !rows.is_empty() {
                sections.push((title, rows));
            }
        }
    };
    section(
        "rights statements",
        "rights_statements",
        "statements",
        &|s| (text(s, "field"), text(s, "value")),
    );
    section("identifiers", "embedded_identifiers", "identifiers", &|i| {
        (
            format!(
                "{} ({})",
                text(i, "field"),
                text(i, "kind").replace('_', " ")
            ),
            text(i, "value"),
        )
    });
    section("vendor AI flags", "vendor_ai_flag", "flags", &|f| {
        (
            format!("{}:{}", text(f, "prefix"), text(f, "field")),
            text(f, "value"),
        )
    });
    section("data mining", "data_mining_reservation", "values", &|v| {
        (format!("{}:DataMining", text(v, "prefix")), text(v, "code"))
    });
    if !ran {
        return String::new();
    }
    let mut out = String::from("  metadata fields (as written; unsigned)\n");
    if sections.is_empty() {
        out.push_str("    none of the listed fields is present\n");
    }
    for (title, rows) in sections {
        out.push_str(&format!("    {title}\n"));
        let width = rows.iter().map(|r| r.0.chars().count()).max().unwrap_or(0);
        for (field, value) in rows {
            out.push_str(&format!("      {field:<width$}  {value}\n"));
        }
    }
    out
}

/// IIM text: UTF-8 when valid, otherwise Latin-1, the two encodings writers use.
fn iim_text(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => tidy(s),
        Err(_) => tidy(&b.iter().map(|&c| c as char).collect::<String>()),
    }
}

/// Record-2 datasets from a Photoshop image-resource block (resource `0x0404`).
/// `None` when the block holds no IIM resource.
fn irb_iim(b: &[u8]) -> Option<Vec<(u8, String)>> {
    let mut i = 0;
    while i + 12 <= b.len() {
        if &b[i..i + 4] != b"8BIM" {
            return None;
        }
        let id = u16::from_be_bytes([b[i + 4], b[i + 5]]);
        // Pascal-string name, padded to an even length including its length byte.
        let name_len = b[i + 6] as usize;
        let mut p = i + 7 + name_len;
        if (1 + name_len) % 2 == 1 {
            p += 1;
        }
        let size = u32::from_be_bytes(b.get(p..p + 4)?.try_into().ok()?) as usize;
        let start = p + 4;
        let end = start.checked_add(size).filter(|&e| e <= b.len())?;
        if id == 0x0404 {
            return Some(iim_datasets(&b[start..end]));
        }
        i = end + (size % 2);
    }
    None
}

/// Decode IIM datasets `0x1C record dataset size(u16) data`; record 2 only, extended
/// sizes end the walk.
fn iim_datasets(b: &[u8]) -> Vec<(u8, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 5 <= b.len() && b[i] == 0x1C {
        let (record, dataset) = (b[i + 1], b[i + 2]);
        let size = u16::from_be_bytes([b[i + 3], b[i + 4]]);
        if size & 0x8000 != 0 {
            break;
        }
        let start = i + 5;
        let end = start + size as usize;
        if end > b.len() {
            break;
        }
        if record == 2 {
            out.push((dataset, iim_text(&b[start..end])));
        }
        i = end;
    }
    out
}

/// `tEXt` and uncompressed `iTXt` chunks as (keyword, text). Other chunks, compressed
/// text and anything malformed are skipped.
fn png_text(b: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 8;
    while i + 12 <= b.len() {
        let len = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let ty = &b[i + 4..i + 8];
        let start = i + 8;
        let Some(end) = start.checked_add(len).filter(|&e| e + 4 <= b.len()) else {
            break;
        };
        let data = &b[start..end];
        if let Some(nul) = data.iter().position(|&c| c == 0) {
            let keyword = String::from_utf8_lossy(&data[..nul]).into_owned();
            let rest = &data[nul + 1..];
            if ty == b"tEXt" {
                out.push((keyword, iim_text(rest)));
            } else if ty == b"iTXt" && rest.first() == Some(&0) && rest.len() >= 2 {
                // compression flag 0, method, language tag NUL, translated keyword NUL.
                let after = &rest[2..];
                let mut parts = after.splitn(3, |&c| c == 0);
                if let (Some(_lang), Some(_tk), Some(text)) =
                    (parts.next(), parts.next(), parts.next())
                {
                    if keyword != "XML:com.adobe.xmp" {
                        out.push((keyword, tidy(&String::from_utf8_lossy(text))));
                    }
                }
            }
        }
        if ty == b"IEND" {
            break;
        }
        i = end + 4;
    }
    out
}

#[cfg(test)]
pub(crate) mod testutil {
    //! Builders for the sources' tests: a JPEG with chosen XMP, EXIF text fields and
    //! IIM datasets; a PNG with text chunks.

    use crate::jpeg::testutil::synth_jpeg;

    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut s = vec![0xFF, marker];
        s.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        s.extend_from_slice(payload);
        s
    }

    /// A little-endian EXIF block with ASCII tags in IFD0 (`ifd0`) and in the Exif
    /// sub-IFD (`sub`).
    pub fn exif(ifd0: &[(u16, &str)], sub: &[(u16, &str)]) -> Vec<u8> {
        fn ifd(entries: &[(u16, u16, u32, Vec<u8>)], at: u32, next: u32) -> Vec<u8> {
            // entries: (tag, type, count, data). Data over 4 bytes goes after the IFD.
            let mut dir = (entries.len() as u16).to_le_bytes().to_vec();
            let mut extra: Vec<u8> = Vec::new();
            let data_at = at + 2 + 12 * entries.len() as u32 + 4;
            for (tag, ty, count, data) in entries {
                dir.extend_from_slice(&tag.to_le_bytes());
                dir.extend_from_slice(&ty.to_le_bytes());
                dir.extend_from_slice(&count.to_le_bytes());
                if data.len() <= 4 {
                    let mut v = data.clone();
                    v.resize(4, 0);
                    dir.extend_from_slice(&v);
                } else {
                    dir.extend_from_slice(&(data_at + extra.len() as u32).to_le_bytes());
                    extra.extend_from_slice(data);
                    if extra.len() % 2 == 1 {
                        extra.push(0);
                    }
                }
            }
            dir.extend_from_slice(&next.to_le_bytes());
            dir.extend(extra);
            dir
        }
        let ascii = |list: &[(u16, &str)]| -> Vec<(u16, u16, u32, Vec<u8>)> {
            list.iter()
                .map(|(t, s)| {
                    let mut d = s.as_bytes().to_vec();
                    d.push(0);
                    (*t, 2u16, d.len() as u32, d)
                })
                .collect()
        };
        let mut e0 = ascii(ifd0);
        let sub_entries = ascii(sub);
        // Size IFD0 first with a placeholder pointer to learn where the sub-IFD goes.
        if !sub_entries.is_empty() {
            e0.push((0x8769, 4, 1, 0u32.to_le_bytes().to_vec()));
        }
        e0.sort_by_key(|e| e.0);
        let first = ifd(&e0, 8, 0);
        let sub_at = 8 + first.len() as u32;
        if let Some(p) = e0.iter_mut().find(|e| e.0 == 0x8769) {
            p.3 = sub_at.to_le_bytes().to_vec();
        }
        let mut tiff = b"II\x2A\x00\x08\x00\x00\x00".to_vec();
        tiff.extend(ifd(&e0, 8, 0));
        if !sub_entries.is_empty() {
            tiff.extend(ifd(&sub_entries, sub_at, 0));
        }
        let mut out = b"Exif\0\0".to_vec();
        out.extend(tiff);
        out
    }

    /// A JPEG carrying the given XMP packet, EXIF block and IIM record-2 datasets.
    pub fn jpeg(xmp: Option<&str>, exif: Option<&[u8]>, iim: &[(u8, &[u8])]) -> Vec<u8> {
        let base = synth_jpeg([1; 64]);
        let mut v = vec![0xFF, 0xD8];
        if let Some(e) = exif {
            v.extend(segment(0xE1, e));
        }
        if let Some(x) = xmp {
            let mut p = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
            p.extend_from_slice(x.as_bytes());
            v.extend(segment(0xE1, &p));
        }
        if !iim.is_empty() {
            let mut sets = Vec::new();
            for (ds, val) in iim {
                sets.extend_from_slice(&[0x1C, 2, *ds]);
                sets.extend_from_slice(&(val.len() as u16).to_be_bytes());
                sets.extend_from_slice(val);
            }
            let mut irb = b"Photoshop 3.0\08BIM\x04\x04\0\0".to_vec();
            irb.extend_from_slice(&(sets.len() as u32).to_be_bytes());
            irb.extend_from_slice(&sets);
            if sets.len() % 2 == 1 {
                irb.push(0);
            }
            v.extend(segment(0xED, &irb));
        }
        v.extend_from_slice(&base[2..]);
        v
    }

    /// An XMP packet with the given namespace declarations and property lines.
    pub fn packet(xmlns: &str, properties: &str) -> String {
        format!(
            "<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF \
             xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\
             <rdf:Description rdf:about='' {xmlns}>{properties}</rdf:Description>\
             </rdf:RDF></x:xmpmeta>"
        )
    }

    /// A PNG with the given `tEXt` chunks.
    pub fn png(text: &[(&str, &str)]) -> Vec<u8> {
        fn chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut v = (data.len() as u32).to_be_bytes().to_vec();
            v.extend_from_slice(ty);
            v.extend_from_slice(data);
            v.extend_from_slice(&[0; 4]);
            v
        }
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        out.extend(chunk(b"IHDR", &[0; 13]));
        for (k, v) in text {
            let mut d = k.as_bytes().to_vec();
            d.push(0);
            d.extend_from_slice(v.as_bytes());
            out.extend(chunk(b"tEXt", &d));
        }
        out.extend(chunk(b"IEND", &[]));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    fn read(bytes: Vec<u8>) -> Embedded {
        Embedded::read(&Asset::from_bytes(bytes, None).unwrap()).unwrap()
    }

    #[test]
    fn reads_all_four_places() {
        let x = packet(
            "xmlns:dc='http://purl.org/dc/elements/1.1/' xmlns:q='http://ns.adobe.com/photoshop/1.0/'",
            "<dc:rights><rdf:Alt><rdf:li xml:lang='x-default'>(c)  A.  Person</rdf:li></rdf:Alt></dc:rights>\
             <q:Credit>Agency</q:Credit>",
        );
        let e = read(jpeg(
            Some(&x),
            Some(&exif(
                &[(0x013B, "A. Person"), (0x8298, "(c) A. Person")],
                &[(0xA431, "SN123")],
            )),
            &[
                (116, b"(c) A. Person"),
                (80, b"A. Person"),
                (80, b"B. Person"),
            ],
        ));
        let rights = e.xmp_values("http://purl.org/dc/elements/1.1/", "dc", "rights", true);
        assert_eq!(
            rights[0].value, "(c) A. Person",
            "language alternative, whitespace tidied"
        );
        assert_eq!(
            rights[0].namespace.as_deref(),
            Some("http://purl.org/dc/elements/1.1/")
        );
        // Found through the namespace although the prefix is not the usual one.
        let credit = e.xmp_values(
            "http://ns.adobe.com/photoshop/1.0/",
            "photoshop",
            "Credit",
            false,
        );
        assert_eq!(
            (credit[0].prefix.as_str(), credit[0].value.as_str()),
            ("q", "Agency")
        );
        assert_eq!(e.exif_text(Tag::Artist).as_deref(), Some("A. Person"));
        assert_eq!(
            e.exif_text(Tag::Copyright).as_deref(),
            Some("(c) A. Person")
        );
        assert_eq!(e.exif_text(Tag::BodySerialNumber).as_deref(), Some("SN123"));
        assert_eq!(e.exif_text(Tag::CameraOwnerName), None);
        assert_eq!(e.iim_values(116), ["(c) A. Person"]);
        assert_eq!(e.iim_values(80), ["A. Person", "B. Person"]);
        assert!(e.exif_gps().is_none());

        let p = read(png(&[
            ("Copyright", "(c) Someone"),
            ("Author", "Someone"),
            ("author", ""),
        ]));
        assert_eq!(p.png_values("copyright"), ["(c) Someone"]);
        assert_eq!(p.png_values("Author"), ["Someone"]);
    }

    #[test]
    fn values_are_bounded_and_latin1_iim_is_decoded() {
        let long = "x".repeat(MAX_VALUE + 50);
        let e = read(jpeg(
            None,
            None,
            &[(116, long.as_bytes()), (110, b"Caf\xE9 Press")],
        ));
        let v = &e.iim_values(116)[0];
        assert_eq!(v.chars().count(), MAX_VALUE + 1);
        assert!(v.ends_with('…'));
        assert_eq!(e.iim_values(110), ["Café Press"]);
    }

    #[test]
    fn the_listing_groups_fields_by_source_and_quotes_values() {
        use halftone_core::EvidenceSource;
        let x = packet(
            "xmlns:plus='http://ns.useplus.org/ldf/xmp/1.0/' xmlns:pdf='http://ns.adobe.com/pdf/1.3/' \
             xmlns:v='urn:vendor'",
            "<plus:DataMining>http://ns.useplus.org/ldf/vocab/DMI-PROHIBITED</plus:DataMining>\
             <pdf:Author>Some Person</pdf:Author><v:ContainsAiGeneratedContent>Yes</v:ContainsAiGeneratedContent>",
        );
        let a = Asset::from_bytes(
            jpeg(Some(&x), Some(&exif(&[(0x8298, "(c) Someone")], &[])), &[]),
            None,
        )
        .unwrap();
        let ev = vec![
            crate::rights::RightsStatements.assess(&a).unwrap(),
            crate::identifiers::EmbeddedIdentifiers.assess(&a).unwrap(),
            crate::vendor_ai::VendorAiFlag.assess(&a).unwrap(),
            crate::data_mining::DataMiningReservation
                .assess(&a)
                .unwrap(),
        ];
        let out = render_listing(&ev);
        let expected = "  metadata fields (as written; unsigned)\n    rights statements\n      \
            exif:Copyright  (c) Someone\n    identifiers\n      pdf:Author (person)  Some Person\n    \
            vendor AI flags\n      v:ContainsAiGeneratedContent  Yes\n    data mining\n      \
            plus:DataMining  DMI-PROHIBITED\n";
        assert_eq!(out, expected);

        let plain = Asset::from_bytes(jpeg(None, None, &[]), None).unwrap();
        let ev = vec![crate::rights::RightsStatements.assess(&plain).unwrap()];
        assert!(render_listing(&ev).contains("none of the listed fields is present"));
        assert_eq!(
            render_listing(&[]),
            "",
            "no listing source ran: say nothing"
        );
    }

    #[test]
    fn namespace_of_reads_either_quote() {
        let t = "<a xmlns:p='urn:one' xmlns:q=\"urn:two\">";
        assert_eq!(namespace_of(t, "p").as_deref(), Some("urn:one"));
        assert_eq!(namespace_of(t, "q").as_deref(), Some("urn:two"));
        assert_eq!(namespace_of(t, "r"), None);
    }

    #[test]
    fn hostile_input_never_panics() {
        let ok = jpeg(
            Some(&packet("", "<a:b>c</a:b>")),
            Some(&exif(&[(0x8298, "c")], &[(0xA430, "o")])),
            &[(116, b"c")],
        );
        for n in 0..ok.len() {
            if let Ok(a) = Asset::from_bytes(ok[..n].to_vec(), None) {
                let _ = Embedded::read(&a);
            }
        }
        let p = png(&[("Copyright", "c")]);
        for n in 0..p.len() {
            let _ = png_text(&p[..n]);
        }
        assert!(irb_iim(b"8BIM\x04\x04\xFF").is_none());
    }
}
