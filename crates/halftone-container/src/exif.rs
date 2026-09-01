//! EXIF / XMP consistency: metadata contradictions and self-identifying AI metadata.
//!
//! Two independent things are checked. (1) Self-identification: EXIF `Software`, XMP,
//! JPEG comments and PNG text often *name* the generator or editor outright — a
//! near-provenance signal. (2) Contradiction: a dedicated-camera `Make` with the
//! MakerNote stripped, on a file that a software JPEG encoder re-wrote, does not match
//! a camera-native capture — the metadata was re-saved or transplanted. Contradictions
//! are reported as `Inconclusive`, never as a positive origin claim, to keep false
//! positives (legitimately edited or messaged photos) from turning into accusations.

use std::io::Cursor;

use exif::{In, Tag};
use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};

use crate::{jpeg, png, signatures};

/// The metadata this source extracts from an asset, across containers.
#[derive(Debug, Default)]
struct MetaScan {
    make: Option<String>,
    model: Option<String>,
    software: Option<String>,
    has_makernote: bool,
    has_exif: bool,
    has_xmp: bool,
    /// Combined free-text haystack for signature matching.
    haystack: String,
}

fn field(exif: &exif::Exif, tag: Tag) -> Option<String> {
    exif.get_field(tag, In::PRIMARY)
        .map(|f| f.display_value().to_string().trim().trim_matches('"').trim().to_string())
        .filter(|s| !s.is_empty())
}

fn scan(a: &Asset) -> MetaScan {
    let mut m = MetaScan::default();
    let mut hay = String::new();

    // EXIF via kamadak (works for JPEG and PNG containers with an Exif block).
    if let Ok(exif) = exif::Reader::new().read_from_container(&mut Cursor::new(a.bytes.as_slice())) {
        m.has_exif = true;
        m.make = field(&exif, Tag::Make);
        m.model = field(&exif, Tag::Model);
        m.software = field(&exif, Tag::Software);
        m.has_makernote = exif.get_field(Tag::MakerNote, In::PRIMARY).is_some();
        for tag in [Tag::Software, Tag::ImageDescription, Tag::UserComment] {
            if let Some(v) = field(&exif, tag) {
                hay.push(' ');
                hay.push_str(&v);
            }
        }
    }

    // Container-specific free text: JPEG XMP + comment, or PNG text chunks.
    match a.mime.as_str() {
        "image/jpeg" => {
            if let Ok(s) = jpeg::parse_structure(&a.bytes) {
                m.has_xmp = s.has_xmp;
                if let Some(x) = &s.xmp {
                    hay.push(' ');
                    hay.push_str(x);
                }
                if let Some(c) = &s.comment {
                    hay.push(' ');
                    hay.push_str(c);
                }
            }
        }
        "image/png" => {
            if let Ok(info) = png::parse_png(&a.bytes) {
                for t in &info.text {
                    hay.push(' ');
                    hay.push_str(&t.keyword);
                    hay.push(' ');
                    hay.push_str(&t.value);
                }
            }
        }
        _ => {}
    }

    m.haystack = hay;
    m
}

/// Is this file a software JPEG re-encode (libjpeg-family or Adobe)?
fn is_software_reencode(a: &Asset) -> bool {
    if a.mime != "image/jpeg" {
        return false;
    }
    jpeg::parse_structure(&a.bytes)
        .map(|s| !matches!(jpeg::classify_encoder(&s), jpeg::EncoderClass::NonStandard))
        .unwrap_or(false)
}

/// EXIF/XMP consistency evidence source.
///
/// `Present` only when metadata explicitly names a generation tool. `Inconclusive` for
/// a suggestive contradiction or a named editor. `Absent` when metadata is internally
/// consistent or simply absent (which is unremarkable and not evidence of origin).
#[derive(Debug, Default)]
pub struct ExifConsistency;

impl EvidenceSource for ExifConsistency {
    fn id(&self) -> SourceId {
        SourceId { name: "exif_consistency".into(), version: env!("CARGO_PKG_VERSION").into() }
    }
    fn layer(&self) -> Layer {
        Layer::Container
    }
    fn supports(&self, a: &Asset) -> bool {
        a.modality == Modality::Image && matches!(a.mime.as_str(), "image/jpeg" | "image/png")
    }
    fn assess(&self, a: &Asset) -> halftone_core::Result<Evidence> {
        let m = scan(a);
        let generator = signatures::find_generator(&m.haystack);
        let editor = signatures::find_editor(&m.haystack);
        let camera_make = m.make.as_deref().filter(|mk| signatures::is_camera_make(mk));

        let (status, rationale) = if let Some(tool) = generator {
            (
                Status::Present,
                format!("Metadata explicitly names a generation tool: {tool}."),
            )
        } else if let Some(make) = camera_make {
            if !m.has_makernote && is_software_reencode(a) {
                (
                    Status::Inconclusive,
                    format!(
                        "Make '{make}' claims a camera, but the MakerNote is absent and the file \
                         was re-encoded by a software JPEG encoder — inconsistent with a \
                         camera-native capture (re-saved or transplanted EXIF)."
                    ),
                )
            } else {
                (
                    Status::Absent,
                    format!(
                        "Camera metadata is internally consistent (make '{make}'{}).",
                        if m.has_makernote { ", MakerNote present" } else { "" }
                    ),
                )
            }
        } else if let Some(ed) = editor {
            (
                Status::Inconclusive,
                format!("Metadata names an editor ({ed}); consistent with an edited image."),
            )
        } else if m.has_exif || m.has_xmp {
            (
                Status::Absent,
                "Metadata present but names no generator; no camera/metadata contradiction found."
                    .to_string(),
            )
        } else {
            (
                Status::Absent,
                "No generation or camera metadata found; absence is common and not evidence of \
                 origin."
                    .to_string(),
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
                "make": m.make,
                "model": m.model,
                "software": m.software,
                "has_makernote": m.has_makernote,
                "has_exif": m.has_exif,
                "has_xmp": m.has_xmp,
                "generator": generator,
                "editor": editor,
            }),
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_in_png_text_is_present() {
        // PNG with a Software=Midjourney tEXt chunk.
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&16u32.to_be_bytes());
        ihdr.extend_from_slice(&16u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        v.extend(png_chunk(b"IHDR", &ihdr));
        let mut text = b"Software\0".to_vec();
        text.extend_from_slice(b"Midjourney v6");
        v.extend(png_chunk(b"tEXt", &text));
        v.extend(png_chunk(b"IEND", &[]));

        let a = Asset::from_bytes(v, None).unwrap();
        let ev = ExifConsistency.assess(&a).unwrap();
        assert_eq!(ev.status, Status::Present);
        assert!(ev.rationale.contains("Midjourney"));
    }

    #[test]
    fn no_metadata_is_absent_not_error() {
        // Minimal JPEG with libjpeg tables but no EXIF/XMP.
        let a = Asset::from_bytes(minimal_jpeg(), None).unwrap();
        let ev = ExifConsistency.assess(&a).unwrap();
        assert_eq!(ev.status, Status::Absent);
    }

    fn png_chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = (data.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(ty);
        v.extend_from_slice(data);
        v.extend_from_slice(&[0, 0, 0, 0]);
        v
    }

    fn minimal_jpeg() -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x43, 0x00];
        v.extend_from_slice(&[1u8; 64]);
        v.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x10, 0x00, 0x10, 0x03]);
        v.extend_from_slice(&[0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
        v
    }
}
