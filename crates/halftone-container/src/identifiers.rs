//! Embedded identifiers: the privacy listing. What a file tells anyone who receives
//! it about a person, an account, a device, a display, a document or a place.
//!
//! One source, one question: does the file's metadata carry identifiers? It lists
//! them from a fixed set of fields ([`FIELDS`] documents every one) in XMP, EXIF, IPTC
//! IIM, PNG text chunks and the ICC colour profile, quoted as written. Nothing is judged: a name may be a
//! pseudonym, an identifier may be stale. The point is what the file discloses.
//! Placeholders (`N/A`, `Unknown`, all zeros) and GPS tags without a position are
//! not identifiers and are left out.
//!
//! The EXIF image ID (`ImageUniqueID`) is listed as a document identifier. It is the
//! value a camera or a screenshot tool gives one image, and every copy that keeps
//! the EXIF block carries it, so two files with the same ID come from the same
//! capture. Whether it is unique is the writer's business: a phone's screenshot tool
//! writes a random UUID, some camera firmware writes the same string into every
//! file. It is quoted as written like everything else here.
//!
//! An iPhone or iPad camera writes its own identifiers, and not into any standard
//! field: they sit in Apple's MakerNote ([`crate::apple_makernote`]). Three are UUIDs
//! and are listed as document identifiers: one per photo (`PhotoIdentifier`), one per
//! capture (`ImageCaptureRequestID`), and one that a still shares with its Live Photo
//! clip (`ContentIdentifier`; the `live_photo` source reports what its presence
//! means). The fourth is listed as a device identifier though it names no device:
//! `RunTime`, the time since the device was switched on, counted in thousand
//! millionths of a second. Taken from the capture time it gives the moment of
//! power-on, which is the same for every photo the device takes until it restarts.
//! Two files that agree on it came from one device, in one stretch of use. Measured
//! on six camera files of one iPad (iPadOS 18.7.3, 2026-11-26); all four are in the
//! HEIC and the JPEG alike.
//!
//! A display is listed by the name of its colour profile. macOS embeds in a
//! screenshot the profile of the screen the capture came from (measured on macOS 27),
//! and that profile's description is usually the monitor's model name: the file says
//! what hardware sat on the desk. The description is listed when the profile is of the display class (`mntr`)
//! and the name is not a standard colour space's (`icc_standard_names.txt`; sRGB,
//! Display P3 and the like are display-class profiles too, and name no screen). That
//! rule can list a name that is no screen's: a vendor's own variant of a standard
//! space that is not on the list reads the same. It is masked like any other value,
//! and the list is where to add it.
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
//! own blocks, which often hold serial numbers and owner data) apart from the four
//! fields of Apple's above, the signer and
//! assertions of a C2PA manifest, embedded thumbnails and previews, the colour
//! profile of a HEIF file and what any profile holds besides its description
//! (Apple's `mmod` tag, for one, has fields for a display's make, model and serial
//! number), identifiers hidden in the pixels, and any field outside the list.

use exif::Tag;
use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
use serde::Serialize;

use crate::embedded::{Embedded, is_placeholder};
use crate::icc;

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
    /// A display: the name of the colour profile made for one screen.
    Display,
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
            Kind::Display => ("display name", "display names"),
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
        "EXIF body and lens serial numbers, camera owner name, and their XMP copies; \
         Apple MakerNote RunTime (time since power-on)",
    ),
    (
        Kind::Display,
        "ICC profile description, for a display-class profile not named after a standard colour space",
    ),
    (
        Kind::Document,
        "EXIF ImageUniqueID; Apple MakerNote PhotoIdentifier, ImageCaptureRequestID, \
         ContentIdentifier; xmpMM DocumentID, OriginalDocumentID, InstanceID",
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
    // Apple's cameras: the time since power-on, as the count the camera wrote.
    let apple = e.apple_makernote();
    if let Some(run) = apple.as_ref().and_then(|m| m.run_time) {
        add(Kind::Device, "apple:RunTime", run.value.to_string());
    }

    // Displays: the profile a screenshot tool embeds is the screen's own. A standard
    // colour space is a display-class profile too and names no screen.
    if let Some(p) = e.icc.profile().filter(|p| p.is_display_class()) {
        if let Some(name) = p.description.as_ref().filter(|d| !icc::is_standard_name(d)) {
            add(Kind::Display, "icc:ProfileDescription", name.clone());
        }
    }

    // Documents. The EXIF image ID first: what the capturing device wrote, before
    // what editing software added.
    if let Some(v) = e.exif_text(Tag::ImageUniqueID) {
        add(Kind::Document, "exif:ImageUniqueID", v);
    }
    if let Some(m) = &apple {
        for (field, value) in [
            ("apple:PhotoIdentifier", &m.photo_identifier),
            ("apple:ImageCaptureRequestID", &m.capture_request_id),
            ("apple:ContentIdentifier", &m.content_identifier),
        ] {
            if let Some(v) = value {
                add(Kind::Document, field, v.clone());
            }
        }
    }
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

        const BLIND: &str = "Not read: camera makers' own EXIF blocks (MakerNotes) apart from \
            four fields of Apple's, the signer and contents of a C2PA manifest, embedded \
            thumbnails and previews, the colour profile of a HEIF file and any profile's \
            contents beyond its description, anything hidden in the pixels, and fields \
            outside this source's list.";
        let makernote = e.apple_makernote();
        let makernote_incomplete = makernote.as_ref().is_some_and(|m| m.incomplete());
        let (status, rationale) = if found.is_empty() {
            let also = match (e.icc.unreadable(), makernote_incomplete) {
                (true, true) => Some("the colour profile and part of Apple's MakerNote"),
                (true, false) => Some("the colour profile"),
                (false, true) => Some("part of Apple's MakerNote"),
                (false, false) => None,
            };
            let unread_what = e.unread(also);
            let unread = unread_what.is_some();
            (
                if unread {
                    Status::Inconclusive
                } else {
                    Status::Absent
                },
                format!(
                    "None of the listed identifier fields is present{}. This is not a statement \
                     that the file identifies nobody. {BLIND}",
                    match unread_what {
                        Some(what) => format!(" in what could be read; {what} could not be read"),
                        None => String::new(),
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
                "icc_profile": e.icc.state(),
                "apple_makernote": match &makernote {
                    None => "absent",
                    Some(m) if m.incomplete() => "incomplete",
                    Some(_) => "read",
                },
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

    /// A PNG whose EXIF block is an `eXIf` chunk, as Android's screenshot tool writes.
    fn png_with_exif(block: &[u8]) -> Vec<u8> {
        let tiff = block.strip_prefix(b"Exif\0\0").unwrap();
        let mut out = png(&[]);
        let iend = out.len() - 12;
        let mut chunk = (tiff.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(b"eXIf");
        chunk.extend_from_slice(tiff);
        chunk.extend_from_slice(&[0; 4]);
        out.splice(iend..iend, chunk);
        out
    }

    #[test]
    fn an_apple_camera_file_discloses_its_makernote_identifiers() {
        use crate::apple_makernote::testutil::{CONTENT, PHOTO, REQUEST, TICKS, camera};
        use crate::embedded::testutil::exif_with;

        // A still: the run time, the photo and the capture request. No Live Photo.
        let still = camera(false);
        let ev = assess(jpeg(
            None,
            Some(&exif_with(&[(0x010F, "Apple")], &[], &[(0x927C, &still)])),
            &[],
        ));
        assert_eq!(ev.status, Status::Present);
        assert_eq!(
            fields(&ev),
            [
                ("apple:RunTime".to_string(), TICKS.to_string()),
                ("apple:PhotoIdentifier".to_string(), PHOTO.to_string()),
                (
                    "apple:ImageCaptureRequestID".to_string(),
                    REQUEST.to_string()
                ),
            ]
        );
        assert_eq!(ev.details["identifiers"][0]["kind"], "device");
        assert_eq!(ev.details["identifiers"][1]["kind"], "document");
        assert_eq!(ev.details["apple_makernote"], "read");
        assert!(
            ev.rationale
                .contains("carries 1 device identifier and 2 document identifiers"),
            "{}",
            ev.rationale
        );
        // What is still not read is still said, and no longer says "all MakerNotes".
        assert!(
            ev.rationale
                .contains("MakerNotes) apart from four fields of Apple's"),
            "{}",
            ev.rationale
        );

        // A Live Photo still adds the identifier it shares with its clip, last.
        let live = camera(true);
        let ev = assess(jpeg(
            None,
            Some(&exif_with(&[], &[], &[(0x927C, &live)])),
            &[],
        ));
        assert_eq!(
            fields(&ev).last().unwrap(),
            &("apple:ContentIdentifier".to_string(), CONTENT.to_string())
        );
        assert_eq!(fields(&ev).len(), 4);

        // Masked by default: field, kind and length, never the value.
        let a = Asset::from_bytes(
            jpeg(None, Some(&exif_with(&[], &[], &[(0x927C, &live)])), &[]),
            None,
        )
        .unwrap();
        let masked = EmbeddedIdentifiers::default().assess(&a).unwrap();
        let text = serde_json::to_string(&masked.details).unwrap() + &masked.rationale;
        for secret in [PHOTO, REQUEST, CONTENT, &TICKS.to_string()] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        assert_eq!(
            masked.details["identifiers"][1],
            serde_json::json!({ "kind": "document", "field": "apple:PhotoIdentifier",
                                "masked": true, "length": 36 })
        );
    }

    #[test]
    fn another_makers_block_and_a_damaged_apple_block_list_nothing() {
        use crate::apple_makernote::testutil::camera;
        use crate::embedded::testutil::exif_with;

        // Another maker's MakerNote: not read, and the file is not called incomplete.
        let ev = assess(jpeg(
            None,
            Some(&exif_with(
                &[(0x010F, "NIKON")],
                &[],
                &[(0x927C, b"Nikon\0\x02\x10\0\0MM\0\x2A")],
            )),
            &[],
        ));
        assert_eq!(ev.status, Status::Absent);
        assert_eq!(ev.details["apple_makernote"], "absent");

        // Apple's block cut short before any listed field could be read: nothing is
        // listed, and "none found" is not claimed.
        let whole = camera(true);
        let cut = &whole[..40];
        let ev = assess(jpeg(
            None,
            Some(&exif_with(&[], &[], &[(0x927C, cut)])),
            &[],
        ));
        assert_eq!(ev.status, Status::Inconclusive, "{}", ev.rationale);
        assert_eq!(ev.details["apple_makernote"], "incomplete");
        assert!(
            ev.rationale
                .contains("part of Apple's MakerNote could not be read"),
            "{}",
            ev.rationale
        );
    }

    #[test]
    fn the_exif_image_id_is_a_document_identifier() {
        const ID: &str = "3f2b8c1e-7a4d-4e9b-9c0a-5d6e7f809112";
        // In a JPEG, next to an xmpMM ID: both are listed, the capture's own first.
        let x = packet(
            "xmlns:xmpMM='http://ns.adobe.com/xap/1.0/mm/'",
            "<xmpMM:DocumentID>xmp.did:1234</xmpMM:DocumentID>",
        );
        let ev = assess(jpeg(Some(&x), Some(&exif(&[], &[(0xA420, ID)])), &[]));
        assert_eq!(ev.status, Status::Present);
        assert_eq!(
            fields(&ev),
            [
                ("exif:ImageUniqueID".to_string(), ID.to_string()),
                ("xmpMM:DocumentID".to_string(), "xmp.did:1234".to_string()),
            ]
        );
        assert_eq!(ev.details["identifiers"][0]["kind"], "document");
        assert!(
            ev.rationale.contains("carries 2 document identifiers"),
            "{}",
            ev.rationale
        );

        // In a PNG's eXIf chunk, the only thing in the file that the list covers:
        // a screenshot with no XMP and no text chunk still discloses it.
        let shot = png_with_exif(&exif(
            &[(0x0131, "Android AB1C.230405.006")],
            &[(0xA420, ID)],
        ));
        let ev = assess(shot);
        assert_eq!(ev.status, Status::Present);
        assert_eq!(
            fields(&ev),
            [("exif:ImageUniqueID".to_string(), ID.to_string())]
        );
        assert!(ev.rationale.contains("carries 1 document identifier."));
    }

    #[test]
    fn the_exif_image_id_is_masked_and_an_empty_one_is_not_listed() {
        const ID: &str = "3f2b8c1e-7a4d-4e9b-9c0a-5d6e7f809112";
        let a =
            Asset::from_bytes(jpeg(None, Some(&exif(&[], &[(0xA420, ID)])), &[]), None).unwrap();
        let masked = EmbeddedIdentifiers::default().assess(&a).unwrap();
        assert!(!serde_json::to_string(&masked).unwrap().contains(ID));
        assert_eq!(
            masked.details["identifiers"],
            serde_json::json!([{ "kind": "document", "field": "exif:ImageUniqueID",
                                 "masked": true, "length": 36 }])
        );

        // Cameras that have no ID to give fill the field: zeros, or spaces.
        for filler in [
            "00000000000000000000000000000000",
            "                                ",
        ] {
            let ev = assess(jpeg(None, Some(&exif(&[], &[(0xA420, filler)])), &[]));
            assert_eq!(ev.status, Status::Absent, "{filler:?}");
            assert_eq!(ev.details["identifiers"], serde_json::json!([]));
        }
        // The tag belongs to the Exif sub-IFD. In IFD0 it is not this field, and
        // ExifTool does not report it as ExifIFD:ImageUniqueID either.
        let misplaced = assess(jpeg(None, Some(&exif(&[(0xA420, ID)], &[])), &[]));
        assert_eq!(misplaced.status, Status::Absent);
    }

    #[test]
    fn a_display_profile_names_the_screen_in_every_container() {
        use crate::icc::testutil::{display, jpeg_with, png_with, webp_with};
        const NAME: &str = "Example Display EX2700";
        let profile = display(NAME);
        for (what, bytes) in [
            ("jpeg", jpeg_with(&profile, 1)),
            ("jpeg, profile in two chunks", jpeg_with(&profile, 2)),
            ("png", png_with("ICC Profile", &profile)),
            ("webp", webp_with(&profile)),
        ] {
            let ev = assess(bytes.clone());
            assert_eq!(ev.status, Status::Present, "{what}");
            assert_eq!(
                fields(&ev),
                [("icc:ProfileDescription".to_string(), NAME.to_string())],
                "{what}"
            );
            assert_eq!(ev.details["identifiers"][0]["kind"], "display");
            assert_eq!(ev.details["icc_profile"], "read");
            assert!(
                ev.rationale.contains("carries 1 display name."),
                "{}",
                ev.rationale
            );

            // Masked by default: field, kind and length, never the name.
            let a = Asset::from_bytes(bytes, None).unwrap();
            let masked = EmbeddedIdentifiers::default().assess(&a).unwrap();
            assert!(!serde_json::to_string(&masked).unwrap().contains(NAME));
            assert_eq!(
                masked.details["identifiers"],
                serde_json::json!([{ "kind": "display", "field": "icc:ProfileDescription",
                                     "masked": true, "length": 22 }])
            );
        }
        // macOS's name for a screen it has no model for is listed too: it is not a
        // colour space, and the profile is still that screen's.
        let generic = assess(jpeg_with(&display("Display"), 1));
        assert_eq!(
            fields(&generic),
            [("icc:ProfileDescription".into(), "Display".into())]
        );
    }

    #[test]
    fn a_standard_colour_space_or_another_class_of_profile_names_no_screen() {
        use crate::icc::testutil::{desc_v2, display, jpeg_with, mluc, png_with, profile};
        // Display-class profiles of standard colour spaces, however they are spelled.
        for name in ["sRGB IEC61966-2.1", "Display P3", "display  p3", "c2"] {
            let ev = assess(jpeg_with(&display(name), 1));
            assert_eq!(ev.status, Status::Absent, "{name}");
            assert_eq!(ev.details["identifiers"], serde_json::json!([]));
            assert_eq!(ev.details["icc_profile"], "read");
        }
        // The same in a version 4 profile.
        let v4 = profile(b"mntr", 4, &[(b"desc", mluc(&[("enUS", "sRGB built-in")]))]);
        assert_eq!(assess(png_with("ICC Profile", &v4)).status, Status::Absent);
        // A printer or scanner profile is named after an output condition or a
        // device that is no screen: not this field's business.
        for class in [b"prtr", b"scnr", b"spac"] {
            let p = profile(class, 2, &[(b"desc", desc_v2("Example Press Coated v3"))]);
            assert_eq!(assess(jpeg_with(&p, 1)).status, Status::Absent, "{class:?}");
        }
        // A display profile without a description, and a file without a profile.
        let bare = profile(b"mntr", 2, &[(b"wtpt", vec![0; 20])]);
        assert_eq!(assess(jpeg_with(&bare, 1)).status, Status::Absent);
        let none = assess(jpeg(None, None, &[]));
        assert_eq!(none.details["icc_profile"], "absent");
        assert!(
            none.rationale
                .contains("the colour profile of a HEIF file and any profile's contents")
        );
    }

    #[test]
    fn a_profile_that_cannot_be_read_makes_nothing_found_inconclusive() {
        use crate::icc::testutil::{display, jpeg_with, png_with_stream};
        // A PNG whose iCCP stream is cut short: a profile is there, its name unknown.
        let stream = b"\x78\x9c\x01\xff";
        let ev = assess(png_with_stream("ICC Profile", 0, stream));
        assert_eq!(ev.status, Status::Inconclusive);
        assert_eq!(ev.details["icc_profile"], "unreadable");
        assert!(
            ev.rationale
                .contains("in what could be read; the colour profile could not be read"),
            "{}",
            ev.rationale
        );
        // With something else found the file is Present, as with unread XMP.
        let mut both = jpeg_with(&display("x")[..40], 1);
        let exif_block = exif(&[(0x013B, "A. Person")], &[]);
        let mut app1 = vec![0xFF, 0xE1];
        app1.extend_from_slice(&(exif_block.len() as u16 + 2).to_be_bytes());
        app1.extend(exif_block);
        both.splice(2..2, app1);
        let ev = assess(both);
        assert_eq!(ev.status, Status::Present);
        assert_eq!(ev.details["icc_profile"], "unreadable");
        assert_eq!(fields(&ev), [("exif:Artist".into(), "A. Person".into())]);
    }

    #[test]
    fn every_kind_has_a_documented_field_list() {
        let kinds: Vec<Kind> = FIELDS.iter().map(|f| f.0).collect();
        assert_eq!(kinds.len(), 7);
        assert!(
            kinds.windows(2).all(|w| w[0] < w[1]),
            "one line per kind, in order"
        );
        assert_eq!(
            join_and(&["a".into(), "b".into(), "c".into()]),
            "a, b and c"
        );
    }

    #[test]
    fn png_iptc_profile_is_read_and_an_unread_chunk_is_said() {
        use crate::pngtext::testutil::ztxt_damaged;
        use crate::pngtext::testutil::{iim, irb, png as png_of, raw_profile_text, ztxt};
        let block = raw_profile_text("IPTC profile", &irb(&iim(&[(80, b"A. Person")])));
        let ev = assess(png_of(&[(b"zTXt", ztxt("Raw profile type iptc", &block))]));
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
        assert_eq!(
            fields(&ev),
            [("iim:By-line".to_string(), "A. Person".to_string())]
        );

        let ev = assess(png_of(&[(
            b"zTXt",
            ztxt_damaged("Author", b"Someone Else, at some length"),
        )]));
        assert_eq!(ev.status, Status::Inconclusive, "{}", ev.rationale);
        assert!(
            ev.rationale
                .contains("a compressed text chunk could not be read"),
            "{}",
            ev.rationale
        );
        assert_eq!(ev.details["identifiers"], serde_json::json!([]));
    }
}
