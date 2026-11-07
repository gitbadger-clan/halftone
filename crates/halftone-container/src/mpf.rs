//! The MPF-index evidence source (`mpf_index`): one question, "does this file
//! contain what its own MPF index declares?", answered from the bytes alone. The
//! index parser and layout rule live in [`halftone_formats::mpf`] and are
//! re-exported here.
//!
//! Companion to [`crate::gcontainer::ContainerDirectory`]: Ultra HDR JPEGs declare
//! their gain map in both places, iPhone JPEGs in MPF only, Motion Photo clips in
//! the container directory only. Each source reads one declaration and says so.

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId, Status};
pub use halftone_formats::mpf::*;
use halftone_formats::thousands;

/// "a, b and c".
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The MPF-index evidence source (`mpf_index`).
///
/// `Absent`: a JPEG with no MPF index, which is most of them; it says nothing about
/// origin, and a file can still hold images no MPF index declares. `Present`: the
/// index is there, the primary is a complete JPEG, and every later image is a
/// complete JPEG of the declared size where the index places it. The primary's
/// own declared size is reported but not checked (see [`Layout::primary_size_stale`]). `Inconclusive`: the file and its index disagree (an
/// image missing, cut short, moved, or a count that does not add up), or the index
/// cannot be read. A disagreement is a fact about structure, never an accusation:
/// the index is unsigned, and re-encoders routinely keep the APP2 segment while
/// dropping the images it points to. Agreement says nothing about edits inside an
/// image.
#[derive(Debug, Default)]
pub struct MpfIndex;

impl EvidenceSource for MpfIndex {
    fn id(&self) -> SourceId {
        SourceId {
            name: "mpf_index".into(),
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
                "No MPF index (the APP2 table of further images that iPhone, Ultra HDR and \
                 many camera JPEGs carry). Most JPEGs have none; its absence says nothing \
                 about origin, and images declared only elsewhere (a Motion Photo clip, in \
                 the container directory) are not looked for here."
                    .to_string(),
                serde_json::json!({ "index": false }),
            ),
            Err(e) => (
                Status::Inconclusive,
                format!("The file has an MPF segment, but it could not be read: {e}."),
                serde_json::json!({ "index": true, "error": e }),
            ),
            Ok(Some(l)) => {
                let details = serde_json::to_value(&l).unwrap_or(serde_json::Value::Null);
                if l.agrees() {
                    let items: Vec<String> = l
                        .placements
                        .iter()
                        .map(|p| {
                            let what = describe(p.type_name);
                            // The primary by its real length; its declared size, if
                            // different, is named below.
                            let size = match p.index {
                                0 => l.primary_size_found.unwrap_or(p.size),
                                _ => p.size,
                            };
                            match p.self_described_as.as_slice() {
                                [] => format!("{what} ({} bytes)", thousands(size)),
                                says => format!(
                                    "{what} ({} bytes; its own metadata: {})",
                                    thousands(size),
                                    says.join(", ")
                                ),
                            }
                        })
                        .collect();
                    let mut tail = String::new();
                    if let Some((declared, found)) = l.primary_size_stale() {
                        tail.push_str(&format!(
                            " The index gives the primary image as {} bytes; it is {}. Later \
                             images are placed relative to the index itself, so metadata added \
                             or removed in front of it after writing (camera pipelines and \
                             editors both do this) changes that size without moving anything \
                             else. It is recorded, not checked, so this source cannot tell \
                             whether that metadata changed.",
                            thousands(declared),
                            thousands(found)
                        ));
                    }
                    if l.trailing_bytes > 0 {
                        tail.push_str(&format!(
                            " {} bytes follow the last declared image; the index does not \
                             describe them.",
                            thousands(l.trailing_bytes)
                        ));
                    }
                    (
                        Status::Present,
                        format!(
                            "The file holds what its MPF index declares: {}, each where the \
                             index places it. The index is unsigned metadata, so this confirms \
                             the file's structure, not that any image is unedited.{tail}",
                            and_list(&items)
                        ),
                        details,
                    )
                } else {
                    let more = match l.disagreements.len() {
                        1 => String::new(),
                        n => format!(" ({} more in details)", n - 1),
                    };
                    (
                        Status::Inconclusive,
                        format!(
                            "The file does not match its own MPF index: {}{more}. The index is \
                             what the writing app declared; a mismatch says the file no longer \
                             matches that declaration, not who changed what or why (re-encoders \
                             often keep the index and drop the images it lists).",
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

    // Synthetic files as in halftone-formats' mpf tests (test code cannot be
    // shared across crates).
    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut s = vec![0xFF, marker];
        s.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        s.extend_from_slice(payload);
        s
    }

    fn tiny_jpeg(extra: &[Vec<u8>]) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8];
        for s in extra {
            b.extend_from_slice(s);
        }
        b.extend(segment(0xDA, &[1, 1, 0, 0, 63, 0]));
        b.extend_from_slice(&[0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD9]);
        b
    }

    /// Big-endian MPF payload with two entries: (attribute, size, raw offset).
    fn mpf(entries: [(u32, u32, u32); 2]) -> Vec<u8> {
        let mut t = b"MM\x00\x2A\x00\x00\x00\x08\x00\x02".to_vec();
        // NumberOfImages = 2
        t.extend_from_slice(&[0xB0, 0x01, 0x00, 0x04, 0, 0, 0, 1, 0, 0, 0, 2]);
        // MPEntry: 32 bytes at offset 8 + 2 + 24 + 4 = 38
        t.extend_from_slice(&[0xB0, 0x02, 0x00, 0x07, 0, 0, 0, 32, 0, 0, 0, 38]);
        t.extend_from_slice(&[0, 0, 0, 0]);
        for (a, s, o) in entries {
            t.extend_from_slice(&a.to_be_bytes());
            t.extend_from_slice(&s.to_be_bytes());
            t.extend_from_slice(&o.to_be_bytes());
            t.extend_from_slice(&[0, 0, 0, 0]);
        }
        let mut p = b"MPF\0".to_vec();
        p.extend(t);
        p
    }

    /// iPhone-style: primary with MPF + gain map that names Apple's namespace.
    fn iphone_like() -> (Vec<u8>, usize) {
        let gain = tiny_jpeg(&[segment(
            0xE1,
            b"http://ns.adobe.com/xap/1.0/\0<x xmlns:HDRGainMap=\"http://ns.apple.com/HDRGainMap/1.0/\"/>",
        )]);
        let primary_len = tiny_jpeg(&[segment(0xE2, &mpf([(0, 0, 0); 2]))]).len();
        let mut f = tiny_jpeg(&[segment(
            0xE2,
            &mpf([
                (0x2003_0000, primary_len as u32, 0),
                (0, gain.len() as u32, (primary_len - 10) as u32),
            ]),
        )]);
        f.extend_from_slice(&gain);
        (f, primary_len)
    }

    fn assess(bytes: Vec<u8>) -> Evidence {
        let a = Asset::from_bytes(bytes, None).unwrap();
        MpfIndex.assess(&a).unwrap()
    }

    #[test]
    fn an_iphone_style_file_is_present_and_names_its_gain_map() {
        let ev = assess(iphone_like().0);
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
        assert!(
            ev.rationale.contains("image of undefined MPF type"),
            "{}",
            ev.rationale
        );
        assert!(
            ev.rationale.contains("Apple HDR gain map"),
            "{}",
            ev.rationale
        );
        assert!(ev.rationale.contains("unsigned"));
    }

    #[test]
    fn a_stale_primary_size_is_present_and_named_with_its_blind_spot() {
        let (f, p) = iphone_like();
        // Shrink the declared primary size by 7, as on every Pixel capture.
        let mut g = f.clone();
        let at = g
            .windows(4)
            .position(|w| w == (p as u32).to_be_bytes())
            .unwrap();
        g[at..at + 4].copy_from_slice(&(p as u32 - 7).to_be_bytes());
        let ev = assess(g);
        assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
        assert!(
            ev.rationale.contains("recorded, not checked"),
            "{}",
            ev.rationale
        );
        assert_eq!(ev.details["primary_size_found"], p as u64);
    }

    #[test]
    fn a_dropped_gain_map_is_inconclusive_and_says_why_it_happens() {
        let (f, p) = iphone_like();
        let ev = assess(f[..p].to_vec());
        assert_eq!(ev.status, Status::Inconclusive);
        assert!(
            ev.rationale.contains("past the end of the file"),
            "{}",
            ev.rationale
        );
        assert!(ev.rationale.contains("not who changed what"));
    }

    #[test]
    fn no_index_is_absent_and_names_its_blind_spot() {
        let ev = assess(tiny_jpeg(&[]));
        assert_eq!(ev.status, Status::Absent);
        assert!(ev.rationale.contains("says nothing about origin"));
        assert!(ev.rationale.contains("container directory"));
    }

    #[test]
    fn an_unreadable_index_is_inconclusive() {
        let ev = assess(tiny_jpeg(&[segment(0xE2, b"MPF\0XX\0\0")]));
        assert_eq!(ev.status, Status::Inconclusive);
        assert!(ev.rationale.contains("could not be read"));
    }

    #[test]
    fn only_jpeg_is_supported() {
        let png = Asset::from_bytes(b"\x89PNG\r\n\x1a\n\0\0\0\0".to_vec(), None).unwrap();
        assert!(!MpfIndex.supports(&png));
    }
}
