//! The MPF index (CIPA DC-007, Multi-Picture Format): the APP2 table in a JPEG's
//! primary image that lists further images stored after it in the same file, each
//! as an offset, a size and a type code. iPhone JPEGs declare their HDR gain map
//! this way and nowhere else; Ultra HDR JPEGs declare their gain map here as well
//! as in the container directory ([`crate::gcontainer`]).
//!
//! Reading only. The index is what the writer declared, unsigned: [`analyze`]
//! checks it against the bytes and lists every place the two disagree, and says
//! nothing about whether any image is unedited.
//!
//! Layout rules (DC-007 §5.2): offsets are counted from the start of the MP header
//! (the byte-order mark right after `MPF\0`), except the first image's, which is 0
//! and means the start of the file. Integers follow the header's byte order.
//!
//! The primary's own declared size is recorded but not checked. Because offsets
//! are relative to the header, a segment added or removed in front of it after
//! the index was written changes the primary's length and leaves every later
//! offset right; nothing updates the size. Every multi-part capture in the corpus
//! (Pixel 5, Pixel 9 Pro Fold, ExifTool rewrites of both) carries a stale size.

use serde::Serialize;

/// APP2 payload prefix of an MPF segment.
const MPF_ID: &[u8] = b"MPF\0";
/// `MPFVersion` (UNDEFINED, 4 bytes, "0100").
const TAG_VERSION: u16 = 0xB000;
/// `NumberOfImages` (LONG).
const TAG_NUMBER_OF_IMAGES: u16 = 0xB001;
/// `MPEntry` (UNDEFINED, 16 bytes per image).
const TAG_MP_ENTRY: u16 = 0xB002;
/// Bytes per MP entry.
const ENTRY_LEN: usize = 16;
/// IFD entries read at most; a real MP index IFD has five or fewer.
const MAX_IFD_ENTRIES: usize = 64;

/// Self-description markers looked for in a declared image's own APP1/APP2
/// segments: the namespace or URN, and what the marker says the image is.
const SELF_DESCRIPTIONS: &[(&[u8], &str)] = &[
    (
        b"http://ns.apple.com/HDRGainMap/1.0/",
        "Apple HDR gain map (XMP)",
    ),
    (
        b"http://ns.adobe.com/hdr-gain-map/1.0/",
        "Adobe/Ultra HDR gain map (XMP hdrgm)",
    ),
    (
        b"urn:iso:std:iso:ts:21496:-1",
        "ISO 21496-1 gain map metadata",
    ),
];

/// One image the MPF index declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// Position in the index, from 0 (the primary image).
    pub index: usize,
    /// The 32-bit Individual Image Attribute as written.
    pub attribute: u32,
    /// MP type code (the attribute's low 24 bits), e.g. `0x030000`.
    pub type_code: u32,
    /// [`type_name`] of `type_code`.
    pub type_name: &'static str,
    /// Image data format (attribute bits 24–26); 0 is JPEG, the only one defined.
    pub data_format: u8,
    /// Dependent parent image flag (attribute bit 31).
    pub dependent_parent: bool,
    /// Dependent child image flag (attribute bit 30).
    pub dependent_child: bool,
    /// Representative image flag (attribute bit 29).
    pub representative: bool,
    /// Declared size in bytes.
    pub size: u64,
    /// The offset as written (relative to the MP header; 0 for the first image).
    pub raw_offset: u32,
    /// The declared start in bytes from the start of the file: 0 for the first
    /// image, the MP header's position plus `raw_offset` for every other.
    pub offset: u64,
    /// Dependent image entry numbers 1 and 2, as written.
    pub dependents: [u16; 2],
}

/// An MPF index as read from the primary image's APP2 segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Index {
    /// Position of the MP header (the byte-order mark) in the file.
    pub header_offset: u64,
    /// `true` for big-endian (`MM`), `false` for little-endian (`II`).
    pub big_endian: bool,
    /// `MPFVersion` as text, when present and printable (normally `0100`).
    pub version: Option<String>,
    /// `NumberOfImages`, when present.
    pub number_of_images: Option<u32>,
    /// Every declared image, in index order.
    pub entries: Vec<Entry>,
    /// MPF segments in the primary's metadata after the first (ignored).
    pub extra_segments: usize,
}

/// One declared image checked against the bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Placement {
    /// Position in the index.
    pub index: usize,
    /// [`type_name`] of the entry's type code.
    pub type_name: &'static str,
    /// Declared start in the file.
    pub offset: u64,
    /// Declared size.
    pub size: u64,
    /// Bytes of the declared range the file actually holds.
    pub present_bytes: u64,
    /// The range starts with a JPEG SOI (`FF D8 FF`).
    pub starts_jpeg: bool,
    /// What the image's own metadata segments say it is (see the module docs);
    /// empty when they say nothing this reader knows.
    pub self_described_as: Vec<&'static str>,
}

/// An MPF index checked against the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Layout {
    /// The index as read.
    pub index: Index,
    /// File length in bytes.
    pub file_len: u64,
    /// One placement per entry, in index order.
    pub placements: Vec<Placement>,
    /// Where the primary JPEG actually ends (its length in bytes), when it is
    /// complete. Compared with the primary's declared size only for reporting:
    /// see [`Layout::primary_size_stale`].
    pub primary_size_found: Option<u64>,
    /// Bytes after the end of the last declared image. Not a disagreement: the
    /// index does not claim to describe the whole file (a Motion Photo's clip
    /// follows the MPF images and is declared only in the container directory).
    pub trailing_bytes: u64,
    /// Every place where the file and its index disagree, in plain words. Empty
    /// when they agree.
    pub disagreements: Vec<String>,
}

impl Layout {
    /// The file holds every declared image where the index places it. The
    /// primary's declared size is not part of this (see the module docs).
    pub fn agrees(&self) -> bool {
        self.disagreements.is_empty()
    }

    /// `(declared, found)` when the primary's declared size differs from its real
    /// length. Not a disagreement: MPF offsets are relative to the MP header, so
    /// metadata added or removed in front of it after writing changes the
    /// primary's length without moving any later image, and nothing updates the
    /// size. Every multi-part capture in the corpus shows this.
    pub fn primary_size_stale(&self) -> Option<(u64, u64)> {
        let declared = self.index.entries.first()?.size;
        let found = self.primary_size_found?;
        (declared != found).then_some((declared, found))
    }
}

/// The DC-007 name of an MP type code, as ExifTool prints it.
pub fn type_name(code: u32) -> &'static str {
    match code {
        0x030000 => "Baseline MP Primary Image",
        0x010001 => "Large Thumbnail (VGA equivalent)",
        0x010002 => "Large Thumbnail (full HD equivalent)",
        0x020001 => "Multi-frame Panorama",
        0x020002 => "Multi-frame Disparity",
        0x020003 => "Multi-angle",
        0x000000 => "Undefined",
        _ => "Unknown",
    }
}

/// What an MP type name means, in plain words: `Undefined` is "an image of
/// undefined MPF type". Names this module does not produce are returned as written.
pub fn describe(type_name: &str) -> &str {
    match type_name {
        "Baseline MP Primary Image" => "primary image",
        "Large Thumbnail (VGA equivalent)" | "Large Thumbnail (full HD equivalent)" => {
            "large thumbnail"
        }
        "Multi-frame Panorama" => "panorama frame",
        "Multi-frame Disparity" => "disparity frame",
        "Multi-angle" => "multi-angle frame",
        "Undefined" => "image of undefined MPF type",
        "Unknown" => "image of unknown MPF type",
        other => other,
    }
}

/// Read the MPF index from the primary image's metadata segments. `Ok(None)`: the
/// file is a JPEG without one (most JPEGs). `Err`: not a JPEG, or an MPF segment
/// that cannot be read. Bounds-checked; never panics.
pub fn index(file: &[u8]) -> Result<Option<Index>, String> {
    let mut found: Option<(usize, &[u8])> = None;
    let mut extra = 0;
    for Segment {
        marker,
        start,
        payload,
    } in segments(file)?
    {
        if marker == 0xE2 && payload.starts_with(MPF_ID) {
            if found.is_none() {
                found = Some((start + MPF_ID.len(), &payload[MPF_ID.len()..]));
            } else {
                extra += 1;
            }
        }
    }
    let Some((header_offset, tiff)) = found else {
        return Ok(None);
    };
    let mut idx = parse_header(tiff, header_offset as u64)?;
    idx.extra_segments = extra;
    Ok(Some(idx))
}

/// Read the index and check every declared image against the bytes. `Ok(None)` and
/// `Err` as for [`index`].
pub fn analyze(file: &[u8]) -> Result<Option<Layout>, String> {
    Ok(index(file)?.map(|i| resolve(i, file)))
}

/// Check an index against `file`.
pub fn resolve(index: Index, file: &[u8]) -> Layout {
    let file_len = file.len() as u64;
    let mut disagreements = Vec::new();
    let mut placements = Vec::with_capacity(index.entries.len());

    if let Some(n) = index.number_of_images {
        if n as usize != index.entries.len() {
            disagreements.push(format!(
                "NumberOfImages says {n}, the MP entry table holds {}",
                index.entries.len()
            ));
        }
    }
    if index.extra_segments > 0 {
        disagreements.push(format!(
            "{} more MPF segment(s) after the first; only the first was read",
            index.extra_segments
        ));
    }
    if index.entries.is_empty() {
        disagreements.push("the MP entry table is empty".into());
    }

    let mut last_end = 0u64;
    let mut primary_size_found = None;
    for e in &index.entries {
        let end = e.offset.saturating_add(e.size);
        let present_bytes = end.min(file_len).saturating_sub(e.offset.min(file_len));
        let lo = usize::try_from(e.offset.min(file_len)).unwrap_or(file.len());
        let hi = usize::try_from(end.min(file_len)).unwrap_or(file.len());
        let bytes = &file[lo..hi];
        let starts_jpeg = bytes.starts_with(&[0xFF, 0xD8, 0xFF]);
        let label = format!("image {} ({})", e.index, describe(e.type_name));

        if e.index == 0 && e.raw_offset != 0 {
            disagreements.push(format!(
                "{label} has offset {} where the format requires 0",
                e.raw_offset
            ));
        }
        if e.index > 0 && e.offset < last_end {
            disagreements.push(format!(
                "{label} starts at byte {}, inside the image before it (which ends at {last_end})",
                e.offset
            ));
        }
        if e.index == 0 {
            // The primary's declared size is recorded, not checked: MPF places every
            // later image relative to the MP header, so bytes added or removed in
            // front of the header after writing (camera pipelines and metadata
            // editors both do this) leave this one size stale while every offset
            // stays right. Measured on every multi-part capture in the corpus.
            if !starts_jpeg {
                disagreements.push(format!("{label} does not start a JPEG at byte 0"));
            } else {
                primary_size_found = crate::jpeg::primary_end(file).map(|n| n as u64);
                if primary_size_found.is_none() {
                    disagreements.push(format!("{label} does not hold a complete JPEG"));
                }
            }
        } else if present_bytes < e.size {
            disagreements.push(if present_bytes == 0 {
                format!(
                    "{label} is declared at bytes {}..{end}, past the end of the file ({file_len} bytes)",
                    e.offset
                )
            } else {
                format!(
                    "{label} is cut short: {present_bytes} of {} declared bytes present",
                    e.size
                )
            });
        } else if e.data_format == 0 {
            if !starts_jpeg {
                disagreements.push(format!(
                    "{label} does not start a JPEG at byte {}",
                    e.offset
                ));
            } else {
                match crate::jpeg::primary_end(&file[lo..]) {
                    Some(n) if n as u64 == e.size => {}
                    Some(n) => disagreements.push(format!(
                        "{label}'s JPEG ends at byte {}; the index declares it ends at {end} \
                         ({} bytes declared, {} found)",
                        e.offset + n as u64,
                        e.size,
                        n
                    )),
                    None => disagreements.push(format!(
                        "{label} at byte {} does not hold a complete JPEG",
                        e.offset
                    )),
                }
            }
        }
        // The primary ends where its JPEG ends, whatever size the index gives it.
        let end = match (e.index, primary_size_found) {
            (0, Some(n)) => n,
            _ => end,
        };
        last_end = last_end.max(end);
        placements.push(Placement {
            index: e.index,
            type_name: e.type_name,
            offset: e.offset,
            size: e.size,
            present_bytes,
            starts_jpeg,
            self_described_as: if starts_jpeg && e.index > 0 {
                self_described(bytes)
            } else {
                Vec::new()
            },
        });
    }
    let trailing_bytes = file_len.saturating_sub(last_end);
    Layout {
        index,
        file_len,
        placements,
        primary_size_found,
        trailing_bytes,
        disagreements,
    }
}

/// What a JPEG's own APP1/APP2 segments say it is, from the markers in
/// `SELF_DESCRIPTIONS`, in that order. Reads the segments before the first scan
/// only, so entropy-coded data can never match. Unreadable input yields nothing.
pub fn self_described(jpeg: &[u8]) -> Vec<&'static str> {
    let Ok(segs) = segments(jpeg) else {
        return Vec::new();
    };
    SELF_DESCRIPTIONS
        .iter()
        .filter(|(needle, _)| {
            segs.iter()
                .filter(|s| matches!(s.marker, 0xE1 | 0xE2))
                .any(|s| contains(s.payload, needle))
        })
        .map(|(_, what)| *what)
        .collect()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// One metadata segment: its marker, where its payload starts in the buffer, and
/// the payload.
struct Segment<'a> {
    marker: u8,
    start: usize,
    payload: &'a [u8],
}

/// The metadata segments before the first SOS. Same walk as [`crate::jpeg::metadata`]: markers may be preceded by fill
/// bytes, standalone markers carry no length.
fn segments(b: &[u8]) -> Result<Vec<Segment<'_>>, String> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return Err("not a JPEG".into());
    }
    let mut out = Vec::new();
    let mut i = 2;
    while i + 1 < b.len() {
        if b[i] != 0xFF {
            return Err(format!("marker expected at {i}"));
        }
        let mut m = i + 1;
        while m < b.len() && b[m] == 0xFF {
            m += 1;
        }
        if m >= b.len() {
            break;
        }
        let marker = b[m];
        i = m + 1;
        match marker {
            0xD9 | 0xDA => break,
            0x01 | 0xD0..=0xD7 => continue,
            _ => {}
        }
        if i + 2 > b.len() {
            return Err("truncated segment header".into());
        }
        let len = u16::from_be_bytes([b[i], b[i + 1]]) as usize;
        if len < 2 || i + len > b.len() {
            return Err("bad segment length".into());
        }
        out.push(Segment {
            marker,
            start: i + 2,
            payload: &b[i + 2..i + len],
        });
        i += len;
    }
    Ok(out)
}

/// Integer reads in the MP header's byte order, bounds-checked.
struct Tiff<'a> {
    b: &'a [u8],
    be: bool,
}

impl Tiff<'_> {
    fn u16(&self, at: usize) -> Result<u16, String> {
        let s = self
            .b
            .get(at..at.checked_add(2).ok_or("offset overflow")?)
            .ok_or_else(|| format!("MP header truncated at {at}"))?;
        let a = [s[0], s[1]];
        Ok(if self.be {
            u16::from_be_bytes(a)
        } else {
            u16::from_le_bytes(a)
        })
    }
    fn u32(&self, at: usize) -> Result<u32, String> {
        let s = self
            .b
            .get(at..at.checked_add(4).ok_or("offset overflow")?)
            .ok_or_else(|| format!("MP header truncated at {at}"))?;
        let a = [s[0], s[1], s[2], s[3]];
        Ok(if self.be {
            u32::from_be_bytes(a)
        } else {
            u32::from_le_bytes(a)
        })
    }
}

/// Parse the MP header and MP Index IFD. `tiff` starts at the byte-order mark;
/// `header_offset` is where that is in the file.
fn parse_header(tiff: &[u8], header_offset: u64) -> Result<Index, String> {
    let be = match tiff.get(..4) {
        Some([b'M', b'M', 0x00, 0x2A]) => true,
        Some([b'I', b'I', 0x2A, 0x00]) => false,
        Some(_) => return Err("MPF segment without a TIFF byte-order header".into()),
        None => return Err("MPF segment too short for its header".into()),
    };
    let t = Tiff { b: tiff, be };
    let ifd = t.u32(4)? as usize;
    let count = t.u16(ifd)? as usize;
    if count > MAX_IFD_ENTRIES {
        return Err(format!(
            "MP index IFD declares {count} entries (at most {MAX_IFD_ENTRIES} read)"
        ));
    }
    let mut version = None;
    let mut number_of_images = None;
    let mut table: Option<(usize, usize)> = None; // (start, byte count)
    for k in 0..count {
        let at = ifd + 2 + 12 * k;
        let tag = t.u16(at)?;
        let count = t.u32(at + 4)? as usize;
        let value_at = at + 8;
        match tag {
            TAG_VERSION if count == 4 => {
                let v = tiff
                    .get(value_at..value_at + 4)
                    .ok_or("MPFVersion truncated")?;
                if v.iter().all(u8::is_ascii_graphic) {
                    version = Some(String::from_utf8_lossy(v).into_owned());
                }
            }
            TAG_NUMBER_OF_IMAGES => number_of_images = Some(t.u32(value_at)?),
            TAG_MP_ENTRY => {
                // Inline when it fits in four bytes, which no real table does.
                let start = if count <= 4 {
                    value_at
                } else {
                    t.u32(value_at)? as usize
                };
                table = Some((start, count));
            }
            _ => {}
        }
    }
    let Some((start, bytes)) = table else {
        return Err("MPF index has no MP entry table".into());
    };
    if bytes % ENTRY_LEN != 0 {
        return Err(format!(
            "MP entry table is {bytes} bytes, not a multiple of {ENTRY_LEN}"
        ));
    }
    let end = start
        .checked_add(bytes)
        .filter(|&e| e <= tiff.len())
        .ok_or("MP entry table runs past its segment")?;
    let mut entries = Vec::with_capacity(bytes / ENTRY_LEN);
    for (index, at) in (start..end).step_by(ENTRY_LEN).enumerate() {
        let attribute = t.u32(at)?;
        let size = u64::from(t.u32(at + 4)?);
        let raw_offset = t.u32(at + 8)?;
        let type_code = attribute & 0x00FF_FFFF;
        entries.push(Entry {
            index,
            attribute,
            type_code,
            type_name: type_name(type_code),
            data_format: ((attribute >> 24) & 0x7) as u8,
            dependent_parent: attribute & (1 << 31) != 0,
            dependent_child: attribute & (1 << 30) != 0,
            representative: attribute & (1 << 29) != 0,
            size,
            raw_offset,
            offset: if index == 0 {
                0
            } else {
                header_offset.saturating_add(u64::from(raw_offset))
            },
            dependents: [t.u16(at + 12)?, t.u16(at + 14)?],
        });
    }
    Ok(Index {
        header_offset,
        big_endian: be,
        version,
        number_of_images,
        entries,
        extra_segments: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut s = vec![0xFF, marker];
        s.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        s.extend_from_slice(payload);
        s
    }

    /// A small complete JPEG: optional extra segments, a scan with a stuffed FF00,
    /// EOI.
    fn tiny_jpeg(extra: &[Vec<u8>]) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8];
        for s in extra {
            b.extend_from_slice(s);
        }
        b.extend(segment(0xDA, &[1, 1, 0, 0, 63, 0]));
        b.extend_from_slice(&[0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD9]);
        b
    }

    /// MPF APP2 payload: header, IFD with version, count and the entry table.
    /// `entries`: (attribute, size, raw offset).
    fn mpf_payload(be: bool, entries: &[(u32, u32, u32)], claimed_n: u32) -> Vec<u8> {
        let w16 = |v: u16| if be { v.to_be_bytes() } else { v.to_le_bytes() };
        let w32 = |v: u32| if be { v.to_be_bytes() } else { v.to_le_bytes() };
        let mut t: Vec<u8> = if be {
            b"MM\x00\x2A".to_vec()
        } else {
            b"II\x2A\x00".to_vec()
        };
        t.extend_from_slice(&w32(8));
        let n_tags = 3u16;
        t.extend_from_slice(&w16(n_tags));
        let table_at = 8 + 2 + 12 * u32::from(n_tags) + 4;
        // MPFVersion
        t.extend_from_slice(&w16(TAG_VERSION));
        t.extend_from_slice(&w16(7));
        t.extend_from_slice(&w32(4));
        t.extend_from_slice(b"0100");
        // NumberOfImages
        t.extend_from_slice(&w16(TAG_NUMBER_OF_IMAGES));
        t.extend_from_slice(&w16(4));
        t.extend_from_slice(&w32(1));
        t.extend_from_slice(&w32(claimed_n));
        // MPEntry
        t.extend_from_slice(&w16(TAG_MP_ENTRY));
        t.extend_from_slice(&w16(7));
        t.extend_from_slice(&w32((entries.len() * ENTRY_LEN) as u32));
        t.extend_from_slice(&w32(table_at));
        t.extend_from_slice(&w32(0)); // next IFD
        for (attr, size, off) in entries {
            t.extend_from_slice(&w32(*attr));
            t.extend_from_slice(&w32(*size));
            t.extend_from_slice(&w32(*off));
            t.extend_from_slice(&w16(0));
            t.extend_from_slice(&w16(0));
        }
        let mut p = MPF_ID.to_vec();
        p.extend(t);
        p
    }

    /// Primary with an MPF index + a gain map after it, iPhone style (no container
    /// directory). Returns (file, primary length, gain map length).
    fn iphone_like(be: bool) -> (Vec<u8>, usize, usize) {
        let gain = tiny_jpeg(&[segment(
            0xE1,
            b"http://ns.adobe.com/xap/1.0/\0<x xmlns:HDRGainMap=\"http://ns.apple.com/HDRGainMap/1.0/\"/>",
        )]);
        // The primary's length depends only on the payload's length, which does
        // not depend on the values in it: build once to measure, then for real.
        let probe = tiny_jpeg(&[segment(0xE2, &mpf_payload(be, &[(0, 0, 0), (0, 0, 0)], 2))]);
        let primary_len = probe.len();
        // MP header sits after SOI (2), marker+length (4) and "MPF\0" (4).
        let header = 2 + 4 + 4;
        let entries = [
            (0x2003_0000, primary_len as u32, 0),
            (
                0x0000_0000,
                gain.len() as u32,
                (primary_len - header) as u32,
            ),
        ];
        let mut f = tiny_jpeg(&[segment(0xE2, &mpf_payload(be, &entries, 2))]);
        assert_eq!(f.len(), primary_len);
        f.extend_from_slice(&gain);
        (f, primary_len, gain.len())
    }

    #[test]
    fn an_iphone_style_index_agrees_in_both_byte_orders() {
        for be in [true, false] {
            let (f, p, g) = iphone_like(be);
            let l = analyze(&f).unwrap().unwrap();
            assert!(l.agrees(), "{:?}", l.disagreements);
            assert_eq!(l.index.version.as_deref(), Some("0100"));
            assert_eq!(l.index.header_offset, 10);
            let got: Vec<_> = l
                .placements
                .iter()
                .map(|p| (p.offset, p.size, p.type_name))
                .collect();
            assert_eq!(
                got,
                [
                    (0, p as u64, "Baseline MP Primary Image"),
                    (p as u64, g as u64, "Undefined")
                ]
            );
            assert!(l.index.entries[0].representative);
            assert_eq!(
                l.placements[1].self_described_as,
                ["Apple HDR gain map (XMP)"]
            );
            assert_eq!(l.trailing_bytes, 0);
        }
    }

    #[test]
    fn trailing_bytes_are_a_fact_not_a_disagreement() {
        let (mut f, ..) = iphone_like(true);
        let clip = b"\0\0\0\x18ftypmp42 a motion clip";
        f.extend_from_slice(clip);
        let l = analyze(&f).unwrap().unwrap();
        assert!(l.agrees());
        assert_eq!(l.trailing_bytes, clip.len() as u64);
    }

    #[test]
    fn a_removed_gain_map_is_reported_as_missing() {
        let (f, p, _) = iphone_like(true);
        let l = analyze(&f[..p]).unwrap().unwrap();
        assert!(!l.agrees());
        assert!(
            l.disagreements[0].contains("past the end of the file"),
            "{:?}",
            l.disagreements
        );
        assert_eq!(l.placements[1].present_bytes, 0);
    }

    #[test]
    fn a_cut_or_shifted_gain_map_disagrees() {
        let (f, p, g) = iphone_like(false);
        let l = analyze(&f[..p + g / 2]).unwrap().unwrap();
        assert!(
            l.disagreements[0].contains("cut short"),
            "{:?}",
            l.disagreements
        );

        let mut shifted = f[..p].to_vec();
        shifted.push(0x00);
        shifted.extend_from_slice(&f[p..]);
        let l = analyze(&shifted).unwrap().unwrap();
        assert!(
            l.disagreements
                .iter()
                .any(|d| d.contains("does not start a JPEG")),
            "{:?}",
            l.disagreements
        );
    }

    /// The corpus pattern: the primary's declared size is off (short on every
    /// Pixel capture, long after an ExifTool GPS strip) while image 1 sits where
    /// its offset says.
    fn stale(primary_delta: i64) -> (Vec<u8>, usize) {
        let (f, p, g) = iphone_like(true);
        let header = 10u32;
        let declared = (p as i64 + primary_delta) as u32;
        let entries = [(0x2003_0000, declared, 0), (0, g as u32, p as u32 - header)];
        let mut out = tiny_jpeg(&[segment(0xE2, &mpf_payload(true, &entries, 2))]);
        out.extend_from_slice(&f[p..]);
        (out, p)
    }

    #[test]
    fn a_stale_primary_size_is_recorded_not_a_disagreement() {
        for delta in [-7, 9] {
            let (f, p) = stale(delta);
            let l = analyze(&f).unwrap().unwrap();
            assert!(l.agrees(), "delta {delta}: {:?}", l.disagreements);
            assert_eq!(l.primary_size_found, Some(p as u64));
            assert_eq!(
                l.primary_size_stale(),
                Some(((p as i64 + delta) as u64, p as u64))
            );
            assert_eq!(l.trailing_bytes, 0);
        }
        let (f, ..) = iphone_like(true);
        assert_eq!(analyze(&f).unwrap().unwrap().primary_size_stale(), None);
    }

    #[test]
    fn a_stale_primary_size_does_not_hide_a_missing_gain_map() {
        let (f, p) = stale(-7);
        let l = analyze(&f[..p]).unwrap().unwrap();
        assert_eq!(l.disagreements.len(), 1, "{:?}", l.disagreements);
        assert!(l.disagreements[0].contains("past the end of the file"));
    }

    #[test]
    fn a_secondary_of_the_wrong_size_still_disagrees() {
        let (f, p, g) = iphone_like(true);
        let entries = [(0x2003_0000, p as u32, 0), (0, g as u32 - 3, p as u32 - 10)];
        let mut out = tiny_jpeg(&[segment(0xE2, &mpf_payload(true, &entries, 2))]);
        out.extend_from_slice(&f[p..]);
        let l = analyze(&out).unwrap().unwrap();
        assert_eq!(
            l.disagreements,
            [format!(
                "image 1 (image of undefined MPF type)'s JPEG ends at byte {}; the index \
                 declares it ends at {} ({} bytes declared, {g} found)",
                p + g,
                p + g - 3,
                g - 3
            )]
        );
    }

    #[test]
    fn a_broken_primary_still_disagrees() {
        let (mut f, ..) = iphone_like(true);
        // Cut inside the primary's entropy-coded data: the MPF segment is intact,
        // the primary has no EOI.
        let sos = f.windows(2).position(|w| w == [0xFF, 0xDA]).unwrap();
        f.truncate(sos + 9);
        let l = analyze(&f).unwrap().unwrap();
        assert!(
            l.disagreements
                .iter()
                .any(|d| d.contains("does not hold a complete JPEG")),
            "{:?}",
            l.disagreements
        );
        assert_eq!(l.primary_size_found, None);
    }

    #[test]
    fn a_wrong_image_count_disagrees() {
        let f = tiny_jpeg(&[segment(0xE2, &mpf_payload(true, &[(0x030000, 10, 0)], 3))]);
        let l = analyze(&f).unwrap().unwrap();
        assert!(
            l.disagreements
                .iter()
                .any(|d| d.contains("NumberOfImages says 3"))
        );
    }

    #[test]
    fn jpegs_without_an_index_have_none() {
        assert_eq!(index(&tiny_jpeg(&[])).unwrap(), None);
        assert_eq!(
            index(&tiny_jpeg(&[segment(0xE2, b"ICC_PROFILE\0...")])).unwrap(),
            None
        );
    }

    #[test]
    fn hostile_headers_are_errors_not_panics() {
        assert_eq!(index(b"PNG").unwrap_err(), "not a JPEG");
        for payload in [
            b"MPF\0".to_vec(),
            b"MPF\0XX\0\0\0\0\0\x08".to_vec(),
            b"MPF\0MM\0\x2A\xFF\xFF\xFF\xFF".to_vec(),
            b"MPF\0MM\0\x2A\0\0\0\x08\0\x01\xB0\x02\0\x07\xFF\xFF\xFF\xF0\0\0\0\0".to_vec(),
            b"MPF\0MM\0\x2A\0\0\0\x08\0\x01\xB0\x02\0\x07\0\0\0\x11\0\0\0\x1A".to_vec(),
            b"MPF\0II\x2A\0\x08\0\0\0\xFF\xFF".to_vec(),
        ] {
            let f = tiny_jpeg(&[segment(0xE2, &payload)]);
            assert!(index(&f).is_err(), "accepted {payload:?}");
        }
    }

    #[test]
    fn analyze_never_panics_on_any_prefix() {
        let (f, ..) = iphone_like(true);
        for n in 0..f.len() {
            let _ = analyze(&f[..n]);
        }
        // Offsets and sizes near u32::MAX: arithmetic must saturate, not wrap.
        let entries = [(0x030000, u32::MAX, 0), (0, u32::MAX, u32::MAX - 4)];
        let f = tiny_jpeg(&[segment(0xE2, &mpf_payload(true, &entries, 2))]);
        let l = analyze(&f).unwrap().unwrap();
        assert!(!l.agrees());
        assert_eq!(l.placements[1].present_bytes, 0);
    }

    #[test]
    fn type_names_read_as_plain_words() {
        assert_eq!(describe(type_name(0x030000)), "primary image");
        assert_eq!(describe(type_name(0)), "image of undefined MPF type");
        assert_eq!(describe(type_name(0x010002)), "large thumbnail");
        assert_eq!(type_name(0x123456), "Unknown");
    }
}
