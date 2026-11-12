//! Data-mining reservation: the IPTC / PLUS `Data Mining` property, reported as an
//! explicit field.
//!
//! Where the signal lives: IPTC Photo Metadata 2023.1 adopted the PLUS property
//! `plus:DataMining` (namespace `http://ns.useplus.org/ldf/xmp/1.0/`), whose value is
//! a term from the PLUS Data Mining vocabulary
//! (`http://ns.useplus.org/ldf/vocab/DMI-…`). It is how a rights holder states, in
//! machine-readable metadata inside the file, whether the image may be used for data
//! mining or AI training. This source reads it from XMP in the same containers and
//! with the same tolerant scan as `marking_metadata` ([`crate::marking`]).
//!
//! Who holds the key: nobody. It is plain text; anything can add it, change it or
//! remove it. What it survives: only paths that preserve XMP. How it is scored: it
//! is not; the field is there or not, and this source reports which and what it says.
//!
//! One source, one question: does this file's XMP reserve rights against data
//! mining? Status semantics:
//! - `Present`: a prohibiting term is declared (`DMI-PROHIBITED…`). The file's
//!   metadata states a reservation. Says nothing about who wrote it, who holds the
//!   rights, or what the statement is worth in law.
//! - `Absent`: no `Data Mining` property, or one that prohibits nothing
//!   (`DMI-UNSPECIFIED`, `DMI-ALLOWED`). Absence is not permission: a reservation can
//!   be made in other places this source does not read.
//! - `Inconclusive`: a value outside the vocabulary snapshot this build knows,
//!   prohibiting and non-prohibiting values together, or XMP that could not be read
//!   completely. Reported verbatim, never interpreted.
//!
//! Not read here, and named in the rationale where it matters: a C2PA
//! training-and-data-mining assertion (signed, inside the manifest), licence and
//! rights texts in other fields (`dc:rights`, `xmpRights:UsageTerms`), the rights
//! expressions two of the terms point to, and anything stated outside the file
//! (robots.txt, TDMRep, a site's terms).
//!
//! `plus:OtherConstraints` is part of the answer for the term
//! `DMI-PROHIBITED-SEECONSTRAINT`, which defers to it; its text is reported in
//! `details` and quoted in the rationale for that term, and never affects the status.

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
use serde::Serialize;

use crate::marking::{Form, find_property, packets, prefix_bound, xmp_text};

/// Namespace URI of the PLUS License Data Format in XMP, where `DataMining` lives.
pub const PLUS_NS: &str = "http://ns.useplus.org/ldf/xmp/1.0/";

/// URI prefix of the PLUS vocabulary concepts; a term's URI is this plus its code.
pub const VOCABULARY_URI: &str = "http://ns.useplus.org/ldf/vocab/";

/// Identifier of the vocabulary snapshot compiled into this build. Bump when
/// [`VOCABULARY`] changes so old reports say which list they were judged against.
pub const VOCABULARY_VERSION: &str = "plus-datamining-2023-1";

/// What a Data Mining term says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TermKind {
    /// Data mining is prohibited, wholly or for a stated purpose.
    Prohibited,
    /// Data mining is allowed.
    Allowed,
    /// Neither allowed nor prohibited: the writer states no position.
    Unspecified,
}

/// One entry of the PLUS Data Mining vocabulary as known to this build.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Term {
    /// Term code, the last path segment of the concept URI.
    pub code: &'static str,
    /// The vocabulary's own label.
    pub label: &'static str,
    /// Kind.
    pub kind: TermKind,
}

/// The PLUS Data Mining vocabulary as adopted by IPTC Photo Metadata 2023.1. Unknown
/// codes are reported verbatim and yield `Inconclusive`; this list decides
/// interpretation, not existence.
pub const VOCABULARY: &[Term] = &[
    Term {
        code: "DMI-UNSPECIFIED",
        label: "unspecified, no prohibition defined",
        kind: TermKind::Unspecified,
    },
    Term {
        code: "DMI-ALLOWED",
        label: "allowed",
        kind: TermKind::Allowed,
    },
    Term {
        code: "DMI-PROHIBITED-AIMLTRAINING",
        label: "prohibited for AI/ML training",
        kind: TermKind::Prohibited,
    },
    Term {
        code: "DMI-PROHIBITED-GENAIMLTRAINING",
        label: "prohibited for generative AI/ML training",
        kind: TermKind::Prohibited,
    },
    Term {
        code: "DMI-PROHIBITED-EXCEPTSEARCHENGINEINDEXING",
        label: "prohibited except for search engine indexing",
        kind: TermKind::Prohibited,
    },
    Term {
        code: "DMI-PROHIBITED",
        label: "prohibited",
        kind: TermKind::Prohibited,
    },
    Term {
        code: "DMI-PROHIBITED-SEECONSTRAINT",
        label: "prohibited, see the Other Constraints text",
        kind: TermKind::Prohibited,
    },
    Term {
        code: "DMI-PROHIBITED-SEEEMBEDDEDRIGHTSEXPR",
        label: "prohibited, see the embedded rights expression",
        kind: TermKind::Prohibited,
    },
    Term {
        code: "DMI-PROHIBITED-SEELINKEDRIGHTSEXPR",
        label: "prohibited, see the linked rights expression",
        kind: TermKind::Prohibited,
    },
];

/// Look a code up in [`VOCABULARY`].
pub fn lookup(code: &str) -> Option<&'static Term> {
    VOCABULARY.iter().find(|t| t.code == code)
}

/// Term code of a value as written: the value with the vocabulary URI prefix removed
/// once, as ExifTool reads it. A bare code and the concept URI both give the code.
/// Anything else stays as written and is then not in the vocabulary: a different
/// scheme or host, or the prefix twice (which ExifTool 13.59 itself writes when given
/// a full URI as a raw value, and then reads back as unknown).
pub fn code_of(raw: &str) -> String {
    let r = raw.trim();
    r.strip_prefix(VOCABULARY_URI).unwrap_or(r).to_string()
}

/// One `Data Mining` occurrence found in XMP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DataMiningValue {
    /// Namespace prefix the property was written with (`plus` by convention).
    pub prefix: String,
    /// The value exactly as written, trimmed.
    pub raw: String,
    /// Term code ([`code_of`]).
    pub code: String,
    /// Where the value sat.
    pub form: Form,
    /// Whether the prefix is bound to [`PLUS_NS`] somewhere in the packet.
    pub namespace_bound: bool,
}

/// Everything this source extracted from one asset.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DataMiningScan {
    /// XMP packets examined.
    pub xmp_packets: usize,
    /// Every `Data Mining` occurrence, in file order, duplicates kept.
    pub values: Vec<DataMiningValue>,
    /// The first `plus:OtherConstraints` text found, bounded.
    pub other_constraints: Option<String>,
    /// JPEG only: ExtendedXMP was present but could not be fully reassembled.
    pub xmp_extended_incomplete: bool,
    /// HEIF only: XMP items whose bytes could not be read, with why.
    pub xmp_items_unreadable: Vec<String>,
    /// A C2PA manifest container is present. A training-and-data-mining assertion
    /// inside it would be signed; this source does not read it.
    pub manifest_container_present: bool,
}

/// Extract the Data Mining property from JPEG, PNG, WebP or HEIF bytes.
pub fn scan(mime: &str, b: &[u8]) -> Result<DataMiningScan, String> {
    let (packets, read) = packets(mime, b)?;
    let mut s = DataMiningScan {
        xmp_extended_incomplete: read.xmp_extended_incomplete,
        xmp_items_unreadable: read.xmp_items_unreadable,
        manifest_container_present: read.manifest_container_present,
        ..DataMiningScan::default()
    };
    for p in packets {
        s.xmp_packets += 1;
        let text = xmp_text(&p);
        for v in find_property(&text, "DataMining", false) {
            s.values.push(DataMiningValue {
                code: code_of(&v.raw),
                namespace_bound: prefix_bound(&text, &v.prefix, PLUS_NS),
                prefix: v.prefix,
                raw: v.raw,
                form: v.form,
            });
        }
        if s.other_constraints.is_none() {
            s.other_constraints = find_property(&text, "OtherConstraints", true)
                .into_iter()
                .next()
                .map(|v| crate::embedded::unescape(&v.raw));
        }
    }
    Ok(s)
}

/// Data-mining reservation evidence source (`data_mining_reservation`).
///
/// `Present` = XMP declares a prohibiting `Data Mining` term; `Absent` = no such
/// property, or one that prohibits nothing; `Inconclusive` = a value outside the
/// known vocabulary, conflicting values, or XMP that could not be read completely.
/// An unsigned statement: it records what the writer stated and verifies nothing.
#[derive(Debug, Default)]
pub struct DataMiningReservation;

impl EvidenceSource for DataMiningReservation {
    fn id(&self) -> SourceId {
        SourceId {
            name: "data_mining_reservation".into(),
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
        let known = || terms.iter().flatten();
        let prohibiting: Vec<&Term> = known()
            .filter(|t| t.kind == TermKind::Prohibited)
            .copied()
            .collect();
        let permissive = known().any(|t| t.kind != TermKind::Prohibited);

        const CAVEAT: &str = "This is an unsigned statement written by whatever software last \
            saved the file: it records what the writer chose to state, says nothing about who \
            holds the rights, and is removed by any re-save or messaging path that strips XMP. \
            Whether it is a valid reservation under any law is a legal question this tool does \
            not answer.";
        let manifest = if s.manifest_container_present {
            " A C2PA manifest container is present: a training-and-data-mining assertion inside \
             it would be signed, and this source does not read it."
        } else {
            ""
        };

        let (status, rationale) = if codes.is_empty() {
            let unread = s.xmp_extended_incomplete || !s.xmp_items_unreadable.is_empty();
            let where_ = if s.xmp_extended_incomplete {
                "XMP present, ExtendedXMP could not be fully reassembled"
            } else if !s.xmp_items_unreadable.is_empty() {
                "an XMP item is listed but its bytes could not be read"
            } else if s.xmp_packets > 0 {
                "XMP present but without the property"
            } else {
                "no XMP"
            };
            (
                if unread {
                    Status::Inconclusive
                } else {
                    Status::Absent
                },
                format!(
                    "No IPTC/PLUS Data Mining property found ({where_}). The file's XMP states \
                     nothing about data mining. Absence is not permission: a reservation can be \
                     made in a licence text, in a signed C2PA assertion or outside the file \
                     (robots.txt, TDMRep, a site's terms), none of which this source reads, and \
                     any XMP statement is stripped by common re-save and messaging paths.{manifest}"
                ),
            )
        } else if !unknown.is_empty() {
            (
                Status::Inconclusive,
                format!(
                    "XMP carries a Data Mining value outside the PLUS vocabulary snapshot this \
                     build knows ({}): reported verbatim, not interpreted.",
                    unknown.join(", ")
                ),
            )
        } else if !prohibiting.is_empty() && permissive {
            (
                Status::Inconclusive,
                format!(
                    "XMP carries conflicting Data Mining values ({}); not interpreted. {CAVEAT}",
                    codes.join(", ")
                ),
            )
        } else if !prohibiting.is_empty() {
            let said: Vec<String> = prohibiting
                .iter()
                .map(|t| format!("{} ({})", t.code, t.label))
                .collect();
            let has = |code: &str| prohibiting.iter().any(|t| t.code == code);
            let mut refers = String::new();
            if has("DMI-PROHIBITED-SEECONSTRAINT") {
                refers.push_str(&match &s.other_constraints {
                    Some(text) => format!(" Its Other Constraints text reads: \"{text}\"."),
                    None => " It defers to an Other Constraints text, which the file does not \
                             carry."
                        .to_string(),
                });
            }
            if has("DMI-PROHIBITED-SEEEMBEDDEDRIGHTSEXPR")
                || has("DMI-PROHIBITED-SEELINKEDRIGHTSEXPR")
            {
                refers.push_str(
                    " It defers to a rights expression (embedded or linked), which this source \
                     does not read.",
                );
            }
            (
                Status::Present,
                format!(
                    "XMP declares the IPTC/PLUS Data Mining property = {}: the file's metadata \
                     reserves rights against data mining.{refers} {CAVEAT}{manifest}",
                    said.join(", ")
                ),
            )
        } else {
            let said: Vec<String> = known()
                .map(|t| format!("{} ({})", t.code, t.label))
                .collect();
            (
                Status::Absent,
                format!(
                    "XMP declares the IPTC/PLUS Data Mining property = {}: no reservation \
                     against data mining is stated. {CAVEAT}{manifest}",
                    said.join(", ")
                ),
            )
        };

        let unbound = s.values.iter().any(|v| !v.namespace_bound);
        let mut details = serde_json::json!({
            "vocabulary_version": VOCABULARY_VERSION,
            "data_mining": codes,
            "values": s.values,
            "other_constraints": s.other_constraints,
            "prefix_not_bound_to_plus": unbound,
            "xmp_packets": s.xmp_packets,
            "xmp_extended_incomplete": s.xmp_extended_incomplete,
            "manifest_container_present": s.manifest_container_present,
        });
        if a.mime == "image/heif" {
            details["xmp_items_unreadable"] = serde_json::json!(s.xmp_items_unreadable);
        }
        Ok(Evidence {
            layer: self.layer(),
            source: self.id(),
            status,
            statistic: None,
            calibration: None,
            rationale,
            details,
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jpeg::testutil::synth_jpeg;

    const VOCAB: &str = VOCABULARY_URI;

    /// The packet ExifTool 13.59 writes for `-XMP-plus:DataMining=…` (element form,
    /// single-quoted attributes), with the given property lines.
    fn exiftool_packet(properties: &str) -> String {
        format!(
            "<x:xmpmeta xmlns:x='adobe:ns:meta/' x:xmptk='Image::ExifTool 13.59'>\n\
             <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n \
             <rdf:Description rdf:about=''\n  xmlns:plus='{PLUS_NS}'>\n{properties}\n \
             </rdf:Description>\n</rdf:RDF>\n</x:xmpmeta>"
        )
    }

    fn element(code: &str) -> String {
        exiftool_packet(&format!(
            "  <plus:DataMining>{VOCAB}{code}</plus:DataMining>"
        ))
    }

    /// A minimal JPEG, with the packet in an APP1 XMP segment when given.
    fn jpeg(xmp: Option<&str>) -> Vec<u8> {
        let base = synth_jpeg([1; 64]);
        let mut v = vec![0xFF, 0xD8];
        if let Some(x) = xmp {
            let mut p = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
            p.extend_from_slice(x.as_bytes());
            v.extend_from_slice(&[0xFF, 0xE1]);
            v.extend_from_slice(&((p.len() + 2) as u16).to_be_bytes());
            v.extend_from_slice(&p);
        }
        v.extend_from_slice(&base[2..]);
        v
    }

    fn jpeg_with(xmp: &str) -> Vec<u8> {
        jpeg(Some(xmp))
    }

    fn png_with(xmp: &str) -> Vec<u8> {
        fn chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut v = (data.len() as u32).to_be_bytes().to_vec();
            v.extend_from_slice(ty);
            v.extend_from_slice(data);
            v.extend_from_slice(&[0; 4]); // CRC is not checked by the reader
            v
        }
        let mut itxt = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
        itxt.extend_from_slice(xmp.as_bytes());
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend(chunk(b"IHDR", &[0; 13]));
        png.extend(chunk(b"iTXt", &itxt));
        png.extend(chunk(b"IEND", &[]));
        png
    }

    fn assess(bytes: Vec<u8>) -> Evidence {
        let a = Asset::from_bytes(bytes, None).unwrap();
        DataMiningReservation.assess(&a).unwrap()
    }

    #[test]
    fn every_prohibiting_term_is_present_and_names_its_label() {
        for t in VOCABULARY.iter().filter(|t| t.kind == TermKind::Prohibited) {
            let ev = assess(jpeg_with(&element(t.code)));
            assert_eq!(ev.status, Status::Present, "{}", t.code);
            assert!(ev.rationale.contains(t.code) && ev.rationale.contains(t.label));
            assert!(ev.rationale.contains("reserves rights against data mining"));
            assert!(ev.rationale.contains("unsigned statement"));
            assert!(ev.rationale.contains("legal question"));
            assert_eq!(ev.details["data_mining"], serde_json::json!([t.code]));
            assert_eq!(ev.details["vocabulary_version"], VOCABULARY_VERSION);
        }
        assert_eq!(
            VOCABULARY
                .iter()
                .filter(|t| t.kind == TermKind::Prohibited)
                .count(),
            7
        );
    }

    #[test]
    fn allowed_and_unspecified_are_absent_with_the_value_reported() {
        for (code, label) in [
            ("DMI-ALLOWED", "allowed"),
            ("DMI-UNSPECIFIED", "unspecified, no prohibition defined"),
        ] {
            let ev = assess(png_with(&element(code)));
            assert_eq!(ev.status, Status::Absent, "{code}");
            assert!(
                ev.rationale.contains(&format!("{code} ({label})")),
                "{}",
                ev.rationale
            );
            assert!(
                ev.rationale
                    .contains("no reservation against data mining is stated")
            );
            assert_eq!(ev.details["data_mining"], serde_json::json!([code]));
        }
    }

    #[test]
    fn no_property_is_absent_and_says_absence_is_not_permission() {
        let none = assess(jpeg(None));
        assert_eq!(none.status, Status::Absent);
        assert!(none.rationale.contains("(no XMP)"));
        assert!(none.rationale.contains("Absence is not permission"));
        assert!(none.rationale.contains("C2PA assertion") && none.rationale.contains("TDMRep"));

        // XMP with a different PLUS property and the marking field, but no Data Mining.
        let other = assess(jpeg_with(&exiftool_packet(
            "  <plus:CreditLineRequired>x</plus:CreditLineRequired>\n  \
             <Iptc4xmpExt:DigitalSourceType>trainedAlgorithmicMedia</Iptc4xmpExt:DigitalSourceType>",
        )));
        assert_eq!(other.status, Status::Absent);
        assert!(
            other
                .rationale
                .contains("XMP present but without the property")
        );
        assert_eq!(other.details["data_mining"], serde_json::json!([]));
    }

    #[test]
    fn attribute_form_and_other_prefixes_are_read() {
        // ExifTool with -api XMPShorthand=1 writes the property as an attribute.
        let shorthand = "<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF \
            xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about='' \
            xmlns:plus='http://ns.useplus.org/ldf/xmp/1.0/' \
            plus:DataMining='http://ns.useplus.org/ldf/vocab/DMI-PROHIBITED-GENAIMLTRAINING'/>\
            </rdf:RDF></x:xmpmeta>";
        let ev = assess(jpeg_with(shorthand));
        assert_eq!(ev.status, Status::Present);
        assert_eq!(ev.details["values"][0]["form"], "attribute");
        assert_eq!(ev.details["prefix_not_bound_to_plus"], false);

        // Another prefix bound to the PLUS namespace, double quotes.
        let renamed = shorthand
            .replace("plus", "p")
            .replace('\'', "\"")
            .replace("http://ns.usep.org/ldf", "http://ns.useplus.org/ldf");
        let ev = assess(jpeg_with(&renamed));
        assert_eq!(ev.status, Status::Present, "{renamed}");
        assert_eq!(ev.details["values"][0]["prefix"], "p");
        assert_eq!(ev.details["prefix_not_bound_to_plus"], false);

        // A prefix bound to something else is still read, and flagged.
        let unbound = shorthand.replace(PLUS_NS, "http://example.org/other/");
        let ev = assess(jpeg_with(&unbound));
        assert_eq!(ev.status, Status::Present);
        assert_eq!(ev.details["prefix_not_bound_to_plus"], true);
    }

    #[test]
    fn the_code_is_the_value_minus_one_vocabulary_prefix_as_exiftool_reads_it() {
        assert_eq!(code_of("DMI-PROHIBITED"), "DMI-PROHIBITED");
        assert_eq!(code_of(&format!(" {VOCAB}DMI-ALLOWED ")), "DMI-ALLOWED");
        // A bare code is read like the URI.
        let bare = assess(jpeg_with(&exiftool_packet(
            "  <plus:DataMining>DMI-PROHIBITED</plus:DataMining>",
        )));
        assert_eq!(bare.status, Status::Present);

        // ExifTool 13.59 given the full URI as a raw value (`-DataMining#=<uri>`)
        // writes the prefix twice and reads the result back as "Unknown (…)". Not in
        // the vocabulary here either: reported verbatim, not interpreted.
        let doubled = format!("{VOCAB}{VOCAB}DMI-PROHIBITED-SEECONSTRAINT");
        let ev = assess(jpeg_with(&exiftool_packet(&format!(
            "  <plus:DataMining>{doubled}</plus:DataMining>"
        ))));
        assert_eq!(ev.status, Status::Inconclusive);
        assert!(
            ev.rationale
                .contains(&format!("{VOCAB}DMI-PROHIBITED-SEECONSTRAINT"))
        );
        assert_eq!(ev.details["values"][0]["raw"], doubled);

        // Another host or scheme is another vocabulary.
        for other in [
            "https://ns.useplus.org/ldf/vocab/DMI-PROHIBITED",
            "http://example.org/DMI-PROHIBITED",
        ] {
            let ev = assess(jpeg_with(&exiftool_packet(&format!(
                "  <plus:DataMining>{other}</plus:DataMining>"
            ))));
            assert_eq!(ev.status, Status::Inconclusive, "{other}");
        }
    }

    #[test]
    fn see_constraint_quotes_the_other_constraints_text_or_says_it_is_missing() {
        let with_text = exiftool_packet(&format!(
            "  <plus:DataMining>{VOCAB}DMI-PROHIBITED-SEECONSTRAINT</plus:DataMining>\n  \
             <plus:OtherConstraints>\n   <rdf:Alt>\n    \
             <rdf:li xml:lang='x-default'>Research use only, ask first</rdf:li>\n   \
             </rdf:Alt>\n  </plus:OtherConstraints>"
        ));
        let ev = assess(jpeg_with(&with_text));
        assert_eq!(ev.status, Status::Present);
        assert_eq!(
            ev.details["other_constraints"],
            "Research use only, ask first"
        );
        assert!(
            ev.rationale
                .contains("Its Other Constraints text reads: \"Research use only, ask first\"."),
            "{}",
            ev.rationale
        );

        let without = assess(jpeg_with(&element("DMI-PROHIBITED-SEECONSTRAINT")));
        assert!(without.rationale.contains("which the file does not carry"));
        assert_eq!(
            without.details["other_constraints"],
            serde_json::Value::Null
        );

        // The text alone states nothing in the vocabulary: no status from it.
        let only_text = exiftool_packet(
            "  <plus:OtherConstraints><rdf:Alt><rdf:li xml:lang='x-default'>No AI</rdf:li>\
             </rdf:Alt></plus:OtherConstraints>",
        );
        let ev = assess(jpeg_with(&only_text));
        assert_eq!(ev.status, Status::Absent);
        assert_eq!(ev.details["other_constraints"], "No AI");

        let expr = assess(jpeg_with(&element("DMI-PROHIBITED-SEELINKEDRIGHTSEXPR")));
        assert!(expr.rationale.contains("which this source does not read"));
    }

    #[test]
    fn unknown_and_conflicting_values_are_inconclusive() {
        let unknown = assess(jpeg_with(&element("DMI-ALLOWED-noaitraining")));
        assert_eq!(unknown.status, Status::Inconclusive);
        assert!(unknown.rationale.contains("DMI-ALLOWED-noaitraining"));
        assert!(
            unknown
                .rationale
                .contains("reported verbatim, not interpreted")
        );

        let both = exiftool_packet(&format!(
            "  <plus:DataMining>{VOCAB}DMI-ALLOWED</plus:DataMining>\n  \
             <plus:DataMining>{VOCAB}DMI-PROHIBITED</plus:DataMining>"
        ));
        let ev = assess(jpeg_with(&both));
        assert_eq!(ev.status, Status::Inconclusive);
        assert!(
            ev.rationale
                .contains("conflicting Data Mining values (DMI-ALLOWED, DMI-PROHIBITED)")
        );

        // Two prohibiting terms agree on the question asked: Present, both named.
        let two = exiftool_packet(&format!(
            "  <plus:DataMining>{VOCAB}DMI-PROHIBITED-AIMLTRAINING</plus:DataMining>\n  \
             <plus:DataMining>{VOCAB}DMI-PROHIBITED</plus:DataMining>"
        ));
        let ev = assess(jpeg_with(&two));
        assert_eq!(ev.status, Status::Present);
        assert!(ev.rationale.contains("DMI-PROHIBITED-AIMLTRAINING (prohibited for AI/ML training), DMI-PROHIBITED (prohibited)"));
    }

    #[test]
    fn longer_names_closing_tags_and_empty_values_are_not_values() {
        let noise = exiftool_packet(
            "  <plus:DataMiningPolicy>x</plus:DataMiningPolicy>\n  <plus:DataMining/>\n  \
             <plus:DataMining></plus:DataMining>\n  <dc:description>about DataMining</dc:description>",
        );
        let ev = assess(jpeg_with(&noise));
        assert_eq!(ev.status, Status::Absent);
        assert_eq!(ev.details["values"], serde_json::json!([]));
    }

    #[test]
    fn only_the_four_image_containers_are_supported() {
        let mp4 = Asset::from_bytes(b"\0\0\0\x18ftypmp42\0\0\0\0mp42isom".to_vec(), None).unwrap();
        assert!(!DataMiningReservation.supports(&mp4));
        let jpeg = Asset::from_bytes(jpeg(None), None).unwrap();
        assert!(DataMiningReservation.supports(&jpeg));
    }

    #[test]
    fn hostile_input_errors_instead_of_panicking() {
        let ok = jpeg_with(&element("DMI-PROHIBITED"));
        for n in 0..ok.len() {
            let _ = scan("image/jpeg", &ok[..n]);
        }
        assert!(scan("image/png", b"\x89PNG\r\n\x1a\n\0\0\0").is_ok_and(|s| s.values.is_empty()));
        assert!(scan("image/gif", b"GIF89a").is_err());
    }
}
