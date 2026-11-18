//! The ICC colour profile a file embeds: where it sits in the container, what its
//! header says, and how the profile describes itself.
//!
//! Reading only. Nothing here judges a profile; the sources that use it do
//! ([`crate::identifiers`] lists a display's name). What is read:
//!
//! - the profile bytes: a JPEG's APP2 `ICC_PROFILE` segments (reassembled when the
//!   profile was split), a PNG's `iCCP` chunk (a zlib stream, inflated here), a
//!   WebP's `ICCP` chunk;
//! - from the 128-byte header: version, profile class (`mntr` display, `scnr` input,
//!   `prtr` output, `spac` colour space, …) and data colour space;
//! - the signatures in the tag table, in order;
//! - the profile description, the `desc` tag: the ASCII string of a version 2
//!   `desc` element, or one record of a version 4 `mluc` element.
//!
//! The description is read the way ExifTool reports `ICC_Profile:ProfileDescription`,
//! so the differential test can compare the two (measured with ExifTool 12.76 and
//! 13.64 on hand-built profiles, 2026-11-17): of a multi-language `mluc` element the
//! `en-US` record is the description, the last one if there are several, and when
//! there is no such record the first one written is taken.
//!
//! Not read: the profile of a HEIF file (its `colr` box), Apple's localized
//! description (`dscm`), and every other tag's content. Among those are tags that
//! describe one physical screen (`vcgt`, and Apple's `mmod` and `ndin`); their
//! presence shows in [`Profile::tags`], their content is not opened.
//!
//! Bounds: a profile larger than [`MAX_PROFILE`] is not read, inflating stops at
//! that size, and every offset in the tag table is checked against the profile's
//! length. Never panics on malformed input.

use halftone_core::Asset;
use serde::Serialize;

use crate::embedded::tidy;

/// Largest profile read, in bytes, embedded or inflated. Display and working-space
/// profiles are a few kilobytes; print profiles reach a few megabytes.
pub const MAX_PROFILE: usize = 8 * 1024 * 1024;

/// Most tag-table entries looked at. Real profiles have a dozen or two.
const MAX_TAGS: usize = 256;

/// The names of standard colour spaces, one per line; see the file for what belongs
/// on it. `scripts/differential.py` reads the same file.
const STANDARD_NAMES: &str = include_str!("icc_standard_names.txt");

/// What an ICC profile says about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Profile {
    /// Length of the profile in bytes, as embedded (after inflating, for a PNG).
    pub size: usize,
    /// Version from the header, `major.minor`.
    pub version: String,
    /// Profile class from the header, as its four-character signature: `mntr`
    /// (display), `scnr` (input), `prtr` (output), `spac`, `link`, `abst`, `nmcl`.
    pub class: String,
    /// Colour space of the data (`RGB`, `GRAY`, `CMYK`, …).
    pub colour_space: String,
    /// The profile description (`desc` tag), trimmed and bounded. `None` when the
    /// tag is missing, empty or cannot be read.
    pub description: Option<String>,
    /// Tag signatures in table order.
    pub tags: Vec<String>,
}

impl Profile {
    /// Whether the header gives the display class (`mntr`). Every common RGB working
    /// space is of this class too (sRGB, Display P3, Adobe RGB), so the class alone
    /// says nothing about a screen.
    pub fn is_display_class(&self) -> bool {
        self.class == "mntr"
    }
}

/// What was found where a container keeps its profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Found {
    /// No profile, or a container this module does not look into.
    #[default]
    None,
    /// A profile is there and could not be read; the reason, for a rationale.
    Unreadable(String),
    /// The profile.
    Profile(Profile),
}

impl Found {
    /// The profile, when one was read.
    pub fn profile(&self) -> Option<&Profile> {
        match self {
            Found::Profile(p) => Some(p),
            _ => None,
        }
    }

    /// Whether a profile is there that could not be read.
    pub fn unreadable(&self) -> bool {
        matches!(self, Found::Unreadable(_))
    }

    /// One word for `details`: `absent`, `unreadable` or `read`.
    pub fn state(&self) -> &'static str {
        match self {
            Found::None => "absent",
            Found::Unreadable(_) => "unreadable",
            Found::Profile(_) => "read",
        }
    }
}

/// Whether `name` is the name of a standard colour space (the shared list in
/// `icc_standard_names.txt`). Compared after trimming and collapsing whitespace,
/// without regard to ASCII case.
pub fn is_standard_name(name: &str) -> bool {
    let name = squeeze(name);
    standard_names().any(|n| squeeze(n).eq_ignore_ascii_case(&name))
}

/// The entries of the shared list, as written.
pub fn standard_names() -> impl Iterator<Item = &'static str> {
    STANDARD_NAMES
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

fn squeeze(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Read the profile an asset embeds. JPEG, PNG and WebP; anything else is
/// [`Found::None`].
pub fn read(a: &Asset) -> Found {
    let bytes = match a.mime.as_str() {
        "image/jpeg" => jpeg_profile(&a.bytes),
        "image/png" => png_profile(&a.bytes),
        "image/webp" => webp_profile(&a.bytes),
        _ => None,
    };
    match bytes {
        None => Found::None,
        Some(Err(why)) => Found::Unreadable(why),
        Some(Ok(b)) => match parse(&b) {
            Ok(p) => Found::Profile(p),
            Err(why) => Found::Unreadable(why),
        },
    }
}

fn too_large(n: usize) -> String {
    format!("the profile is larger than {MAX_PROFILE} bytes ({n})")
}

fn jpeg_profile(b: &[u8]) -> Option<Result<Vec<u8>, String>> {
    let m = halftone_formats::jpeg::metadata(b).ok()?;
    let total: usize = m.icc_chunks.iter().map(|c| c.data.len()).sum();
    if total > MAX_PROFILE {
        return Some(Err(too_large(total)));
    }
    m.icc_profile()
}

fn png_profile(b: &[u8]) -> Option<Result<Vec<u8>, String>> {
    let mut i = 8;
    while i + 12 <= b.len() {
        let len = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let ty = &b[i + 4..i + 8];
        let start = i + 8;
        let end = start.checked_add(len).filter(|&e| e + 4 <= b.len())?;
        match ty {
            b"iCCP" => {
                // Name, NUL, compression method (0 = zlib), the compressed profile.
                let data = &b[start..end];
                let Some(nul) = data.iter().position(|&c| c == 0) else {
                    return Some(Err("the iCCP chunk has no name terminator".into()));
                };
                let Some((&method, stream)) = data[nul + 1..].split_first() else {
                    return Some(Err("the iCCP chunk ends after its name".into()));
                };
                if method != 0 {
                    return Some(Err(format!(
                        "the iCCP chunk uses compression method {method}"
                    )));
                }
                return Some(
                    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(stream, MAX_PROFILE)
                        .map_err(|e| format!("the iCCP chunk could not be inflated ({e})")),
                );
            }
            // The profile comes before the image data.
            b"IDAT" | b"IEND" => return None,
            _ => {}
        }
        i = end + 4;
    }
    None
}

fn webp_profile(b: &[u8]) -> Option<Result<Vec<u8>, String>> {
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return None;
    }
    let mut i = 12;
    while i + 8 <= b.len() {
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let start = i + 8;
        let end = start.checked_add(len).filter(|&e| e <= b.len())?;
        if &b[i..i + 4] == b"ICCP" {
            return Some(if len > MAX_PROFILE {
                Err(too_large(len))
            } else {
                Ok(b[start..end].to_vec())
            });
        }
        i = end + (len & 1); // chunks are padded to an even length
    }
    None
}

fn be32(b: &[u8], at: usize) -> Option<usize> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

/// A four-byte signature as text: printable ASCII as written (padding spaces
/// dropped), anything else in hex.
fn signature(b: &[u8]) -> String {
    if b.iter().all(|c| c.is_ascii_graphic() || *c == b' ') {
        String::from_utf8_lossy(b).trim_end().to_string()
    } else {
        format!(
            "0x{}",
            b.iter().map(|c| format!("{c:02x}")).collect::<String>()
        )
    }
}

/// Parse a profile: header, tag table, description. `Err` says why the bytes are
/// not a profile this module can read.
pub fn parse(p: &[u8]) -> Result<Profile, String> {
    if p.len() < 132 {
        return Err(format!("{} bytes is too short for an ICC profile", p.len()));
    }
    if &p[36..40] != b"acsp" {
        return Err("the ICC profile signature (acsp) is missing".into());
    }
    let count = be32(p, 128).unwrap_or(0);
    let mut tags = Vec::new();
    let mut description = None;
    for k in 0..count.min(MAX_TAGS) {
        let at = 132 + 12 * k;
        let Some(entry) = p.get(at..at + 12) else {
            break; // the table runs past the profile: keep what was there
        };
        tags.push(signature(&entry[..4]));
        if &entry[..4] == b"desc" && description.is_none() {
            let (offset, size) = (be32(entry, 4).unwrap_or(0), be32(entry, 8).unwrap_or(0));
            description = offset
                .checked_add(size)
                .and_then(|end| p.get(offset..end))
                .and_then(describe);
        }
    }
    Ok(Profile {
        size: p.len(),
        version: format!("{}.{}", p[8], p[9] >> 4),
        class: signature(&p[12..16]),
        colour_space: signature(&p[16..20]),
        description,
        tags,
    })
}

/// The text of a `desc` tag element, by the element's own type.
fn describe(el: &[u8]) -> Option<String> {
    let text = match el.get(..4)? {
        // Version 2 textDescriptionType: ASCII count, then the ASCII string. (Its
        // Unicode and ScriptCode copies follow and are not read.)
        b"desc" => {
            let n = be32(el, 8)?;
            let ascii = el.get(12..12usize.checked_add(n)?)?;
            latin(ascii)
        }
        // Plain text, as a few writers store it.
        b"text" => latin(el.get(8..)?),
        // Version 4 multiLocalizedUnicodeType: records of language, country,
        // length and offset, each pointing at UTF-16BE text inside the element.
        b"mluc" => {
            let n = be32(el, 8)?.min(MAX_TAGS);
            let stride = be32(el, 12)?.max(12);
            let mut first = None;
            let mut default = None;
            for k in 0..n {
                let Some(rec) = el.get(16 + stride * k..16 + stride * k + 12) else {
                    break;
                };
                let (len, off) = (be32(rec, 4)?, be32(rec, 8)?);
                let Some(raw) = off.checked_add(len).and_then(|end| el.get(off..end)) else {
                    continue;
                };
                let units: Vec<u16> = raw
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                let text = String::from_utf16_lossy(&units);
                // ExifTool's rule for which record is "the" description: en-US, or
                // a code that is not two letters and two letters.
                let code = &rec[..4];
                let coded = code.iter().all(u8::is_ascii_alphabetic);
                if !coded || code.eq_ignore_ascii_case(b"enUS") {
                    default = Some(text.clone());
                }
                first.get_or_insert(text);
            }
            default.or(first)?
        }
        _ => return None,
    };
    let text = tidy(text.trim_end_matches('\0'));
    (!text.is_empty()).then_some(text)
}

/// A NUL-terminated byte string as text: UTF-8 when it is, and `?` for each run of
/// bytes that is not, which is how ExifTool's JSON shows them.
fn latin(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).replace('\u{FFFD}', "?")
}

#[cfg(test)]
pub(crate) mod testutil {
    //! Hand-built profiles for the tests here and in the sources that read one.

    /// A version 2 `desc` element holding `ascii`.
    pub fn desc_v2(ascii: &str) -> Vec<u8> {
        let mut v = b"desc\0\0\0\0".to_vec();
        v.extend_from_slice(&(ascii.len() as u32 + 1).to_be_bytes());
        v.extend_from_slice(ascii.as_bytes());
        v.push(0);
        v.extend_from_slice(&[0; 8 + 3 + 67]); // empty Unicode and ScriptCode parts
        v
    }

    /// A version 4 `mluc` element with one record per (language+country, text).
    pub fn mluc(records: &[(&str, &str)]) -> Vec<u8> {
        let mut table = Vec::new();
        let mut body = Vec::new();
        let base = 16 + 12 * records.len();
        for (code, text) in records {
            let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
            table.extend_from_slice(code.as_bytes());
            table.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
            table.extend_from_slice(&((base + body.len()) as u32).to_be_bytes());
            body.extend(utf16);
        }
        let mut v = b"mluc\0\0\0\0".to_vec();
        v.extend_from_slice(&(records.len() as u32).to_be_bytes());
        v.extend_from_slice(&12u32.to_be_bytes());
        v.extend(table);
        v.extend(body);
        v
    }

    /// A profile of the given class and version with these tags, in order.
    pub fn profile(class: &[u8; 4], major: u8, tags: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut head = vec![0u8; 128];
        head[8] = major;
        head[9] = 0x30;
        head[12..16].copy_from_slice(class);
        head[16..20].copy_from_slice(b"RGB ");
        head[20..24].copy_from_slice(b"XYZ ");
        head[36..40].copy_from_slice(b"acsp");
        let mut table = (tags.len() as u32).to_be_bytes().to_vec();
        let mut data = Vec::new();
        let base = 128 + 4 + 12 * tags.len();
        for (sig, payload) in tags {
            table.extend_from_slice(*sig);
            table.extend_from_slice(&((base + data.len()) as u32).to_be_bytes());
            table.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            data.extend_from_slice(payload);
            data.resize(data.len().next_multiple_of(4), 0);
        }
        let mut out = head;
        out.extend(table);
        out.extend(data);
        let size = (out.len() as u32).to_be_bytes();
        out[..4].copy_from_slice(&size);
        out
    }

    /// A display-class version 2 profile described as `name`.
    pub fn display(name: &str) -> Vec<u8> {
        profile(
            b"mntr",
            2,
            &[(b"desc", desc_v2(name)), (b"wtpt", vec![0; 20])],
        )
    }

    /// A JPEG that embeds `profile`, split into `chunks` APP2 segments.
    pub fn jpeg_with(profile: &[u8], chunks: usize) -> Vec<u8> {
        let mut f = vec![0xFF, 0xD8];
        let size = profile.len().div_ceil(chunks).max(1);
        for (k, part) in profile.chunks(size).enumerate() {
            let mut p = b"ICC_PROFILE\0".to_vec();
            p.extend_from_slice(&[k as u8 + 1, chunks as u8]);
            p.extend_from_slice(part);
            f.extend_from_slice(&[0xFF, 0xE2]);
            f.extend_from_slice(&(p.len() as u16 + 2).to_be_bytes());
            f.extend(p);
        }
        f.extend_from_slice(&[0xFF, 0xD9]);
        f
    }

    fn png_chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = (data.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(ty);
        v.extend_from_slice(data);
        v.extend_from_slice(&[0; 4]); // the CRC is not checked
        v
    }

    /// A PNG whose `iCCP` chunk, named `name`, holds this already-compressed stream.
    pub fn png_with_stream(name: &str, method: u8, stream: &[u8]) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&16u32.to_be_bytes());
        ihdr.extend_from_slice(&16u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        v.extend(png_chunk(b"IHDR", &ihdr));
        let mut iccp = name.as_bytes().to_vec();
        iccp.extend_from_slice(&[0, method]);
        iccp.extend_from_slice(stream);
        v.extend(png_chunk(b"iCCP", &iccp));
        v.extend(png_chunk(b"IDAT", &[0; 4]));
        v.extend(png_chunk(b"IEND", &[]));
        v
    }

    /// A PNG that embeds `profile` in an `iCCP` chunk named `name`.
    pub fn png_with(name: &str, profile: &[u8]) -> Vec<u8> {
        let stream = miniz_oxide::deflate::compress_to_vec_zlib(profile, 6);
        png_with_stream(name, 0, &stream)
    }

    /// A WebP (extended format) that embeds `profile`.
    pub fn webp_with(profile: &[u8]) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        let mut chunk = |fourcc: &[u8; 4], data: &[u8]| {
            body.extend_from_slice(fourcc);
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        };
        chunk(b"VP8X", &[0x20, 0, 0, 0, 15, 0, 0, 15, 0, 0]);
        chunk(b"ICCP", profile);
        chunk(b"VP8 ", &[0; 10]);
        let mut v = b"RIFF".to_vec();
        v.extend_from_slice(&(body.len() as u32).to_le_bytes());
        v.extend(body);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    fn asset(bytes: Vec<u8>) -> Asset {
        Asset::from_bytes(bytes, None).unwrap()
    }

    #[test]
    fn the_header_the_tag_list_and_the_description_are_read() {
        let p = parse(&profile(
            b"mntr",
            2,
            &[
                (b"desc", desc_v2("Example Display EX2700")),
                (b"wtpt", vec![0; 20]),
                (b"vcgt", vec![0; 18]),
                (b"mmod", vec![0; 40]),
            ],
        ))
        .unwrap();
        assert_eq!(p.version, "2.3");
        assert_eq!(p.class, "mntr");
        assert!(p.is_display_class());
        assert_eq!(p.colour_space, "RGB");
        assert_eq!(p.description.as_deref(), Some("Example Display EX2700"));
        assert_eq!(p.tags, ["desc", "wtpt", "vcgt", "mmod"]);
        assert_eq!(p.size % 4, 0);

        let printer = parse(&profile(
            b"prtr",
            4,
            &[(b"desc", mluc(&[("enUS", "Coated")]))],
        ))
        .unwrap();
        assert_eq!(
            (printer.version.as_str(), printer.class.as_str()),
            ("4.3", "prtr")
        );
        assert!(!printer.is_display_class());
    }

    #[test]
    fn the_description_is_the_one_exiftool_reports() {
        let d = |el: Vec<u8>| {
            parse(&profile(b"mntr", 4, &[(b"desc", el)]))
                .unwrap()
                .description
        };
        let some = |s: &str| Some(s.to_string());
        // Version 2: the ASCII string, to its NUL; padding collapsed.
        assert_eq!(
            d(desc_v2("Example Display EX2700")),
            some("Example Display EX2700")
        );
        assert_eq!(d(desc_v2("  Example   Display  ")), some("Example Display"));
        assert_eq!(d(desc_v2("Left\0Right")), some("Left"));
        assert_eq!(d(desc_v2("")), None);
        // Version 4: en-US is the description wherever it sits, and the last one
        // when there are two; with no en-US record, the first one written.
        assert_eq!(d(mluc(&[("enUS", "American")])), some("American"));
        assert_eq!(
            d(mluc(&[("deDE", "Deutsch"), ("enUS", "American")])),
            some("American")
        );
        assert_eq!(
            d(mluc(&[("enUS", "American"), ("enGB", "British")])),
            some("American")
        );
        assert_eq!(
            d(mluc(&[("enUS", "First"), ("enUS", "Second")])),
            some("Second")
        );
        assert_eq!(
            d(mluc(&[("deDE", "Deutsch"), ("frFR", "Francais")])),
            some("Deutsch")
        );
        assert_eq!(
            d(mluc(&[("deDE", "Deutsch"), ("en\0\0", "Plain")])),
            some("Plain")
        );
        assert_eq!(
            d(mluc(&[("enUS", "Écran d’exemple — 27″")])),
            some("Écran d’exemple — 27″")
        );
        assert_eq!(d(mluc(&[])), None);
        // A text element, and bytes that are not UTF-8.
        assert_eq!(
            d(b"text\0\0\0\0Text Typed Name\0".to_vec()),
            some("Text Typed Name")
        );
        let mut high = desc_v2("Xcran EX");
        high[12] = 0xC9;
        assert_eq!(d(high), some("?cran EX"));
        // Another element type, and no desc tag at all.
        assert_eq!(d(b"XYZ \0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0".to_vec()), None);
        assert_eq!(parse(&profile(b"mntr", 2, &[])).unwrap().description, None);
        // Long descriptions are cut like every other value.
        let long = d(desc_v2(&"x".repeat(1000))).unwrap();
        assert_eq!(long.chars().count(), crate::embedded::MAX_VALUE + 1);
    }

    #[test]
    fn the_profile_is_found_in_each_container() {
        let p = display("Example Display EX2700");
        for (what, bytes) in [
            ("jpeg", jpeg_with(&p, 1)),
            ("jpeg in three chunks", jpeg_with(&p, 3)),
            ("png", png_with("ICC Profile", &p)),
            ("webp", webp_with(&p)),
        ] {
            let found = read(&asset(bytes));
            let got = found
                .profile()
                .unwrap_or_else(|| panic!("{what}: {found:?}"));
            assert_eq!(
                got.description.as_deref(),
                Some("Example Display EX2700"),
                "{what}"
            );
            assert_eq!(got.size, p.len(), "{what}");
            assert_eq!(found.state(), "read");
        }
        // Files without one.
        for bytes in [
            crate::embedded::testutil::jpeg(None, None, &[]),
            crate::embedded::testutil::png(&[]),
        ] {
            assert_eq!(read(&asset(bytes)), Found::None);
        }
    }

    #[test]
    fn a_profile_that_cannot_be_read_is_said_to_be_there() {
        let p = display("Example Display EX2700");
        let unreadable = |bytes: Vec<u8>| match read(&asset(bytes)) {
            Found::Unreadable(why) => why,
            other => panic!("{other:?}"),
        };
        // JPEG: the second of two chunks is gone.
        let mut one_of_two = jpeg_with(&p, 2);
        let marker = b"ICC_PROFILE\0";
        let second = one_of_two
            .windows(marker.len())
            .rposition(|w| w == marker)
            .unwrap()
            - 4;
        one_of_two.drain(second..one_of_two.len() - 2);
        assert!(unreadable(one_of_two).contains("missing"));
        // PNG: an unknown compression method, a broken stream, a stream that is not
        // a profile, and one that inflates past the limit.
        let stream = miniz_oxide::deflate::compress_to_vec_zlib(&p, 6);
        assert!(unreadable(png_with_stream("x", 1, &stream)).contains("method 1"));
        assert!(
            unreadable(png_with_stream("x", 0, &stream[..stream.len() / 2])).contains("inflated")
        );
        assert!(unreadable(png_with("x", b"not a profile")).contains("too short"));
        assert!(unreadable(png_with("x", &[7u8; 400])).contains("acsp"));
        let bomb = miniz_oxide::deflate::compress_to_vec_zlib(&vec![0u8; MAX_PROFILE + 1], 6);
        assert!(bomb.len() < 64 * 1024, "the test input stays small");
        assert!(unreadable(png_with_stream("x", 0, &bomb)).contains("inflated"));
        // WebP: bytes that are not a profile.
        assert!(unreadable(webp_with(&[1, 2, 3])).contains("too short"));
    }

    #[test]
    fn hostile_tag_tables_are_read_as_far_as_they_hold() {
        let good = display("Example Display EX2700");
        // The table claims more entries than the profile has room for.
        let mut many = good.clone();
        many[128..132].copy_from_slice(&u32::MAX.to_be_bytes());
        let p = parse(&many).unwrap();
        assert!(p.tags.len() <= MAX_TAGS);
        assert_eq!(p.description.as_deref(), Some("Example Display EX2700"));
        // The desc entry points outside the profile, or its length overflows.
        for (offset, size) in [(u32::MAX, 16u32), (200, u32::MAX), (good.len() as u32, 64)] {
            let mut bad = good.clone();
            bad[136..140].copy_from_slice(&offset.to_be_bytes());
            bad[140..144].copy_from_slice(&size.to_be_bytes());
            assert_eq!(parse(&bad).unwrap().description, None);
        }
        // A desc element whose string length runs past the element.
        let mut over = desc_v2("Short");
        over[8..12].copy_from_slice(&5000u32.to_be_bytes());
        assert_eq!(
            parse(&profile(b"mntr", 2, &[(b"desc", over)]))
                .unwrap()
                .description,
            None
        );
        // An mluc record pointing outside its element is skipped; the next is read.
        let mut el = mluc(&[("enUS", "Lost"), ("deDE", "Kept")]);
        el[24..28].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(
            parse(&profile(b"mntr", 4, &[(b"desc", el)]))
                .unwrap()
                .description
                .as_deref(),
            Some("Kept")
        );
        // Every prefix of a profile parses or errors; none panics.
        for n in 0..good.len() {
            let _ = parse(&good[..n]);
        }
    }

    #[test]
    fn standard_names_are_recognised_whatever_the_spacing_or_case() {
        for name in [
            "sRGB IEC61966-2.1",
            "  display p3 ",
            "Adobe  RGB (1998)",
            "c2",
            "SRGB",
        ] {
            assert!(is_standard_name(name), "{name}");
        }
        for name in [
            "Example Display EX2700",
            "Display",
            "Color LCD",
            "sRGB IEC61966-2.1 (copy)",
            "",
        ] {
            assert!(!is_standard_name(name), "{name}");
        }
        // The file is a list of names and nothing else.
        let names: Vec<&str> = standard_names().collect();
        assert!(names.len() >= 20);
        assert!(names.iter().all(|n| !n.contains('#') && n.len() <= 100));
        let mut unique: Vec<String> = names.iter().map(|n| n.to_ascii_lowercase()).collect();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "a name is listed twice");
    }
}
