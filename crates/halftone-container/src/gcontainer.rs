//! The container-directory evidence source (`container_directory`): one question,
//! "does this file contain what its own Motion Photo / Ultra HDR container
//! directory declares?", answered from the bytes alone, with or without a C2PA
//! manifest. The directory parser and layout rule live in
//! [`halftone_formats::gcontainer`] and are re-exported here.

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
pub use halftone_formats::gcontainer::*;
use halftone_formats::thousands;

/// A readable name for an item's `Item:Semantic`.
fn item_name(semantic: &str) -> &str {
    match semantic {
        "Primary" => "the primary image",
        "GainMap" => "a gain map",
        "MotionPhoto" => "a motion clip",
        other => other,
    }
}

/// "a, b and c".
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The container-directory evidence source (`container_directory`).
///
/// `Absent`: a JPEG with no container directory, which is most of them; it says
/// nothing about origin. `Present`: the directory is there and the file holds every
/// declared item where the directory places it, and nothing after. `Inconclusive`:
/// the file and its directory disagree (an item missing, cut short, or not where
/// declared; bytes after the motion clip), or the directory cannot be read. A
/// disagreement is reported as fact, never as an accusation: the directory is
/// unsigned, and apps and upload pipelines drop motion clips as a matter of course.
/// Agreement, for its part, says nothing about edits inside an item.
#[derive(Debug, Default)]
pub struct ContainerDirectory;

impl EvidenceSource for ContainerDirectory {
    fn id(&self) -> SourceId {
        SourceId {
            name: "container_directory".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
    fn layer(&self) -> Layer {
        Layer::Container
    }
    fn supports(&self, a: &Asset) -> bool {
        a.modality == Modality::Image && a.mime == "image/jpeg"
    }
    fn assess(&self, a: &Asset) -> halftone_core::Result<Evidence> {
        let (status, rationale, details) = match analyze(&a.bytes) {
            Ok(None) => (
                Status::Absent,
                "No container directory (the XMP item list that Motion Photo and Ultra HDR \
                 JPEGs carry). Most JPEGs have none; its absence says nothing about origin."
                    .to_string(),
                serde_json::json!({ "directory": false }),
            ),
            Err(e) => (
                Status::Inconclusive,
                format!(
                    "The file declares a container directory, but it could not be checked: {e}."
                ),
                serde_json::json!({ "directory": true, "error": e }),
            ),
            Ok(Some(l)) => {
                let details = serde_json::to_value(&l).unwrap_or(serde_json::Value::Null);
                if l.agrees() {
                    let mut items = vec![item_name("Primary").to_string()];
                    for p in &l.placements {
                        items.push(if p.length == 0 {
                            format!(
                                "{} (sharing the previous item's bytes)",
                                item_name(&p.semantic)
                            )
                        } else {
                            format!("{} ({} bytes)", item_name(&p.semantic), thousands(p.length))
                        });
                    }
                    (
                        Status::Present,
                        format!(
                            "The file holds what its container directory declares: {}, each \
                             where the directory places it. The directory is unsigned metadata, \
                             so this confirms the file's structure, not that any item is unedited.",
                            and_list(&items)
                        ),
                        details,
                    )
                } else {
                    let more = match l.disagreements.len() {
                        1 => String::new(),
                        n => format!(" ({} more in details)", n - 1),
                    };
                    // Only informative when it is the motion clip that is short.
                    let clip_short = l
                        .placements
                        .iter()
                        .any(|p| p.semantic == "MotionPhoto" && p.present_bytes < p.length);
                    let why = if clip_short {
                        " (some apps and upload pipelines drop the motion clip)"
                    } else {
                        ""
                    };
                    (
                        Status::Inconclusive,
                        format!(
                            "The file does not match its own container directory: {}{more}. The \
                             directory is what the writing app declared; a mismatch says the file \
                             no longer matches that declaration, not who changed what or why{why}.",
                            l.disagreements[0]
                        ),
                        details,
                    )
                }
            }
        };
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

    // The synthetic files of halftone-formats' gcontainer tests (test code cannot
    // be shared across crates).
    /// A Pixel-style directory: attributes on `Container:Item` inside
    /// `rdf:li rdf:parseType="Resource"`.
    fn xmp(items: &[(&str, &str, Option<u64>, Option<u64>)]) -> String {
        let mut lis = String::new();
        for (sem, mime, len, pad) in items {
            let mut attrs = format!(r#"Item:Mime="{mime}" Item:Semantic="{sem}""#);
            if let Some(l) = len {
                attrs.push_str(&format!(r#" Item:Length="{l}""#));
            }
            if let Some(p) = pad {
                attrs.push_str(&format!(r#" Item:Padding="{p}""#));
            }
            lis.push_str(&format!(
                r#"<rdf:li rdf:parseType="Resource"><Container:Item {attrs}/></rdf:li>"#
            ));
        }
        format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:Camera="http://ns.google.com/photos/1.0/camera/" xmlns:Container="{CONTAINER_NS}" xmlns:Item="{ITEM_NS}" Camera:MotionPhoto="1"><Container:Directory><rdf:Seq>{lis}</rdf:Seq></Container:Directory></rdf:Description></rdf:RDF></x:xmpmeta>"#
        )
    }

    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut s = vec![0xFF, marker];
        s.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        s.extend_from_slice(payload);
        s
    }

    /// A JPEG with an optional XMP packet, an APP1 holding an embedded thumbnail
    /// (its own SOI..EOI), and entropy data containing a stuffed FF00 followed by
    /// D9, a restart marker and fill bytes: none of which may end the image early.
    fn jpeg(xmp: Option<&str>) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8];
        if let Some(x) = xmp {
            let mut p = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
            p.extend_from_slice(x.as_bytes());
            b.extend(segment(0xE1, &p));
        }
        b.extend(segment(0xE1, b"Exif\0\0\xFF\xD8\xFF\xDB\x00\x02\xFF\xD9"));
        b.extend(segment(0xDA, &[1, 1, 0, 0, 63, 0]));
        b.extend_from_slice(&[
            0x12, 0xFF, 0x00, 0xD9, 0x34, 0xFF, 0xD3, 0x56, 0xFF, 0xFF, 0xD9,
        ]);
        b
    }

    fn mp4(len: usize) -> Vec<u8> {
        let mut v = vec![0, 0, 0, 0x18];
        v.extend_from_slice(b"ftypmp42");
        v.resize(len, 0x77);
        v
    }

    /// Primary (with directory) + gain map + clip, like the Fold's `.MP.jpg`.
    fn motion_photo() -> (Vec<u8>, u64, u64, u64) {
        let gain = jpeg(None);
        let clip = mp4(300);
        let dir = xmp(&[
            ("Primary", "image/jpeg", Some(0), Some(0)),
            ("GainMap", "image/jpeg", Some(gain.len() as u64), None),
            ("MotionPhoto", "video/mp4", Some(clip.len() as u64), Some(0)),
        ]);
        let mut f = jpeg(Some(&dir));
        let primary = f.len() as u64;
        f.extend(&gain);
        f.extend(&clip);
        (f, primary, gain.len() as u64, clip.len() as u64)
    }

    fn asset(bytes: Vec<u8>, mime: &str) -> Asset {
        Asset {
            path: None,
            bytes,
            modality: Modality::Image,
            mime: mime.into(),
            sha256: String::new(),
        }
    }

    /// No rationale may read as an accusation (principles: contradictions are
    /// Inconclusive, never accusations).
    fn neutral(r: &str) {
        for w in ["removed", "tamper", "stripped", "manipulat", "fake"] {
            assert!(!r.to_lowercase().contains(w), "{w:?} in {r:?}");
        }
    }

    #[test]
    fn source_verdicts_cover_absent_present_and_inconclusive() {
        let src = ContainerDirectory;
        let plain = src.assess(&asset(jpeg(None), "image/jpeg")).unwrap();
        assert_eq!(plain.status, Status::Absent);
        assert!(plain.rationale.contains("says nothing about origin"));

        let (f, primary, gain, clip) = motion_photo();
        let whole = src.assess(&asset(f.clone(), "image/jpeg")).unwrap();
        assert_eq!(whole.status, Status::Present, "{}", whole.rationale);
        assert!(
            whole
                .rationale
                .contains(&format!("a gain map ({gain} bytes)")),
            "{}",
            whole.rationale
        );
        assert!(
            whole
                .rationale
                .contains(&format!("a motion clip ({clip} bytes)")),
            "{}",
            whole.rationale
        );
        assert!(whole.rationale.contains("not that any item is unedited"));
        assert_eq!(whole.details["placements"][1]["magic"], "matches");
        assert_eq!(whole.details["declared_end"], whole.details["file_len"]);

        let cut = src
            .assess(&asset(
                f[..(primary + gain) as usize].to_vec(),
                "image/jpeg",
            ))
            .unwrap();
        assert_eq!(cut.status, Status::Inconclusive);
        assert!(
            cut.rationale.contains("item 2 (MotionPhoto"),
            "{}",
            cut.rationale
        );
        assert!(cut.rationale.contains("not who changed what or why"));
        assert!(
            cut.rationale.contains("drop the motion clip"),
            "{}",
            cut.rationale
        );
        assert_eq!(cut.details["placements"][1]["present_bytes"], 0);

        let both = src
            .assess(&asset(f[..primary as usize].to_vec(), "image/jpeg"))
            .unwrap();
        assert!(
            both.rationale.contains("(1 more in details)"),
            "{}",
            both.rationale
        );

        // Trailing bytes: a disagreement, but not a short clip, so no clip aside.
        let mut tail = f.clone();
        tail.extend_from_slice(b"appended");
        let tail = src.assess(&asset(tail, "image/jpeg")).unwrap();
        assert_eq!(tail.status, Status::Inconclusive);
        assert!(
            !tail.rationale.contains("drop the motion clip"),
            "{}",
            tail.rationale
        );

        for e in [&plain, &whole, &cut, &both, &tail] {
            neutral(&e.rationale);
        }
    }

    #[test]
    fn source_applies_to_jpeg_only_and_reports_an_unreadable_directory() {
        let src = ContainerDirectory;
        assert!(src.supports(&asset(jpeg(None), "image/jpeg")));
        assert!(!src.supports(&asset(jpeg(None), "image/png")));
        let bad =
            xmp(&[("Primary", "image/jpeg", Some(0), None)]).replace("</Container:Directory>", "");
        let e = src.assess(&asset(jpeg(Some(&bad)), "image/jpeg")).unwrap();
        assert_eq!(e.status, Status::Inconclusive);
        assert!(
            e.rationale.contains("could not be checked"),
            "{}",
            e.rationale
        );
        assert_eq!(e.details["directory"], true);
        neutral(&e.rationale);
    }
}
