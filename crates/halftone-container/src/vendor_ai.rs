//! Vendor AI flags: fields outside the IPTC vocabulary in which an application states
//! that its output contains AI-generated content.
//!
//! One source, one question: does the file's XMP carry a known vendor-specific
//! statement that the content is AI-generated? `marking_metadata` reports the IPTC
//! `DigitalSourceType` and nothing else; a vendor's own yes/no field is a different
//! statement, in a different vocabulary, and is reported here so it is not lost.
//!
//! Only fields in [`FIELDS`] are looked for. That list holds what has been measured in
//! the corpus, not what might exist: a field is added when a real file carries it.
//!
//! Status semantics:
//! - `Present`: a known field says yes.
//! - `Absent`: none of the known fields is there, or one says no (value in `details`).
//! - `Inconclusive`: a value that is neither yes nor no, yes and no together, or XMP
//!   that could not be read completely. Reported verbatim, never interpreted.
//!
//! Unsigned, like every XMP field: it records what the writing application stated.

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
use serde::Serialize;

use crate::embedded::Embedded;

/// A vendor field this build knows.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Field {
    /// Local name of the XMP property (any prefix).
    pub name: &'static str,
    /// What the field is, for the rationale.
    pub about: &'static str,
}

/// The known vendor AI fields. Extend only with fields measured on real files.
pub const FIELDS: &[Field] = &[Field {
    name: "ContainsAiGeneratedContent",
    about: "a yes/no statement by the writing application that the file contains AI-generated \
            content",
}];

/// One occurrence of a known field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Flag {
    /// The field's local name.
    pub field: &'static str,
    /// Prefix as written.
    pub prefix: String,
    /// Namespace URI the prefix is bound to, when the packet declares it.
    pub namespace: Option<String>,
    /// The value as written.
    pub value: String,
    /// `Some(true)` for yes, `Some(false)` for no, `None` for anything else.
    pub says: Option<bool>,
}

/// Yes/no reading of a flag value: `yes`/`true`/`1` and `no`/`false`/`0`, in any
/// letter case. Anything else is not interpreted.
pub fn says(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "1" => Some(true),
        "no" | "false" | "0" => Some(false),
        _ => None,
    }
}

/// Vendor AI flag evidence source (`vendor_ai_flag`).
#[derive(Debug, Default)]
pub struct VendorAiFlag;

impl EvidenceSource for VendorAiFlag {
    fn id(&self) -> SourceId {
        SourceId {
            name: "vendor_ai_flag".into(),
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
        let mut flags: Vec<Flag> = Vec::new();
        for f in FIELDS {
            for v in e.xmp_any(f.name) {
                flags.push(Flag {
                    field: f.name,
                    says: says(&v.value),
                    prefix: v.prefix,
                    namespace: v.namespace,
                    value: v.value,
                });
            }
        }
        let yes = flags.iter().any(|f| f.says == Some(true));
        let no = flags.iter().any(|f| f.says == Some(false));
        let unknown: Vec<&Flag> = flags.iter().filter(|f| f.says.is_none()).collect();
        let known: Vec<&str> = FIELDS.iter().map(|f| f.name).collect();

        const CAVEAT: &str = "This is an unsigned statement by whatever software last saved the \
            file: it records what that software stated, verifies nothing about how the content \
            was made, and is removed by any re-save or messaging path that strips XMP.";
        let shown = |f: &Flag| {
            let ns = f
                .namespace
                .as_deref()
                .map(|n| format!(", namespace {n}"))
                .unwrap_or_default();
            format!("{}:{} = {}{ns}", f.prefix, f.field, f.value)
        };
        let (status, rationale) = if flags.is_empty() {
            let unread = e.xmp_incomplete();
            (
                if unread {
                    Status::Inconclusive
                } else {
                    Status::Absent
                },
                format!(
                    "No known vendor AI field found ({}). Looked for: {}. Only fields this \
                     build knows are recognised, so another vendor's field, or the same \
                     statement in the IPTC vocabulary (marking_metadata) or in a signed \
                     manifest, is not reported here. Absence says nothing about origin.",
                    if unread {
                        "some XMP could not be read"
                    } else if e.xmp.is_empty() {
                        "no XMP"
                    } else {
                        "XMP present but without such a field"
                    },
                    known.join(", ")
                ),
            )
        } else if !unknown.is_empty() {
            (
                Status::Inconclusive,
                format!(
                    "XMP carries a vendor AI field with a value that is neither yes nor no ({}): \
                     reported verbatim, not interpreted.",
                    shown(unknown[0])
                ),
            )
        } else if yes && no {
            (
                Status::Inconclusive,
                format!(
                    "XMP carries vendor AI fields that say both yes and no; not interpreted. \
                     {CAVEAT}"
                ),
            )
        } else if yes {
            let f = flags
                .iter()
                .find(|f| f.says == Some(true))
                .unwrap_or(&flags[0]);
            (
                Status::Present,
                format!(
                    "XMP carries the vendor field {}: the writing application states that the \
                     file contains AI-generated content. This field is not part of the IPTC \
                     DigitalSourceType vocabulary, so marking_metadata does not report it. \
                     {CAVEAT}",
                    shown(f)
                ),
            )
        } else {
            (
                Status::Absent,
                format!(
                    "XMP carries the vendor field {}: the writing application states that the \
                     file does not contain AI-generated content. {CAVEAT}",
                    shown(&flags[0])
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
                "known_fields": known,
                "flags": flags,
                "xmp_packets": e.xmp.len(),
                "xmp_incomplete": e.xmp_incomplete(),
            }),
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedded::testutil::{jpeg, packet};

    fn assess(xmp: Option<&str>) -> Evidence {
        let a = Asset::from_bytes(jpeg(xmp, None, &[]), None).unwrap();
        VendorAiFlag.assess(&a).unwrap()
    }

    fn flag(value: &str) -> String {
        packet(
            "xmlns:ContainsAiGeneratedContent='https://example.test/export'",
            &format!(
                "<ContainsAiGeneratedContent:ContainsAiGeneratedContent>{value}\
                 </ContainsAiGeneratedContent:ContainsAiGeneratedContent>"
            ),
        )
    }

    #[test]
    fn yes_is_present_and_names_field_value_and_namespace() {
        let ev = assess(Some(&flag("Yes")));
        assert_eq!(ev.status, Status::Present);
        assert!(ev.rationale.contains(
            "ContainsAiGeneratedContent:ContainsAiGeneratedContent = Yes, namespace \
             https://example.test/export"
        ));
        assert!(ev.rationale.contains("marking_metadata does not report it"));
        assert!(ev.rationale.contains("unsigned statement"));
        assert_eq!(ev.details["flags"][0]["says"], true);
        assert_eq!(
            ev.details["flags"][0]["namespace"],
            "https://example.test/export"
        );
    }

    #[test]
    fn no_is_absent_with_the_value_and_other_values_are_inconclusive() {
        let no = assess(Some(&flag("No")));
        assert_eq!(no.status, Status::Absent);
        assert!(
            no.rationale
                .contains("does not contain AI-generated content")
        );
        assert_eq!(no.details["flags"][0]["says"], false);

        let odd = assess(Some(&flag("Partially")));
        assert_eq!(odd.status, Status::Inconclusive);
        assert!(odd.rationale.contains("= Partially"));
        assert!(odd.rationale.contains("not interpreted"));
    }

    #[test]
    fn attribute_form_any_prefix_and_letter_case() {
        let x = packet(
            "xmlns:v='urn:vendor' v:ContainsAiGeneratedContent='TRUE'",
            "",
        );
        let ev = assess(Some(&x));
        assert_eq!(ev.status, Status::Present);
        assert_eq!(ev.details["flags"][0]["prefix"], "v");
        for (v, want) in [
            ("yes", Some(true)),
            ("0", Some(false)),
            ("maybe", None),
            ("", None),
        ] {
            assert_eq!(says(v), want, "{v}");
        }
    }

    #[test]
    fn nothing_found_is_absent_and_names_what_was_looked_for() {
        for xmp in [
            None,
            Some(packet("xmlns:dc='urn:x'", "<dc:title>t</dc:title>")),
        ] {
            let ev = assess(xmp.as_deref());
            assert_eq!(ev.status, Status::Absent);
            assert!(
                ev.rationale
                    .contains("Looked for: ContainsAiGeneratedContent")
            );
            assert!(ev.rationale.contains("Absence says nothing about origin"));
            assert_eq!(ev.details["flags"], serde_json::json!([]));
        }
    }

    #[test]
    fn yes_and_no_together_are_inconclusive() {
        let x = packet(
            "xmlns:a='urn:a' xmlns:b='urn:b'",
            "<a:ContainsAiGeneratedContent>Yes</a:ContainsAiGeneratedContent>\
             <b:ContainsAiGeneratedContent>No</b:ContainsAiGeneratedContent>",
        );
        assert_eq!(assess(Some(&x)).status, Status::Inconclusive);
    }
}
