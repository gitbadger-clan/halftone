//! The Live Photo evidence source (`live_photo`): one question, "does this still say
//! it was saved with a Live Photo clip?", answered from one field.
//!
//! A Live Photo is two files: a still (HEIC or JPEG) and a short video clip (MOV).
//! Nothing in the still's container points at the clip. What ties them together is
//! an identifier in Apple's MakerNote, `ContentIdentifier`
//! ([`crate::apple_makernote`]), which the clip is meant to repeat in its own
//! metadata. This source reads the still's side of that and nothing else.
//!
//! Measured on six camera files of one iPad (iPadOS 18.7.3, 06-phones, 2026-11-26):
//! the identifier is in the two files shot with Live Photo on, one HEIC and one JPEG,
//! and in none of the four shot with it off. The clips were not collected, so that a
//! clip carries the same identifier is Apple's design as documented, not something
//! checked here.
//!
//! Companion to [`crate::gcontainer::ContainerDirectory`], which reads the Motion
//! Photo that Android cameras write. There the clip is inside the JPEG and can be
//! checked against the bytes; here it is another file, so the most this source can
//! say is that the still claims one.
//!
//! Status semantics:
//! - `Present`: the identifier is there.
//! - `Absent`: no Apple MakerNote, or one without the identifier. Not a statement
//!   that no clip ever existed: a copy that lost its EXIF block reads the same.
//! - `Inconclusive`: Apple's MakerNote is there, part of it could not be read, and
//!   the identifier was not found in the rest.
//!
//! The identifier's value is not repeated here. `embedded_identifiers` lists it,
//! masked, with the file's other identifiers.

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};

use crate::embedded::Embedded;

/// The Live Photo evidence source (`live_photo`).
#[derive(Debug, Default)]
pub struct LivePhoto;

impl EvidenceSource for LivePhoto {
    fn id(&self) -> SourceId {
        SourceId {
            name: "live_photo".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
    fn layer(&self) -> Layer {
        Layer::Container
    }
    fn supports(&self, a: &Asset) -> bool {
        // The two formats an Apple camera saves a still in.
        a.modality == Modality::Image && matches!(a.mime.as_str(), "image/jpeg" | "image/heif")
    }
    fn assess(&self, a: &Asset) -> halftone_core::Result<Evidence> {
        let e = Embedded::read(a).map_err(halftone_core::Error::Parse)?;
        let note = e.apple_makernote();
        const UNSIGNED: &str = "The field is unsigned camera metadata and travels with the \
            EXIF block: a copy that lost its EXIF says nothing about this either way.";
        let (status, rationale) = match &note {
            Some(m) if m.content_identifier.is_some() => (
                Status::Present,
                format!(
                    "Apple's MakerNote carries a content identifier. Apple's cameras write it \
                     when a still is saved as a Live Photo: the still, and a short video clip \
                     kept as a separate file that is meant to carry the same identifier. This \
                     file is the still. The clip was not looked for, and nothing here says \
                     whether it still exists or where. {UNSIGNED}"
                ),
            ),
            Some(m) if m.incomplete() => (
                Status::Inconclusive,
                "Apple's MakerNote is present, but part of it could not be read, and no content \
                 identifier was found in the rest. Whether this still was saved with a Live \
                 Photo clip cannot be said from this file."
                    .to_string(),
            ),
            Some(_) => (
                Status::Absent,
                format!(
                    "Apple's MakerNote is present and has no content identifier: nothing in \
                     this file says it was saved with a Live Photo clip. That is what the \
                     camera writes with Live Photo off (measured on one iPad). {UNSIGNED}"
                ),
            ),
            None => (
                Status::Absent,
                "No Apple MakerNote, which is where a Live Photo still keeps the identifier it \
                 shares with its clip. Most files have none; a file from an Apple camera whose \
                 EXIF block was removed reads the same. Other makers' formats are not looked \
                 for here (container_directory reads the Motion Photo clip inside a JPEG)."
                    .to_string(),
            ),
        };
        Ok(Evidence {
            layer: self.layer(),
            source: self.id(),
            status,
            statistic: None,
            calibration: None,
            rationale,
            details: serde_json::json!({
                "apple_makernote": match &note {
                    None => "absent",
                    Some(m) if m.incomplete() => "incomplete",
                    Some(_) => "read",
                },
                "content_identifier": note.as_ref().is_some_and(|m| m.content_identifier.is_some()),
                "makernote_entries": note.as_ref().map(|m| m.entries),
            }),
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apple_makernote::testutil::{CONTENT, camera};
    use crate::embedded::testutil::{exif, exif_with, jpeg};

    fn assess(bytes: Vec<u8>) -> Evidence {
        let a = Asset::from_bytes(bytes, None).unwrap();
        assert!(LivePhoto.supports(&a));
        LivePhoto.assess(&a).unwrap()
    }

    fn with_makernote(block: &[u8]) -> Vec<u8> {
        jpeg(
            None,
            Some(&exif_with(&[(0x010F, "Apple")], &[], &[(0x927C, block)])),
            &[],
        )
    }

    #[test]
    fn a_still_saved_with_a_clip_is_present_and_keeps_the_value_to_itself() {
        let ev = assess(with_makernote(&camera(true)));
        assert_eq!(ev.status, Status::Present);
        assert_eq!(ev.details["content_identifier"], true);
        assert_eq!(ev.details["apple_makernote"], "read");
        assert_eq!(ev.details["makernote_entries"], 6);
        // The claim is the still's, the clip is not in the file, and it is unsigned.
        for said in ["meant to carry", "was not looked for", "unsigned"] {
            assert!(ev.rationale.contains(said), "{said}: {}", ev.rationale);
        }
        // The value is the identifiers source's to list, masked.
        let all = serde_json::to_string(&ev.details).unwrap() + &ev.rationale;
        assert!(!all.contains(CONTENT), "{all}");
    }

    #[test]
    fn a_still_without_the_identifier_is_absent_for_one_of_two_reasons() {
        // Apple's block, no identifier: Live Photo off.
        let ev = assess(with_makernote(&camera(false)));
        assert_eq!(ev.status, Status::Absent);
        assert_eq!(ev.details["apple_makernote"], "read");
        assert_eq!(ev.details["content_identifier"], false);
        assert!(ev.rationale.contains("Live Photo off"), "{}", ev.rationale);

        // No MakerNote, another maker's, and no EXIF at all: nowhere to look.
        for bytes in [
            jpeg(None, Some(&exif(&[(0x010F, "Apple")], &[])), &[]),
            jpeg(
                None,
                Some(&exif_with(
                    &[],
                    &[],
                    &[(0x927C, b"Nikon\0\x02\x10\0\0MM\0\x2A")],
                )),
                &[],
            ),
            jpeg(None, None, &[]),
        ] {
            let ev = assess(bytes);
            assert_eq!(ev.status, Status::Absent);
            assert_eq!(ev.details["apple_makernote"], "absent");
            assert_eq!(ev.details["makernote_entries"], serde_json::Value::Null);
            assert!(
                ev.rationale.starts_with("No Apple MakerNote"),
                "{}",
                ev.rationale
            );
            // Absence is not a statement about a clip, and the Android format is
            // somebody else's to read.
            assert!(
                ev.rationale.contains("container_directory"),
                "{}",
                ev.rationale
            );
        }
    }

    #[test]
    fn a_damaged_block_is_inconclusive_unless_the_identifier_was_read() {
        let whole = camera(true);
        // Cut before the identifier's entry: not found, and not called absent.
        let ev = assess(with_makernote(&whole[..40]));
        assert_eq!(ev.status, Status::Inconclusive, "{}", ev.rationale);
        assert_eq!(ev.details["apple_makernote"], "incomplete");
        assert_eq!(ev.details["content_identifier"], false);

        // The identifier itself was read and something after it was not: the still
        // still says what it says.
        let mut tail_gone = whole.clone();
        let last = tail_gone.len() - 20;
        tail_gone.truncate(last);
        let ev = assess(with_makernote(&tail_gone));
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
        assert_eq!(ev.details["apple_makernote"], "incomplete");
    }

    #[test]
    fn only_the_two_formats_an_apple_camera_writes_are_looked_at() {
        let png = crate::embedded::testutil::png(&[]);
        let a = Asset::from_bytes(png, None).unwrap();
        assert!(!LivePhoto.supports(&a));
    }
}
