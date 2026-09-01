//! EXIF / XMP consistency: metadata contradictions and self-identifying AI metadata.
//!
//! Three independent things are checked. (1) Self-identification: EXIF `Software`, XMP,
//! JPEG comments and PNG/WebP text often *name* the generator or editor outright — a
//! near-provenance signal. (2) Camera contradiction: a camera `Make` with the
//! MakerNote and/or embedded thumbnail stripped, on a file that a software JPEG encoder
//! re-wrote, does not match a camera-native capture — the metadata was re-saved or
//! transplanted. (3) Dimension contradiction: the EXIF-declared pixel dimensions differ
//! from the actual frame — the image was resized/cropped after the metadata was
//! written, or the metadata came from another file. Contradictions are reported as
//! `Inconclusive`, never as a positive origin claim, so legitimately edited or messaged
//! photos are not turned into accusations.

use std::io::Cursor;

use exif::{In, Tag};
use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};

use crate::{jpeg, png, signatures, webp};

/// The metadata this source extracts from an asset, across containers.
#[derive(Debug, Default)]
struct MetaScan {
    make: Option<String>,
    model: Option<String>,
    software: Option<String>,
    has_makernote: bool,
    has_thumbnail: bool,
    has_exif: bool,
    /// EXIF was malformed and only partially parsed (sloppy writer: piexif-style).
    exif_partial: bool,
    has_xmp: bool,
    /// EXIF-declared dimensions (PixelXDimension/PixelYDimension or ImageWidth/Length).
    exif_dims: Option<(u32, u32)>,
    /// Actual frame dimensions from the container.
    frame_dims: Option<(u32, u32)>,
    /// Combined free-text haystack for signature matching.
    haystack: String,
}

fn field(exif: &exif::Exif, tag: Tag) -> Option<String> {
    exif.get_field(tag, In::PRIMARY)
        .map(|f| {
            f.display_value()
                .to_string()
                .trim()
                .trim_matches('"')
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
}

fn uint(exif: &exif::Exif, tag: Tag) -> Option<u32> {
    exif.get_field(tag, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
}

fn scan(a: &Asset) -> MetaScan {
    let mut m = MetaScan::default();
    let mut hay = String::new();

    // EXIF via kamadak (JPEG, PNG eXIf, WebP EXIF, HEIF). Sloppy writers (piexif and
    // friends omit IFD terminators) are common in generation pipelines, so keep partial
    // results instead of discarding all metadata.
    let mut reader = exif::Reader::new();
    reader.continue_on_error(true);
    let parsed = match reader.read_from_container(&mut Cursor::new(a.bytes.as_slice())) {
        Ok(e) => Some(e),
        Err(exif::Error::PartialResult(pr)) => {
            m.exif_partial = true;
            Some(pr.into_inner().0)
        }
        Err(_) => None,
    };
    if let Some(exif) = parsed {
        m.has_exif = true;
        m.make = field(&exif, Tag::Make);
        m.model = field(&exif, Tag::Model);
        m.software = field(&exif, Tag::Software);
        m.has_makernote = exif.get_field(Tag::MakerNote, In::PRIMARY).is_some();
        m.has_thumbnail = exif
            .get_field(Tag::JPEGInterchangeFormat, In::THUMBNAIL)
            .is_some();
        let w = uint(&exif, Tag::PixelXDimension).or_else(|| uint(&exif, Tag::ImageWidth));
        let h = uint(&exif, Tag::PixelYDimension).or_else(|| uint(&exif, Tag::ImageLength));
        if let (Some(w), Some(h)) = (w, h) {
            if w > 0 && h > 0 {
                m.exif_dims = Some((w, h));
            }
        }
        for tag in [
            Tag::Software,
            Tag::ImageDescription,
            Tag::UserComment,
            Tag::Artist,
            Tag::Copyright,
        ] {
            if let Some(v) = field(&exif, tag) {
                hay.push(' ');
                hay.push_str(&v);
            }
        }
    }

    // Container-specific free text and frame dimensions.
    match a.mime.as_str() {
        "image/jpeg" => {
            if let Ok(s) = jpeg::parse_structure(&a.bytes) {
                m.has_xmp = s.has_xmp;
                m.frame_dims = Some((s.width as u32, s.height as u32));
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
                m.frame_dims = Some((info.width, info.height));
                for t in &info.text {
                    if t.keyword == "XML:com.adobe.xmp" {
                        m.has_xmp = true;
                    }
                    hay.push(' ');
                    hay.push_str(&t.keyword);
                    hay.push(' ');
                    hay.push_str(&t.value);
                }
            }
        }
        "image/webp" => {
            if let Ok(info) = webp::parse_webp(&a.bytes) {
                m.has_xmp = info.has_xmp;
                if let Some(x) = &info.xmp {
                    hay.push(' ');
                    hay.push_str(x);
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

/// Dimension contradiction, allowing for EXIF orientation swaps.
fn dims_conflict(exif: (u32, u32), frame: (u32, u32)) -> bool {
    exif != frame && (exif.1, exif.0) != frame
}

/// EXIF/XMP consistency evidence source.
///
/// `Present` only when metadata explicitly names a generation tool. `Inconclusive` for
/// a contradiction or a named editor. `Absent` when metadata is internally consistent
/// or simply absent (which is unremarkable and not evidence of origin).
#[derive(Debug, Default)]
pub struct ExifConsistency;

impl EvidenceSource for ExifConsistency {
    fn id(&self) -> SourceId {
        SourceId {
            name: "exif_consistency".into(),
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
        let m = scan(a);
        let generator = signatures::find_generator(&m.haystack);
        let editor = signatures::find_editor(&m.haystack);
        let camera_make = m
            .make
            .as_deref()
            .filter(|mk| signatures::is_camera_make(mk));
        let dim_conflict = match (m.exif_dims, m.frame_dims) {
            (Some(e), Some(f)) => dims_conflict(e, f),
            _ => false,
        };
        let stripped_camera = camera_make.is_some() && !m.has_makernote && is_software_reencode(a);

        let (status, rationale) = if let Some(tool) = generator {
            (
                Status::Present,
                format!("Metadata explicitly names a generation tool: {tool}."),
            )
        } else if dim_conflict {
            let (ew, eh) = m.exif_dims.unwrap_or((0, 0));
            let (fw, fh) = m.frame_dims.unwrap_or((0, 0));
            (
                Status::Inconclusive,
                format!(
                    "EXIF declares {ew}×{eh} but the frame is {fw}×{fh}: the image was resized or \
                     cropped after its metadata was written, or the metadata was transplanted from \
                     another file.{}",
                    if stripped_camera {
                        " Camera make present with MakerNote stripped as well."
                    } else {
                        ""
                    }
                ),
            )
        } else if let Some(make) = camera_make {
            if stripped_camera {
                (
                    Status::Inconclusive,
                    format!(
                        "Make '{make}' claims a camera, but the MakerNote is absent{} and the file \
                         was re-encoded by a software JPEG encoder — inconsistent with a \
                         camera-native capture (re-saved or transplanted EXIF).",
                        if m.has_thumbnail { "" } else { ", no embedded thumbnail" }
                    ),
                )
            } else {
                (
                    Status::Absent,
                    format!(
                        "Camera metadata is internally consistent (make '{make}'{}{}).",
                        if m.has_makernote {
                            ", MakerNote present"
                        } else {
                            ""
                        },
                        if m.has_thumbnail {
                            ", thumbnail present"
                        } else {
                            ""
                        }
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
                "has_thumbnail": m.has_thumbnail,
                "has_exif": m.has_exif,
                "exif_partial": m.exif_partial,
                "has_xmp": m.has_xmp,
                "exif_dims": m.exif_dims,
                "frame_dims": m.frame_dims,
                "dimension_conflict": dim_conflict,
                "camera_metadata_stripped": stripped_camera,
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
        let a = Asset::from_bytes(crate::jpeg::testutil::synth_jpeg([1; 64]), None).unwrap();
        let ev = ExifConsistency.assess(&a).unwrap();
        assert_eq!(ev.status, Status::Absent);
    }

    #[test]
    fn dims_conflict_respects_orientation_swap() {
        assert!(!dims_conflict((4000, 3000), (4000, 3000)));
        assert!(!dims_conflict((3000, 4000), (4000, 3000)));
        assert!(dims_conflict((4000, 3000), (2000, 1500)));
    }

    fn png_chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = (data.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(ty);
        v.extend_from_slice(data);
        v.extend_from_slice(&[0, 0, 0, 0]);
        v
    }
}
