//! Embedded identifiers: the privacy listing. What a file tells anyone who receives
//! it about a person, an account, a device, a document or a place.
//!
//! One source, one question: does the file's metadata carry identifiers? It lists
//! them from a fixed set of fields ([`FIELDS`] documents every one) in XMP, EXIF, IPTC
//! IIM and PNG text chunks, quoted as written. Nothing is judged: a name may be a
//! pseudonym, an identifier may be stale. The point is what the file discloses.
//! Placeholders (`N/A`, `Unknown`, all zeros) and GPS tags without a position are
//! not identifiers and are left out.
//!
//! Values are masked unless the source is built with `reveal` (`ht inspect --reveal`):
//! `details` then names each field and kind and gives the value's length, not the
//! value. The listing exists to show what a file discloses, and a report that
//! repeated every name and account number would disclose them a second time. The
//! status and the counts in the rationale are the same either way.
//!
//! Status semantics:
//! - `Present`: at least one identifier is there.
//! - `Absent`: none of the listed fields is there. Not a statement that the file is
//!   free of identifying data: see the blind spots in the rationale.
//! - `Inconclusive`: nothing found, but some metadata could not be read completely.
//!
//! Not read, and named in the `Absent` rationale: EXIF MakerNotes (camera makers'
//! own blocks, which often hold serial numbers and owner data), the signer and
//! assertions of a C2PA manifest, embedded thumbnails and previews, identifiers
//! hidden in the pixels, and any field outside the list.

use exif::Tag;
use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
use serde::Serialize;

use crate::embedded::{Embedded, is_placeholder};

const DC: &str = "http://purl.org/dc/elements/1.1/";
const PDF: &str = "http://ns.adobe.com/pdf/1.3/";
const XMP_MM: &str = "http://ns.adobe.com/xap/1.0/mm/";
const AUX: &str = "http://ns.adobe.com/exif/1.0/aux/";
const EXIF_EX: &str = "http://cipa.jp/exif/1.0/";
const IPTC_CORE: &str = "http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/";
const PHOTOSHOP: &str = "http://ns.adobe.com/photoshop/1.0/";
const ADS: &str = "http://ns.attribution.com/ads/1.0/";
/// Start of the marker Meta's platforms write into the instructions field.
const PLATFORM_MARKER: &str = "FBMD";

/// What an identifier identifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A person's name.
    Person,
    /// A way to reach a person: e-mail, phone, address, web page.
    Contact,
    /// An account or tracking record written by a platform or application.
    Account,
    /// A device: a serial number, a named owner.
    Device,
    /// A document identifier that links files from the same source or edit history.
    Document,
    /// A place: geographic coordinates.
    Location,
}

impl Kind {
    fn noun(self, n: usize) -> String {
        let (one, many) = match self {
            Kind::Person => ("person name", "person names"),
            Kind::Contact => ("contact detail", "contact details"),
            Kind::Account => ("account or tracking record", "account or tracking records"),
            Kind::Device => ("device identifier", "device identifiers"),
            Kind::Document => ("document identifier", "document identifiers"),
            Kind::Location => ("location", "locations"),
        };
        format!("{n} {}", if n == 1 { one } else { many })
    }
}

/// The fields this source reads, for documentation and the `Absent` rationale.
pub const FIELDS: &[(Kind, &str)] = &[
    (
        Kind::Person,
        "EXIF Artist, IPTC By-line, dc:creator, pdf:Author, PNG Author",
    ),
    (
        Kind::Contact,
        "IPTC creator contact info (e-mail, phone, address, web)",
    ),
    (
        Kind::Account,
        "Attrib:Ads records, the FBMD platform marker in the instructions field",
    ),
    (
        Kind::Device,
        "EXIF body and lens serial numbers, camera owner name, and their XMP copies",
    ),
    (
        Kind::Document,
        "xmpMM DocumentID, OriginalDocumentID, InstanceID",
    ),
    (Kind::Location, "EXIF GPS latitude and longitude"),
];

/// One identifier found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identifier {
    /// What it identifies.
    pub kind: Kind,
    /// The field it was read from (`exif:Artist`, `iim:By-line`, `dc:creator`, …).
    pub field: String,
    /// The value as written, bounded.
    pub value: String,
}

/// List every identifier in the asset, in the order of [`FIELDS`].
pub fn scan(e: &Embedded) -> Vec<Identifier> {
    let mut out: Vec<Identifier> = Vec::new();
    let mut add = |kind: Kind, field: &str, value: String| {
        // "N/A", "Unknown", all zeros: a field with no value identifies nobody.
        if is_placeholder(&value) {
            return;
        }
        let id = Identifier {
            kind,
            field: field.to_string(),
            value,
        };
        if !out.contains(&id) {
            out.push(id);
        }
    };
    let xmp = |ns: &str, prefix: &str, name: &str, nested: bool| -> Vec<String> {
        e.xmp_values(ns, prefix, name, nested)
            .into_iter()
            .map(|v| v.value)
            .collect()
    };

    // Persons.
    if let Some(v) = e.exif_text(Tag::Artist) {
        add(Kind::Person, "exif:Artist", v);
    }
    for v in e.iim_values(80) {
        add(Kind::Person, "iim:By-line", v);
    }
    for v in xmp(DC, "dc", "creator", true) {
        add(Kind::Person, "dc:creator", v);
    }
    for v in xmp(PDF, "pdf", "Author", false) {
        add(Kind::Person, "pdf:Author", v);
    }
    for v in e.png_values("Author") {
        add(Kind::Person, "png:Author", v);
    }

    // Contact details (the IPTC creator contact info structure).
    for name in [
        "CiEmailWork",
        "CiTelWork",
        "CiUrlWork",
        "CiAdrExtadr",
        "CiAdrCity",
        "CiAdrPcode",
        "CiAdrRegion",
        "CiAdrCtry",
    ] {
        for v in xmp(IPTC_CORE, "Iptc4xmpCore", name, false) {
            add(Kind::Contact, &format!("Iptc4xmpCore:{name}"), v);
        }
    }

    // Account and tracking records.
    for name in ["ExtId", "FbId", "Data", "TouchType", "Created"] {
        for v in xmp(ADS, "Attrib", name, false) {
            add(Kind::Account, &format!("Attrib:{name}"), v);
        }
    }
    for v in e.iim_values(40) {
        if v.starts_with(PLATFORM_MARKER) {
            add(Kind::Account, "iim:SpecialInstructions", v);
        }
    }
    for v in xmp(PHOTOSHOP, "photoshop", "Instructions", false) {
        if v.starts_with(PLATFORM_MARKER) {
            add(Kind::Account, "photoshop:Instructions", v);
        }
    }

    // Devices.
    for (tag, field) in [
        (Tag::BodySerialNumber, "exif:BodySerialNumber"),
        (Tag::LensSerialNumber, "exif:LensSerialNumber"),
        (Tag::CameraOwnerName, "exif:CameraOwnerName"),
    ] {
        if let Some(v) = e.exif_text(tag) {
            add(Kind::Device, field, v);
        }
    }
    for (ns, prefix, name) in [
        (AUX, "aux", "SerialNumber"),
        (AUX, "aux", "OwnerName"),
        (EXIF_EX, "exifEX", "BodySerialNumber"),
        (EXIF_EX, "exifEX", "LensSerialNumber"),
        (EXIF_EX, "exifEX", "CameraOwnerName"),
    ] {
        for v in xmp(ns, prefix, name, false) {
            add(Kind::Device, &format!("{prefix}:{name}"), v);
        }
    }

    // Documents.
    for name in ["DocumentID", "OriginalDocumentID", "InstanceID"] {
        for v in xmp(XMP_MM, "xmpMM", name, false) {
            add(Kind::Document, &format!("xmpMM:{name}"), v);
        }
    }

    // Location.
    if let Some(v) = e.exif_gps() {
        add(Kind::Location, "exif:GPS", v);
    }
    out
}

/// Embedded identifiers evidence source (`embedded_identifiers`).
#[derive(Debug, Default)]
pub struct EmbeddedIdentifiers {
    /// Put the values themselves into `details`. Off by default: fields, kinds and
    /// lengths only.
    pub reveal: bool,
}

/// How an identifier appears in `details`: with its value, or masked.
fn shown(i: &Identifier, reveal: bool) -> serde_json::Value {
    if reveal {
        serde_json::json!({ "kind": i.kind, "field": i.field, "value": i.value })
    } else {
        serde_json::json!({
            "kind": i.kind,
            "field": i.field,
            "masked": true,
            "length": i.value.chars().count(),
        })
    }
}

impl EvidenceSource for EmbeddedIdentifiers {
    fn id(&self) -> SourceId {
        SourceId {
            name: "embedded_identifiers".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
    fn layer(&self) -> Layer {
        Layer::Container
    }
    fn supports(&self, a: &Asset) -> bool {
        a.modality == Modality::Image
            && matches!(
                a.mime.as_str(),
                "image/jpeg" | "image/png" | "image/webp" | "image/heif"
            )
    }
    fn assess(&self, a: &Asset) -> halftone_core::Result<Evidence> {
        let e = Embedded::read(a).map_err(halftone_core::Error::Parse)?;
        let found = scan(&e);

        const BLIND: &str = "Not read: camera makers' own EXIF blocks (MakerNotes), the signer \
            and contents of a C2PA manifest, embedded thumbnails and previews, anything hidden \
            in the pixels, and fields outside this source's list.";
        let (status, rationale) = if found.is_empty() {
            let unread = e.xmp_incomplete();
            (
                if unread {
                    Status::Inconclusive
                } else {
                    Status::Absent
                },
                format!(
                    "None of the listed identifier fields is present{}. This is not a statement \
                     that the file identifies nobody. {BLIND}",
                    if unread {
                        " in what could be read; some XMP could not be read"
                    } else {
                        ""
                    }
                ),
            )
        } else {
            let mut kinds: Vec<Kind> = found.iter().map(|i| i.kind).collect();
            kinds.sort();
            kinds.dedup();
            let summary: Vec<String> = kinds
                .iter()
                .map(|k| k.noun(found.iter().filter(|i| i.kind == *k).count()))
                .collect();
            (
                Status::Present,
                format!(
                    "The file's metadata carries {}. Anyone who receives the file can read \
                     them; details lists each one with its field{}. They are read as written: \
                     this says what the file discloses, not that any of it is accurate or who \
                     put it there. {BLIND}",
                    join_and(&summary),
                    if self.reveal {
                        " and value"
                    } else {
                        ", with the values masked here (--reveal includes them)"
                    }
                ),
            )
        };
        Ok(Evidence {
            layer: self.layer(),
            source: self.id(),
            status,
            statistic: None,
            calibration: None,
            rationale,
            details: serde_json::json!({
                "identifiers": found.iter().map(|i| shown(i, self.reveal)).collect::<Vec<_>>(),
                "values_masked": !self.reveal,
                "xmp_packets": e.xmp.len(),
                "xmp_incomplete": e.xmp_incomplete(),
                "has_exif": e.has_exif,
            }),
            duration_ms: 0,
        })
    }
}

/// "a, b and c".
pub(crate) fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedded::testutil::{exif, jpeg, packet, png};

    /// Revealed, so the tests can check what was read.
    fn assess(bytes: Vec<u8>) -> Evidence {
        let a = Asset::from_bytes(bytes, None).unwrap();
        EmbeddedIdentifiers { reveal: true }.assess(&a).unwrap()
    }

    fn fields(ev: &Evidence) -> Vec<(String, String)> {
        ev.details["identifiers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                (
                    i["field"].as_str().unwrap().to_string(),
                    i["value"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn a_design_tool_export_discloses_account_person_and_document() {
        // The shape measured in the corpus: an ads record with a data blob, an
        // author name in the PDF namespace, document identifiers.
        let x = packet(
            "xmlns:Attrib='http://ns.attribution.com/ads/1.0/' xmlns:pdf='http://ns.adobe.com/pdf/1.3/' \
             xmlns:xmpMM='http://ns.adobe.com/xap/1.0/mm/'",
            "<Attrib:Ads><rdf:Seq><rdf:li rdf:parseType='Resource'>\
             <Attrib:Created>2026-09-20</Attrib:Created>\
             <Attrib:Data>{\"doc\":\"DAGdoc\",\"user\":\"UAGuser\"}</Attrib:Data>\
             <Attrib:ExtId>ext-1</Attrib:ExtId><Attrib:FbId>525265914179580</Attrib:FbId>\
             <Attrib:TouchType>2</Attrib:TouchType></rdf:li></rdf:Seq></Attrib:Ads>\
             <pdf:Author>Some Person</pdf:Author><xmpMM:DocumentID>xmp.did:1234</xmpMM:DocumentID>",
        );
        let ev = assess(jpeg(Some(&x), None, &[]));
        assert_eq!(ev.status, Status::Present);
        let got = fields(&ev);
        assert!(got.contains(&("pdf:Author".into(), "Some Person".into())));
        assert!(got.contains(&(
            "Attrib:Data".into(),
            "{\"doc\":\"DAGdoc\",\"user\":\"UAGuser\"}".into()
        )));
        assert!(got.contains(&("Attrib:FbId".into(), "525265914179580".into())));
        assert!(got.contains(&("xmpMM:DocumentID".into(), "xmp.did:1234".into())));
        assert!(
            ev.rationale
                .contains("1 person name, 5 account or tracking records and 1 document identifier"),
            "{}",
            ev.rationale
        );
        assert!(
            ev.rationale
                .contains("Anyone who receives the file can read them")
        );
        assert!(ev.rationale.contains("MakerNotes"));
    }

    #[test]
    fn a_camera_file_discloses_person_and_device() {
        let x = packet(
            "xmlns:dc='http://purl.org/dc/elements/1.1/' \
             xmlns:Iptc4xmpCore='http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/'",
            "<dc:creator><rdf:Seq><rdf:li>A. Person</rdf:li></rdf:Seq></dc:creator>\
             <Iptc4xmpCore:CreatorContactInfo rdf:parseType='Resource'>\
             <Iptc4xmpCore:CiEmailWork>a@example.org</Iptc4xmpCore:CiEmailWork>\
             </Iptc4xmpCore:CreatorContactInfo>",
        );
        let ev = assess(jpeg(
            Some(&x),
            Some(&exif(
                &[(0x013B, "A. Person")],
                &[(0xA430, "A. Person"), (0xA431, "SN-0042")],
            )),
            &[(80, b"A. Person")],
        ));
        let got = fields(&ev);
        for want in [
            ("exif:Artist", "A. Person"),
            ("iim:By-line", "A. Person"),
            ("dc:creator", "A. Person"),
            ("Iptc4xmpCore:CiEmailWork", "a@example.org"),
            ("exif:BodySerialNumber", "SN-0042"),
            ("exif:CameraOwnerName", "A. Person"),
        ] {
            assert!(
                got.contains(&(want.0.into(), want.1.into())),
                "{want:?} in {got:?}"
            );
        }
        assert!(
            ev.rationale
                .contains("3 person names, 1 contact detail and 2 device identifiers")
        );
    }

    #[test]
    fn the_platform_marker_is_an_account_record_and_other_instructions_are_not() {
        let marked = assess(jpeg(None, None, &[(40, b"FBMD0a000a880100003e02")]));
        assert_eq!(marked.status, Status::Present);
        assert_eq!(fields(&marked)[0].0, "iim:SpecialInstructions");
        assert_eq!(marked.details["identifiers"][0]["kind"], "account");

        let plain = assess(jpeg(None, None, &[(40, b"Not for resale")]));
        assert_eq!(plain.status, Status::Absent);
    }

    #[test]
    fn nothing_listed_is_absent_and_says_what_it_cannot_see() {
        for bytes in [
            jpeg(None, None, &[]),
            png(&[("Software", "x"), ("Copyright", "c")]),
        ] {
            let ev = assess(bytes);
            assert_eq!(ev.status, Status::Absent);
            assert!(
                ev.rationale
                    .contains("not a statement that the file identifies nobody")
            );
            assert!(ev.rationale.contains("MakerNotes") && ev.rationale.contains("C2PA manifest"));
            assert_eq!(ev.details["identifiers"], serde_json::json!([]));
        }
        let author = assess(png(&[("Author", "Some Person")]));
        assert_eq!(
            fields(&author),
            [("png:Author".to_string(), "Some Person".to_string())]
        );
    }

    #[test]
    fn values_are_masked_unless_revealed() {
        let x = packet(
            "xmlns:pdf='http://ns.adobe.com/pdf/1.3/' xmlns:Attrib='http://ns.attribution.com/ads/1.0/'",
            "<pdf:Author>Some Person</pdf:Author><Attrib:FbId>525265914179580</Attrib:FbId>",
        );
        let bytes = jpeg(
            Some(&x),
            Some(&exif(&[(0x013B, "Some Person")], &[])),
            &[(40, b"FBMD0a000a88")],
        );
        let a = Asset::from_bytes(bytes, None).unwrap();

        let masked = EmbeddedIdentifiers::default().assess(&a).unwrap();
        let text = serde_json::to_string(&masked).unwrap();
        for secret in ["Some Person", "525265914179580", "FBMD0a000a88"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        assert_eq!(masked.details["values_masked"], true);
        assert_eq!(
            masked.details["identifiers"][0],
            serde_json::json!({ "kind": "person", "field": "exif:Artist", "masked": true, "length": 11 })
        );
        assert!(
            masked
                .rationale
                .contains("values masked here (--reveal includes them)")
        );

        let open = EmbeddedIdentifiers { reveal: true }.assess(&a).unwrap();
        assert_eq!(open.details["values_masked"], false);
        assert_eq!(open.details["identifiers"][0]["value"], "Some Person");
        assert!(open.details["identifiers"][0].get("masked").is_none());
        // Same finding either way: status, count and order do not depend on the flag.
        assert_eq!(masked.status, open.status);
        assert_eq!(
            masked.details["identifiers"].as_array().unwrap().len(),
            open.details["identifiers"].as_array().unwrap().len()
        );
        let strip = |r: &str| {
            r.replace(" and value", "")
                .replace(", with the values masked here (--reveal includes them)", "")
        };
        assert_eq!(strip(&masked.rationale), strip(&open.rationale));
    }

    #[test]
    fn placeholders_and_decoded_values() {
        // A lens serial of "N/A" next to a real body serial: one identifier, not two.
        let ev = assess(jpeg(
            None,
            Some(&exif(&[], &[(0xA431, "XCR17"), (0xA435, "N/A")])),
            &[],
        ));
        assert_eq!(
            fields(&ev),
            [("exif:BodySerialNumber".to_string(), "XCR17".to_string())]
        );
        // The tracking blob is shown as the JSON it is, not with XML entities.
        let x = packet(
            "xmlns:Attrib='http://ns.attribution.com/ads/1.0/'",
            "<Attrib:Data>{&quot;doc&quot;:&quot;D1&quot;,&quot;user&quot;:&quot;U1&quot;}</Attrib:Data>",
        );
        let ev = assess(jpeg(Some(&x), None, &[]));
        assert_eq!(fields(&ev)[0].1, "{\"doc\":\"D1\",\"user\":\"U1\"}");
    }

    #[test]
    fn every_kind_has_a_documented_field_list() {
        let kinds: Vec<Kind> = FIELDS.iter().map(|f| f.0).collect();
        assert_eq!(kinds.len(), 6);
        assert!(
            kinds.windows(2).all(|w| w[0] < w[1]),
            "one line per kind, in order"
        );
        assert_eq!(
            join_and(&["a".into(), "b".into(), "c".into()]),
            "a, b and c"
        );
    }
}
