//! Rights statements: what a file's metadata says about copyright, licensing, credit
//! and permitted use, quoted as written.
//!
//! One source, one question: does the file's metadata carry rights or credit
//! statements? It lists them from a fixed set of fields ([`FIELDS`]) in XMP, EXIF,
//! IPTC IIM and PNG text chunks. It does not read them as a lawyer would: free text
//! is quoted, never interpreted, and nothing here says who owns anything or whether a
//! statement is valid. The one machine-readable rights field with a defined
//! vocabulary, the data-mining reservation, has its own source
//! ([`crate::data_mining`]).
//!
//! Status semantics:
//! - `Present`: at least one statement is there.
//! - `Absent`: none of the listed fields is there. Absence is not a waiver.
//! - `Inconclusive`: the copies of one statement disagree. The copyright notice is
//!   meant to be stored identically in EXIF, IIM and XMP, and the credit line in IIM
//!   and XMP; copies that differ show that something rewrote one and not the others,
//!   and this source will not pick which one counts. Also when nothing was found but
//!   some metadata could not be read.
//!
//! A copy cut at IIM's field limit (128 bytes for the notice, 32 for the credit) is
//! the same statement, not a disagreement.
//!
//! Free-text values are masked unless the source is built with `reveal`
//! (`ht inspect --reveal`): notices, credits, owners, terms and links name people and
//! organisations, and a report should not repeat them unasked. Fields whose value is
//! a fixed term or a licence URL ([`OPEN_FIELDS`]) are always shown. The status and
//! the kinds named in the rationale are the same either way.
//!
//! Unsigned throughout: these fields record what the last writer stated.

use exif::Tag;
use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
use serde::Serialize;

use crate::embedded::Embedded;
use crate::identifiers::join_and;

const DC: &str = "http://purl.org/dc/elements/1.1/";
const XMP_RIGHTS: &str = "http://ns.adobe.com/xap/1.0/rights/";
const PHOTOSHOP: &str = "http://ns.adobe.com/photoshop/1.0/";
const PLUS: &str = "http://ns.useplus.org/ldf/xmp/1.0/";
const CC: &str = "http://creativecommons.org/ns#";
const IPTC_EXT: &str = "http://iptc.org/std/Iptc4xmpExt/2008-02-29/";
/// IIM field limits, in bytes: a copy this long may have been cut by the format.
const IIM_COPYRIGHT_MAX: usize = 128;
const IIM_CREDIT_MAX: usize = 32;

/// What a statement is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The copyright notice.
    CopyrightNotice,
    /// Who owns or licenses the rights: owner, licensor, image supplier.
    RightsHolder,
    /// Terms of use, instructions and their flags (`Marked`, credit line required).
    UsageTerms,
    /// A link to a rights statement or licence.
    WebStatement,
    /// A licence named by URL or by a Creative Commons property.
    Licence,
    /// The credit line and the source it names.
    Credit,
    /// Model or property releases and age disclosure.
    Release,
    /// A machine-readable rights expression, embedded or linked.
    RightsExpression,
}

impl Kind {
    fn noun(self) -> &'static str {
        match self {
            Kind::CopyrightNotice => "a copyright notice",
            Kind::RightsHolder => "rights-holder details",
            Kind::UsageTerms => "usage terms or instructions",
            Kind::WebStatement => "a link to a rights statement",
            Kind::Licence => "a licence",
            Kind::Credit => "a credit line or source",
            Kind::Release => "release information",
            Kind::RightsExpression => "a machine-readable rights expression",
        }
    }
}

/// The fields this source reads, for documentation and the `Absent` rationale.
pub const FIELDS: &[(Kind, &str)] = &[
    (
        Kind::CopyrightNotice,
        "EXIF Copyright, IPTC Copyright Notice, dc:rights, PNG Copyright",
    ),
    (
        Kind::RightsHolder,
        "xmpRights:Owner, PLUS copyright owner, licensor and image supplier",
    ),
    (
        Kind::UsageTerms,
        "xmpRights:UsageTerms, Marked and Certificate, IPTC and Photoshop instructions, PLUS \
         credit-line-required and reuse, PNG Disclaimer",
    ),
    (Kind::WebStatement, "xmpRights:WebStatement"),
    (
        Kind::Licence,
        "cc:license, cc:morePermissions, cc:attributionName, cc:attributionURL",
    ),
    (
        Kind::Credit,
        "IPTC Credit and Source, photoshop:Credit and photoshop:Source, PNG Source",
    ),
    (
        Kind::Release,
        "PLUS model and property release status, minor model age disclosure",
    ),
    (
        Kind::RightsExpression,
        "IPTC embedded and linked encoded rights expressions",
    ),
];

/// Fields shown even when values are masked: a flag, a term from a fixed vocabulary,
/// or a public licence URL. None of them can name a person.
pub const OPEN_FIELDS: &[&str] = &[
    "xmpRights:Marked",
    "plus:CreditLineRequired",
    "plus:Reuse",
    "plus:ModelReleaseStatus",
    "plus:PropertyReleaseStatus",
    "plus:MinorModelAgeDisclosure",
    "cc:license",
    "Iptc4xmpExt:RightsExprEncType",
];

/// A field and value as they appear in `details`: with the value, or masked.
fn shown(field: &str, value: &str, reveal: bool) -> serde_json::Value {
    if reveal || OPEN_FIELDS.contains(&field) {
        serde_json::json!({ "field": field, "value": value })
    } else {
        serde_json::json!({ "field": field, "masked": true, "length": value.chars().count() })
    }
}

/// One statement found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Statement {
    /// What it is about.
    pub kind: Kind,
    /// The field it was read from (`exif:Copyright`, `dc:rights`, …).
    pub field: String,
    /// The value as written, bounded.
    pub value: String,
}

/// Copies of one statement that do not agree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Disagreement {
    /// Which statement (`copyright notice`, `credit line`).
    pub statement: &'static str,
    /// The differing copies: (field, value).
    pub copies: Vec<(String, String)>,
}

/// Two copies of one statement agree when they are equal, or when the IIM copy is
/// the other one cut at IIM's field limit.
fn same(a: &str, b: &str, iim_limit: Option<usize>) -> bool {
    if a == b {
        return true;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    match iim_limit {
        // The limit is in bytes; a cut copy sits within one character of it.
        Some(limit) => long.starts_with(short) && short.len() + 4 > limit,
        None => false,
    }
}

/// List every rights statement in the asset and the copies that disagree.
pub fn scan(e: &Embedded) -> (Vec<Statement>, Vec<Disagreement>) {
    let mut out: Vec<Statement> = Vec::new();
    let mut add = |kind: Kind, field: &str, value: String| {
        let s = Statement {
            kind,
            field: field.to_string(),
            value,
        };
        if !out.contains(&s) {
            out.push(s);
        }
    };
    let xmp = |ns: &str, prefix: &str, name: &str, nested: bool| -> Vec<String> {
        e.xmp_values(ns, prefix, name, nested)
            .into_iter()
            .map(|v| v.value)
            .collect()
    };

    // Copyright notice: three copies, plus PNG's own. EXIF can hold two parts
    // (photographer, then editor); the first is the one the other copies mirror.
    let exif_first = e.exif_parts(Tag::Copyright).into_iter().next();
    let exif_c = e.exif_text(Tag::Copyright);
    let iim_c = e.iim_values(116);
    let xmp_c = xmp(DC, "dc", "rights", true);
    if let Some(v) = &exif_c {
        add(Kind::CopyrightNotice, "exif:Copyright", v.clone());
    }
    for v in &iim_c {
        add(Kind::CopyrightNotice, "iim:CopyrightNotice", v.clone());
    }
    for v in &xmp_c {
        add(Kind::CopyrightNotice, "dc:rights", v.clone());
    }
    for v in e.png_values("Copyright") {
        add(Kind::CopyrightNotice, "png:Copyright", v);
    }

    // Rights holders.
    for v in xmp(XMP_RIGHTS, "xmpRights", "Owner", true) {
        add(Kind::RightsHolder, "xmpRights:Owner", v);
    }
    for name in [
        "CopyrightOwnerName",
        "LicensorName",
        "LicensorURL",
        "LicensorEmail",
        "ImageSupplierName",
    ] {
        for v in xmp(PLUS, "plus", name, false) {
            add(Kind::RightsHolder, &format!("plus:{name}"), v);
        }
    }

    // Usage terms, flags and instructions.
    for v in xmp(XMP_RIGHTS, "xmpRights", "UsageTerms", true) {
        add(Kind::UsageTerms, "xmpRights:UsageTerms", v);
    }
    for name in ["Marked", "Certificate"] {
        for v in xmp(XMP_RIGHTS, "xmpRights", name, false) {
            add(Kind::UsageTerms, &format!("xmpRights:{name}"), v);
        }
    }
    // The FBMD platform marker in the instructions field is a tracking record, not
    // an instruction; embedded_identifiers reports it.
    for v in e.iim_values(40) {
        if !v.starts_with("FBMD") {
            add(Kind::UsageTerms, "iim:SpecialInstructions", v);
        }
    }
    for v in xmp(PHOTOSHOP, "photoshop", "Instructions", false) {
        if !v.starts_with("FBMD") {
            add(Kind::UsageTerms, "photoshop:Instructions", v);
        }
    }
    for name in ["CreditLineRequired", "Reuse"] {
        for v in xmp(PLUS, "plus", name, false) {
            add(Kind::UsageTerms, &format!("plus:{name}"), v);
        }
    }
    for v in e.png_values("Disclaimer") {
        add(Kind::UsageTerms, "png:Disclaimer", v);
    }

    // Web statement and licences.
    for v in xmp(XMP_RIGHTS, "xmpRights", "WebStatement", false) {
        add(Kind::WebStatement, "xmpRights:WebStatement", v);
    }
    for name in [
        "license",
        "morePermissions",
        "attributionName",
        "attributionURL",
    ] {
        for v in xmp(CC, "cc", name, false) {
            add(Kind::Licence, &format!("cc:{name}"), v);
        }
    }

    // Credit line (two copies) and source.
    let iim_credit = e.iim_values(110);
    let xmp_credit = xmp(PHOTOSHOP, "photoshop", "Credit", false);
    for v in &iim_credit {
        add(Kind::Credit, "iim:Credit", v.clone());
    }
    for v in &xmp_credit {
        add(Kind::Credit, "photoshop:Credit", v.clone());
    }
    for v in e.iim_values(115) {
        add(Kind::Credit, "iim:Source", v);
    }
    for v in xmp(PHOTOSHOP, "photoshop", "Source", false) {
        add(Kind::Credit, "photoshop:Source", v);
    }
    for v in e.png_values("Source") {
        add(Kind::Credit, "png:Source", v);
    }

    // Releases.
    for name in [
        "ModelReleaseStatus",
        "PropertyReleaseStatus",
        "MinorModelAgeDisclosure",
    ] {
        for v in xmp(PLUS, "plus", name, false) {
            add(Kind::Release, &format!("plus:{name}"), v);
        }
    }

    // Rights expressions: the link, or the fact that one is embedded.
    for v in xmp(IPTC_EXT, "Iptc4xmpExt", "LinkedRightsExpr", false) {
        add(Kind::RightsExpression, "Iptc4xmpExt:LinkedRightsExpr", v);
    }
    for v in xmp(IPTC_EXT, "Iptc4xmpExt", "RightsExprEncType", false) {
        add(Kind::RightsExpression, "Iptc4xmpExt:RightsExprEncType", v);
    }

    // Copies that disagree.
    let mut disagreements = Vec::new();
    let mut check = |statement: &'static str, copies: Vec<(&str, &String, bool)>, limit: usize| {
        let differs = copies.iter().enumerate().any(|(i, a)| {
            copies[i + 1..].iter().any(|b| {
                let lim = (a.2 || b.2).then_some(limit);
                !same(a.1, b.1, lim)
            })
        });
        if differs {
            disagreements.push(Disagreement {
                statement,
                copies: copies
                    .iter()
                    .map(|(f, v, _)| (f.to_string(), (*v).clone()))
                    .collect(),
            });
        }
    };
    let mut copies: Vec<(&str, &String, bool)> = Vec::new();
    if let Some(v) = &exif_first {
        copies.push(("exif:Copyright", v, false));
    }
    if let Some(v) = iim_c.first() {
        copies.push(("iim:CopyrightNotice", v, true));
    }
    if let Some(v) = xmp_c.first() {
        copies.push(("dc:rights", v, false));
    }
    check("copyright notice", copies, IIM_COPYRIGHT_MAX);
    let mut copies: Vec<(&str, &String, bool)> = Vec::new();
    if let Some(v) = iim_credit.first() {
        copies.push(("iim:Credit", v, true));
    }
    if let Some(v) = xmp_credit.first() {
        copies.push(("photoshop:Credit", v, false));
    }
    check("credit line", copies, IIM_CREDIT_MAX);
    (out, disagreements)
}

/// Rights statements evidence source (`rights_statements`).
#[derive(Debug, Default)]
pub struct RightsStatements {
    /// Put free-text values into `details` and the rationale. Off by default.
    pub reveal: bool,
}

impl EvidenceSource for RightsStatements {
    fn id(&self) -> SourceId {
        SourceId {
            name: "rights_statements".into(),
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
        let (found, disagreements) = scan(&e);

        const CAVEAT: &str = "They are quoted as written by whatever software last saved the \
            file: unsigned, not interpreted, and no statement about who holds any right or \
            whether a notice is valid. The data-mining reservation has its own source \
            (data_mining_reservation); rights stated in a C2PA manifest or outside the file are \
            not read here.";
        let mut kinds: Vec<Kind> = found.iter().map(|s| s.kind).collect();
        kinds.sort();
        kinds.dedup();
        let nouns: Vec<String> = kinds.iter().map(|k| k.noun().to_string()).collect();

        let masked_note = if self.reveal {
            ""
        } else {
            " Free-text values are masked here (--reveal includes them)."
        };
        let (status, rationale) = if let Some(d) = disagreements.first() {
            let copies: Vec<String> = d
                .copies
                .iter()
                .map(|(f, v)| {
                    if self.reveal {
                        format!("{f} = \"{v}\"")
                    } else {
                        f.clone()
                    }
                })
                .collect();
            (
                Status::Inconclusive,
                format!(
                    "The copies of the {} disagree: {}. They are meant to be identical, so \
                     something rewrote one and not the others; this source does not pick which \
                     one counts.{masked_note} {CAVEAT}",
                    d.statement,
                    copies.join("; ")
                ),
            )
        } else if found.is_empty() {
            let unread = e.xmp_incomplete();
            (
                if unread {
                    Status::Inconclusive
                } else {
                    Status::Absent
                },
                format!(
                    "None of the listed rights or credit fields is present{}. Absence is not a \
                     waiver: rights do not depend on a notice in the file, and re-saving or \
                     sending a file commonly strips these fields.",
                    if unread {
                        " in what could be read; some XMP could not be read"
                    } else {
                        ""
                    }
                ),
            )
        } else {
            (
                Status::Present,
                format!(
                    "The file's metadata carries {}; details lists each statement with its \
                     field.{masked_note} {CAVEAT}",
                    join_and(&nouns)
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
                "statements": found
                    .iter()
                    .map(|s| {
                        let mut v = shown(&s.field, &s.value, self.reveal);
                        v["kind"] = serde_json::json!(s.kind);
                        v
                    })
                    .collect::<Vec<_>>(),
                "disagreements": disagreements
                    .iter()
                    .map(|d| {
                        serde_json::json!({
                            "statement": d.statement,
                            "copies": d
                                .copies
                                .iter()
                                .map(|(f, v)| shown(f, v, self.reveal))
                                .collect::<Vec<_>>(),
                        })
                    })
                    .collect::<Vec<_>>(),
                "values_masked": !self.reveal,
                "xmp_packets": e.xmp.len(),
                "xmp_incomplete": e.xmp_incomplete(),
                "has_exif": e.has_exif,
            }),
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedded::testutil::{exif, jpeg, packet, png};

    const NS: &str = "xmlns:dc='http://purl.org/dc/elements/1.1/' \
        xmlns:xmpRights='http://ns.adobe.com/xap/1.0/rights/' \
        xmlns:photoshop='http://ns.adobe.com/photoshop/1.0/' \
        xmlns:plus='http://ns.useplus.org/ldf/xmp/1.0/' \
        xmlns:cc='http://creativecommons.org/ns#'";

    /// Revealed, so the tests can check what was read.
    fn assess(bytes: Vec<u8>) -> Evidence {
        let a = Asset::from_bytes(bytes, None).unwrap();
        RightsStatements { reveal: true }.assess(&a).unwrap()
    }

    fn has(ev: &Evidence, field: &str, value: &str) -> bool {
        ev.details["statements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["field"] == field && s["value"] == value)
    }

    fn rights(text: &str) -> String {
        format!(
            "<dc:rights><rdf:Alt><rdf:li xml:lang='x-default'>{text}</rdf:li></rdf:Alt></dc:rights>"
        )
    }

    #[test]
    fn a_fully_captioned_photo_lists_every_statement() {
        let x = packet(
            NS,
            &format!(
                "{}<xmpRights:Marked>True</xmpRights:Marked>\
                 <xmpRights:WebStatement>https://example.org/licence</xmpRights:WebStatement>\
                 <xmpRights:UsageTerms><rdf:Alt><rdf:li xml:lang='x-default'>Editorial use only\
                 </rdf:li></rdf:Alt></xmpRights:UsageTerms>\
                 <photoshop:Credit>Example Agency</photoshop:Credit>\
                 <photoshop:Source>Example Wire</photoshop:Source>\
                 <plus:Licensor><rdf:Seq><rdf:li rdf:parseType='Resource'>\
                 <plus:LicensorName>Example Licensing</plus:LicensorName>\
                 <plus:LicensorURL>https://example.org/l</plus:LicensorURL></rdf:li></rdf:Seq>\
                 </plus:Licensor>\
                 <plus:ModelReleaseStatus>http://ns.useplus.org/ldf/vocab/MR-NON</plus:ModelReleaseStatus>\
                 <cc:license rdf:resource='https://creativecommons.org/licenses/by/4.0/'/>",
                rights("(c) 2026 A. Person")
            ),
        );
        let ev = assess(jpeg(
            Some(&x),
            Some(&exif(&[(0x8298, "(c) 2026 A. Person")], &[])),
            &[
                (116, b"(c) 2026 A. Person"),
                (110, b"Example Agency"),
                (115, b"Example Wire"),
            ],
        ));
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
        for (f, v) in [
            ("exif:Copyright", "(c) 2026 A. Person"),
            ("iim:CopyrightNotice", "(c) 2026 A. Person"),
            ("dc:rights", "(c) 2026 A. Person"),
            ("xmpRights:Marked", "True"),
            ("xmpRights:WebStatement", "https://example.org/licence"),
            ("xmpRights:UsageTerms", "Editorial use only"),
            ("photoshop:Credit", "Example Agency"),
            ("iim:Credit", "Example Agency"),
            ("iim:Source", "Example Wire"),
            ("plus:LicensorName", "Example Licensing"),
            ("plus:LicensorURL", "https://example.org/l"),
            (
                "plus:ModelReleaseStatus",
                "http://ns.useplus.org/ldf/vocab/MR-NON",
            ),
            ("cc:license", "https://creativecommons.org/licenses/by/4.0/"),
        ] {
            assert!(
                has(&ev, f, v),
                "{f} = {v} missing from {}",
                ev.details["statements"]
            );
        }
        assert_eq!(ev.details["disagreements"], serde_json::json!([]));
        for part in [
            "a copyright notice",
            "rights-holder details",
            "usage terms or instructions",
            "a licence",
            "a credit line or source",
            "release information",
        ] {
            assert!(ev.rationale.contains(part), "{part}: {}", ev.rationale);
        }
        assert!(ev.rationale.contains("unsigned") && ev.rationale.contains("not interpreted"));
    }

    #[test]
    fn copies_that_differ_are_inconclusive_and_both_are_quoted() {
        let x = packet(NS, &rights("(c) 2026 Someone Else"));
        let ev = assess(jpeg(
            Some(&x),
            Some(&exif(&[(0x8298, "(c) 2026 A. Person")], &[])),
            &[],
        ));
        assert_eq!(ev.status, Status::Inconclusive);
        assert!(
            ev.rationale
                .contains("copies of the copyright notice disagree")
        );
        assert!(
            ev.rationale
                .contains("exif:Copyright = \"(c) 2026 A. Person\"")
        );
        assert!(
            ev.rationale
                .contains("dc:rights = \"(c) 2026 Someone Else\"")
        );
        assert!(ev.rationale.contains("does not pick which one counts"));
        assert_eq!(
            ev.details["disagreements"][0]["statement"],
            "copyright notice"
        );

        let credit = packet(NS, "<photoshop:Credit>Agency A</photoshop:Credit>");
        let ev = assess(jpeg(Some(&credit), None, &[(110, b"Agency B")]));
        assert_eq!(ev.status, Status::Inconclusive);
        assert_eq!(ev.details["disagreements"][0]["statement"], "credit line");
    }

    #[test]
    fn an_iim_copy_cut_at_the_field_limit_is_the_same_statement() {
        let long = "Example Agency for International Photography"; // 44 bytes
        let cut = &long[..32];
        let x = packet(NS, &format!("<photoshop:Credit>{long}</photoshop:Credit>"));
        let ev = assess(jpeg(Some(&x), None, &[(110, cut.as_bytes())]));
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);

        // A short IIM copy that is merely a prefix is a different statement.
        let ev = assess(jpeg(Some(&x), None, &[(110, b"Example")]));
        assert_eq!(ev.status, Status::Inconclusive);

        assert!(same("abc", "abc", None));
        assert!(!same("abc", "abcd", None));
    }

    #[test]
    fn free_text_is_masked_unless_revealed_and_fixed_terms_stay_visible() {
        let x = packet(
            NS,
            &format!(
                "{}<xmpRights:Marked>True</xmpRights:Marked>\
                 <cc:license rdf:resource='https://creativecommons.org/licenses/by/4.0/'/>\
                 <photoshop:Credit>Someone Else Agency</photoshop:Credit>",
                rights("(c) 2026 Ada Example")
            ),
        );
        let bytes = jpeg(
            Some(&x),
            Some(&exif(&[(0x8298, "(c) 2026 Another Name")], &[])),
            &[],
        );
        let a = Asset::from_bytes(bytes, None).unwrap();

        let masked = RightsStatements::default().assess(&a).unwrap();
        let text = serde_json::to_string(&masked).unwrap();
        for secret in ["Ada Example", "Another Name", "Someone Else Agency"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        // The disagreement is still reported, by field.
        assert_eq!(masked.status, Status::Inconclusive);
        assert!(
            masked
                .rationale
                .contains("disagree: exif:Copyright; dc:rights.")
        );
        assert!(
            masked
                .rationale
                .contains("masked here (--reveal includes them)")
        );
        assert_eq!(
            masked.details["disagreements"][0]["copies"][0],
            serde_json::json!({ "field": "exif:Copyright", "masked": true, "length": 21 })
        );
        // Fixed terms and the licence URL say nothing about a person.
        assert!(has(&masked, "xmpRights:Marked", "True"));
        assert!(has(
            &masked,
            "cc:license",
            "https://creativecommons.org/licenses/by/4.0/"
        ));
        assert_eq!(masked.details["values_masked"], true);

        let open = RightsStatements { reveal: true }.assess(&a).unwrap();
        assert!(
            open.rationale
                .contains("exif:Copyright = \"(c) 2026 Another Name\"")
        );
        assert!(has(&open, "photoshop:Credit", "Someone Else Agency"));
        assert_eq!(masked.status, open.status);
    }

    #[test]
    fn a_two_part_exif_copyright_is_compared_by_its_first_part() {
        let x = packet(NS, &rights("(c) A. Person"));
        let ev = assess(jpeg(
            Some(&x),
            Some(&exif(&[(0x8298, "(c) A. Person\0(c) An Editor")], &[])),
            &[],
        ));
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
        assert!(has(&ev, "exif:Copyright", "(c) A. Person; (c) An Editor"));
    }

    #[test]
    fn the_platform_marker_is_not_an_instruction() {
        let ev = assess(jpeg(None, None, &[(40, b"FBMD0a000a880100003e02")]));
        assert_eq!(ev.status, Status::Absent);
        let ev = assess(jpeg(None, None, &[(40, b"Not for resale")]));
        assert_eq!(ev.status, Status::Present);
        assert!(has(&ev, "iim:SpecialInstructions", "Not for resale"));
    }

    #[test]
    fn nothing_listed_is_absent_and_absence_is_not_a_waiver() {
        let ev = assess(jpeg(None, None, &[]));
        assert_eq!(ev.status, Status::Absent);
        assert!(ev.rationale.contains("Absence is not a waiver"));
        // Creator and identifier fields belong to embedded_identifiers.
        let x = packet(
            NS,
            "<dc:creator><rdf:Seq><rdf:li>A. Person</rdf:li></rdf:Seq></dc:creator>",
        );
        assert_eq!(
            assess(jpeg(Some(&x), None, &[(80, b"A. Person")])).status,
            Status::Absent
        );

        let p = assess(png(&[("Copyright", "(c) Someone"), ("Author", "Someone")]));
        assert_eq!(p.status, Status::Present);
        assert!(has(&p, "png:Copyright", "(c) Someone"));
        assert_eq!(p.details["statements"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn every_kind_has_a_documented_field_list() {
        let kinds: Vec<Kind> = FIELDS.iter().map(|f| f.0).collect();
        assert_eq!(kinds.len(), 8);
        assert!(kinds.windows(2).all(|w| w[0] < w[1]));
    }
}
