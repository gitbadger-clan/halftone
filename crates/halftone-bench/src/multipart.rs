//! Byte-level edits of multi-part JPEGs: what a file loses when the parts after its
//! still image are dropped, cut or altered without re-encoding anything.
//!
//! A phone photo is often several files in one: the still image, then a gain map,
//! then (Motion Photo) an MP4 clip. Delivery paths and editors drop or damage the
//! later parts while leaving the still's bytes alone, and a signature over the file
//! reacts differently to each case. These five edits are the cases:
//!
//! | name         | edit                                                        |
//! |--------------|-------------------------------------------------------------|
//! | `keep_still` | cut right after the still image (everything after it goes)  |
//! | `drop_clip`  | cut where the motion clip starts                            |
//! | `cut_clip`   | cut in the middle of the motion clip                        |
//! | `flip_clip`  | flip one bit in the middle of the motion clip               |
//! | `flip_still` | flip one bit in the middle of the still's compressed data   |
//!
//! Where the parts are comes from the file itself: the still ends at the first
//! complete JPEG's EOI, and the clip is the `MotionPhoto` item of the container
//! directory, used only when the file matches that directory. An input without the
//! part an edit needs is "not applicable", not an error.
//!
//! The positions follow `scripts/make-multipart-stratum.fish` exactly (the middle of
//! the clip; the middle of the last scan, moved forward off any `0xFF` so no marker
//! is touched), so `ht survive --keep` on the 06 motion photo writes the same bytes
//! as the 08-multipart copies. That script takes its offsets from the signed part
//! table through c2patool and stays the builder of the stratum: these edits locate
//! the parts with Halftone's own parser, and identical output from both is the
//! check on it.
//!
//! Pure functions on bytes; never panic on hostile input.

use std::ops::Range;

use halftone_formats::{gcontainer, jpeg};

/// Scans shorter than this are not edited: there is no "middle" to speak of.
const MIN_SCAN_BYTES: usize = 64;

/// Why an edit does not apply to an input, in plain words.
pub type NotApplicable = String;

/// End of the still image: just past the first complete JPEG's EOI. Not applicable
/// when the input is not a JPEG or nothing follows the still.
pub fn still_end(file: &[u8]) -> Result<usize, NotApplicable> {
    let end = jpeg::primary_end(file).ok_or("not a complete JPEG")?;
    if end >= file.len() {
        return Err("nothing follows the still image".into());
    }
    Ok(end)
}

/// Byte range of the motion clip: the `MotionPhoto` item of the container directory.
/// Not applicable unless the file matches its directory and the clip ends the file.
pub fn clip(file: &[u8]) -> Result<Range<usize>, NotApplicable> {
    let layout = gcontainer::analyze(file)
        .map_err(|e| format!("container directory unreadable: {e}"))?
        .ok_or("no container directory, so no declared motion clip")?;
    if !layout.agrees() {
        return Err("the file does not match its container directory".into());
    }
    let p = layout
        .placements
        .iter()
        .find(|p| p.semantic == "MotionPhoto" && p.length > 0)
        .ok_or("the container directory declares no motion clip")?;
    let start = usize::try_from(p.offset).map_err(|_| "clip offset out of range")?;
    let len = usize::try_from(p.length).map_err(|_| "clip length out of range")?;
    let end = start.checked_add(len).ok_or("clip range overflows")?;
    if end != file.len() || len < 2 {
        return Err("the motion clip does not end the file".into());
    }
    Ok(start..end)
}

/// Position of the bit flip in the still: the middle of the entropy-coded data of
/// its last scan, moved forward while the byte or the one before it is `0xFF`, so
/// the flip can neither create nor destroy a marker.
pub fn still_flip_at(file: &[u8]) -> Result<usize, NotApplicable> {
    let end = jpeg::primary_end(file).ok_or("not a complete JPEG")?;
    let still = &file[..end];
    // Scan data never contains FF DA (0xFF is always stuffed), so the last FF DA
    // in the still is a real SOS.
    let sos = still
        .windows(2)
        .rposition(|w| w == [0xFF, 0xDA])
        .ok_or("no scan in the still image")?;
    let header = still
        .get(sos + 2..sos + 4)
        .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
        .ok_or("truncated scan header")?;
    let start = sos + 2 + header;
    let stop = still.len() - 2; // before EOI
    if stop <= start || stop - start < MIN_SCAN_BYTES {
        return Err("the still's compressed data is too short to edit".into());
    }
    let mut m = (start + stop) / 2;
    while m < stop - 1 && (still[m] == 0xFF || still[m - 1] == 0xFF) {
        m += 1;
    }
    if still[m] == 0xFF || still[m - 1] == 0xFF {
        return Err("no byte clear of a marker in the still's compressed data".into());
    }
    Ok(m)
}

fn flipped(file: &[u8], at: usize) -> Vec<u8> {
    let mut out = file.to_vec();
    out[at] ^= 1;
    out
}

/// `keep_still`: the still image alone.
pub fn keep_still(file: &[u8]) -> Result<Vec<u8>, NotApplicable> {
    Ok(file[..still_end(file)?].to_vec())
}

/// `drop_clip`: everything before the motion clip.
pub fn drop_clip(file: &[u8]) -> Result<Vec<u8>, NotApplicable> {
    Ok(file[..clip(file)?.start].to_vec())
}

/// `cut_clip`: the file cut in the middle of the motion clip.
pub fn cut_clip(file: &[u8]) -> Result<Vec<u8>, NotApplicable> {
    let c = clip(file)?;
    Ok(file[..c.start + c.len() / 2].to_vec())
}

/// `flip_clip`: one bit flipped in the middle of the motion clip.
pub fn flip_clip(file: &[u8]) -> Result<Vec<u8>, NotApplicable> {
    let c = clip(file)?;
    Ok(flipped(file, c.start + c.len() / 2))
}

/// `flip_still`: one bit flipped in the still's compressed data ([`still_flip_at`]).
pub fn flip_still(file: &[u8]) -> Result<Vec<u8>, NotApplicable> {
    Ok(flipped(file, still_flip_at(file)?))
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! A synthetic Motion Photo in the Fold's shape. Built like the fixtures in
    //! halftone-formats (test code cannot be shared across crates).

    const CONTAINER_NS: &str = "http://ns.google.com/photos/1.0/container/";
    const ITEM_NS: &str = "http://ns.google.com/photos/1.0/container/item/";

    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut s = vec![0xFF, marker];
        s.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        s.extend_from_slice(payload);
        s
    }

    /// A JPEG with `scan` bytes of entropy data (a stuffed FF00 in the middle, so
    /// the flip position has something to step over) and an optional XMP packet
    /// that may carry extra `marks` (text the stand-in sources look for).
    pub fn jpeg(xmp: Option<&str>, scan: usize) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8];
        if let Some(x) = xmp {
            let mut p = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
            p.extend_from_slice(x.as_bytes());
            b.extend(segment(0xE1, &p));
        }
        // An EXIF thumbnail with its own scan: an earlier FF DA that is not the still's.
        b.extend(segment(0xE1, b"Exif\0\0\xFF\xD8\xFF\xDA\x00\x02\xFF\xD9"));
        b.extend(segment(0xDA, &[1, 1, 0, 0, 63, 0]));
        let mut data: Vec<u8> = (0..scan).map(|i| (i % 200) as u8 + 1).collect();
        if scan >= 4 {
            data[scan / 2] = 0xFF;
            data[scan / 2 + 1] = 0x00;
        }
        b.extend(data);
        b.extend_from_slice(&[0xFF, 0xD9]);
        b
    }

    pub fn mp4(len: usize) -> Vec<u8> {
        let mut v = vec![0, 0, 0, 0x18];
        v.extend_from_slice(b"ftypmp42");
        v.resize(len, 0x77);
        v
    }

    fn directory(items: &[(&str, &str, usize)], marks: &str) -> String {
        let lis: String = items
            .iter()
            .map(|(sem, mime, len)| {
                format!(
                    r#"<rdf:li rdf:parseType="Resource"><Container:Item Item:Mime="{mime}" Item:Semantic="{sem}" Item:Length="{len}"/></rdf:li>"#
                )
            })
            .collect();
        format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:Container="{CONTAINER_NS}" xmlns:Item="{ITEM_NS}" {marks}><Container:Directory><rdf:Seq>{lis}</rdf:Seq></Container:Directory></rdf:Description></rdf:RDF></x:xmpmeta>"#
        )
    }

    /// Still (with directory) + gain map + clip. Returns (file, still length, gain
    /// map length, clip length). `marks` goes into the still's XMP.
    pub fn motion_photo(marks: &str) -> (Vec<u8>, usize, usize, usize) {
        let gain = jpeg(None, 40);
        let clip = mp4(301);
        let dir = directory(
            &[
                ("Primary", "image/jpeg", 0),
                ("GainMap", "image/jpeg", gain.len()),
                ("MotionPhoto", "video/mp4", clip.len()),
            ],
            marks,
        );
        let mut f = jpeg(Some(&dir), 400);
        let still = f.len();
        f.extend(&gain);
        f.extend(&clip);
        (f, still, gain.len(), clip.len())
    }

    /// Still + gain map, no clip: an Ultra HDR photo.
    pub fn ultra_hdr() -> (Vec<u8>, usize) {
        let gain = jpeg(None, 40);
        let dir = directory(
            &[
                ("Primary", "image/jpeg", 0),
                ("GainMap", "image/jpeg", gain.len()),
            ],
            "",
        );
        let mut f = jpeg(Some(&dir), 400);
        let still = f.len();
        f.extend(&gain);
        (f, still)
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn the_parts_are_found_where_the_fixture_put_them() {
        let (f, still, gain, clip_len) = motion_photo("");
        assert_eq!(still_end(&f), Ok(still));
        assert_eq!(clip(&f), Ok(still + gain..still + gain + clip_len));
        assert_eq!(f.len(), still + gain + clip_len);
    }

    #[test]
    fn each_edit_does_exactly_what_its_name_says() {
        let (f, still, gain, clip_len) = motion_photo("");
        let clip_at = still + gain;
        let mid = clip_at + clip_len / 2;

        assert_eq!(keep_still(&f).unwrap(), f[..still]);
        assert_eq!(drop_clip(&f).unwrap(), f[..clip_at]);
        assert_eq!(cut_clip(&f).unwrap(), f[..mid]);

        let fc = flip_clip(&f).unwrap();
        let diff: Vec<usize> = (0..f.len()).filter(|&i| f[i] != fc[i]).collect();
        assert_eq!(diff, [mid]);
        assert_eq!(fc[mid], f[mid] ^ 1);

        let fs = flip_still(&f).unwrap();
        let diff: Vec<usize> = (0..f.len()).filter(|&i| f[i] != fs[i]).collect();
        assert_eq!(diff.len(), 1);
        assert!(diff[0] < still, "inside the still");
        assert_eq!(fs.len(), f.len());
    }

    #[test]
    fn the_still_flip_is_in_the_last_scan_and_clear_of_markers() {
        let (f, still, ..) = motion_photo("");
        let at = still_flip_at(&f).unwrap();
        // After the real SOS header, not the thumbnail's FF DA inside EXIF.
        let sos = f[..still]
            .windows(2)
            .rposition(|w| w == [0xFF, 0xDA])
            .unwrap();
        assert!(at > sos + 8 && at < still - 2);
        // The fixture puts FF 00 at the exact middle; the position stepped past it.
        assert!(f[at] != 0xFF && f[at - 1] != 0xFF);
        // The still is still one complete JPEG of the same length afterwards.
        let edited = flip_still(&f).unwrap();
        assert_eq!(jpeg::primary_end(&edited), Some(still));
    }

    #[test]
    fn a_photo_without_a_clip_has_a_still_to_keep_but_no_clip_to_edit() {
        let (f, still) = ultra_hdr();
        assert_eq!(keep_still(&f).unwrap(), f[..still]);
        for edit in [drop_clip, cut_clip, flip_clip] {
            assert_eq!(
                edit(&f).unwrap_err(),
                "the container directory declares no motion clip"
            );
        }
        assert!(flip_still(&f).is_ok());
    }

    #[test]
    fn single_images_and_non_jpegs_are_not_applicable() {
        let single = jpeg(None, 400);
        assert_eq!(
            keep_still(&single).unwrap_err(),
            "nothing follows the still image"
        );
        assert!(
            drop_clip(&single)
                .unwrap_err()
                .contains("no container directory")
        );
        assert!(
            flip_still(&single).is_ok(),
            "a single JPEG still has a scan to flip"
        );
        assert!(
            flip_still(&jpeg(None, 10))
                .unwrap_err()
                .contains("too short")
        );
        for edit in [keep_still, drop_clip, cut_clip, flip_clip, flip_still] {
            assert!(edit(b"\x89PNG\r\n\x1a\n").is_err());
            assert!(edit(b"").is_err());
        }
    }

    #[test]
    fn a_file_that_no_longer_matches_its_directory_is_left_alone() {
        let (f, ..) = motion_photo("");
        let already_cut = cut_clip(&f).unwrap();
        assert_eq!(
            drop_clip(&already_cut).unwrap_err(),
            "the file does not match its container directory"
        );
    }

    #[test]
    fn no_prefix_of_a_motion_photo_panics() {
        let (f, ..) = motion_photo("");
        for n in 0..f.len() {
            for edit in [keep_still, drop_clip, cut_clip, flip_clip, flip_still] {
                let _ = edit(&f[..n]);
            }
        }
    }
}
