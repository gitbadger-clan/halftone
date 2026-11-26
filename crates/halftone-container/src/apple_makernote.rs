//! Apple's MakerNote: the block an iPhone or iPad camera writes into the EXIF
//! `MakerNote` tag. A reader for the few fields the sources list, not a decoder of
//! the block.
//!
//! Layout, as read from six camera files of an iPad on iPadOS 18.7.3 (06-phones,
//! 2026-11-26; MakerNote version 15) and from ExifTool's public `Apple.jpg` (an
//! iPhone 7 on iOS 10, version 4): the bytes `Apple iOS`, a NUL, two more bytes, the
//! byte-order mark `MM`, then one big-endian TIFF directory: a 16-bit entry count and
//! twelve bytes per entry (tag, type, count, value or offset). Offsets count from the
//! first byte of the block, not from the TIFF header of the EXIF around it, so the
//! block reads the same wherever a writer moves it.
//!
//! Fields read, by tag:
//!
//! | Tag | ExifTool's name | What it holds | Seen in |
//! |---|---|---|---|
//! | `0x0003` | RunTime | a binary property list: the time since the device was switched on, as a count (`value`) of `1/timescale` seconds | all seven files |
//! | `0x0011` | ContentIdentifier | a UUID as text, written when the still was saved with a Live Photo clip | the two Live Photo files, none of the four others |
//! | `0x0020` | ImageCaptureRequestID | a UUID as text, one per capture | the six iPad files |
//! | `0x002b` | PhotoIdentifier | a UUID as text, one per photo | the six iPad files |
//!
//! The names are ExifTool's, which is also the independent reader the differential
//! test compares against. Everything else in the block is left alone: exposure and
//! focus state, the acceleration vector, scene flags and some twenty entries nobody
//! outside Apple has named.
//!
//! Nothing here is trusted. Counts and offsets are bounded and checked against the
//! block before any byte is read, and a field that points outside the block is
//! reported as unread, never guessed.

use crate::embedded::tidy;

/// How a MakerNote of Apple's begins.
const HEADER: &[u8] = b"Apple iOS\0";
/// Where the directory's entry count sits: after the header, two bytes and `MM`.
const DIRECTORY_AT: usize = 14;
/// More entries than any file seen (45 to 47); a larger count is a damaged block.
const MAX_ENTRIES: usize = 512;
/// Longest text value accepted. The identifiers are 36 characters.
const MAX_TEXT: usize = 128;
/// Largest property list accepted. The one seen is 104 bytes.
const MAX_PLIST: usize = 4096;

const TAG_RUN_TIME: u16 = 0x0003;
const TAG_CONTENT_IDENTIFIER: u16 = 0x0011;
const TAG_CAPTURE_REQUEST_ID: u16 = 0x0020;
const TAG_PHOTO_IDENTIFIER: u16 = 0x002b;

const TYPE_ASCII: u16 = 2;
const TYPE_UNDEFINED: u16 = 7;

/// The time since the device was switched on, as the camera wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunTime {
    /// The count, in units of `1/timescale` seconds.
    pub value: i64,
    /// Units per second; a thousand million in every file seen.
    pub timescale: Option<i64>,
}

/// The listed fields of one Apple MakerNote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppleMakerNote {
    /// Entries in the block's directory.
    pub entries: usize,
    /// `RunTime`: the time since power-on.
    pub run_time: Option<RunTime>,
    /// `ContentIdentifier`: present when the still was saved with a Live Photo clip.
    pub content_identifier: Option<String>,
    /// `ImageCaptureRequestID`.
    pub capture_request_id: Option<String>,
    /// `PhotoIdentifier`.
    pub photo_identifier: Option<String>,
    /// Listed fields whose entry is there and could not be read (a value outside the
    /// block, an unexpected type, a property list that does not parse), by
    /// ExifTool's name. Empty when everything that is there was read.
    pub unread: Vec<&'static str>,
    /// The directory itself could not be walked to its end: the block is cut short or
    /// its entry count is not believable. Fields found before that point are kept.
    pub truncated: bool,
}

impl AppleMakerNote {
    /// True when something that is there could not be read, so that a missing field
    /// is not a finding.
    pub fn incomplete(&self) -> bool {
        self.truncated || !self.unread.is_empty()
    }
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Read the listed fields from the bytes of an EXIF `MakerNote` tag. `None` when
/// the block is not Apple's (another maker's, or too short to say).
pub fn parse(block: &[u8]) -> Option<AppleMakerNote> {
    if !block.starts_with(HEADER) || block.get(12..14) != Some(b"MM") {
        return None;
    }
    let mut note = AppleMakerNote::default();
    let Some(count) = be16(block, DIRECTORY_AT).map(usize::from) else {
        note.truncated = true;
        return Some(note);
    };
    if count > MAX_ENTRIES {
        note.truncated = true;
        return Some(note);
    }
    note.entries = count;
    for i in 0..count {
        let at = DIRECTORY_AT + 2 + 12 * i;
        let (Some(tag), Some(ty), Some(n)) =
            (be16(block, at), be16(block, at + 2), be32(block, at + 4))
        else {
            note.truncated = true;
            break;
        };
        let name = match tag {
            TAG_RUN_TIME => "RunTime",
            TAG_CONTENT_IDENTIFIER => "ContentIdentifier",
            TAG_CAPTURE_REQUEST_ID => "ImageCaptureRequestID",
            TAG_PHOTO_IDENTIFIER => "PhotoIdentifier",
            _ => continue,
        };
        // One byte per unit for both types read here; four bytes or fewer sit in the
        // entry itself, anything longer where the entry points.
        let len = n as usize;
        let value: Option<&[u8]> = if len <= 4 {
            block.get(at + 8..at + 8 + len)
        } else {
            be32(block, at + 8)
                .and_then(|off| block.get(off as usize..(off as usize).checked_add(len)?))
        };
        let read = match (tag, ty, value) {
            (TAG_RUN_TIME, TYPE_UNDEFINED, Some(v)) if len <= MAX_PLIST => run_time(v).map(|r| {
                note.run_time = Some(r);
            }),
            (TAG_RUN_TIME, ..) => None,
            (_, TYPE_ASCII, Some(v)) if len <= MAX_TEXT => text(v).map(|s| match tag {
                TAG_CONTENT_IDENTIFIER => note.content_identifier = Some(s),
                TAG_CAPTURE_REQUEST_ID => note.capture_request_id = Some(s),
                _ => note.photo_identifier = Some(s),
            }),
            _ => None,
        };
        if read.is_none() && !note.unread.contains(&name) {
            note.unread.push(name);
        }
    }
    Some(note)
}

/// A text value: printable ASCII up to the first NUL, not empty.
fn text(v: &[u8]) -> Option<String> {
    let end = v.iter().position(|b| *b == 0).unwrap_or(v.len());
    let s = &v[..end];
    if s.is_empty() || !s.iter().all(|b| (0x20..0x7F).contains(b)) {
        return None;
    }
    let t = tidy(&String::from_utf8_lossy(s));
    (!t.is_empty()).then_some(t)
}

/// `RunTime` from its binary property list: a dictionary of whole numbers under the
/// keys `value`, `timescale`, `epoch` and `flags`. Only the first two are kept.
fn run_time(plist: &[u8]) -> Option<RunTime> {
    let numbers = plist_numbers(plist)?;
    let get = |key: &str| numbers.iter().find(|(k, _)| k == key).map(|(_, v)| *v);
    Some(RunTime {
        value: get("value")?,
        timescale: get("timescale"),
    })
}

/// The whole-number entries of a binary property list whose top object is a
/// dictionary with text keys: enough of the format for `RunTime`, and no more.
/// Entries of any other kind are skipped. `None` when the list does not parse.
fn plist_numbers(p: &[u8]) -> Option<Vec<(String, i64)>> {
    const TRAILER: usize = 32;
    if !p.starts_with(b"bplist00") || p.len() < 8 + TRAILER {
        return None;
    }
    let t = &p[p.len() - TRAILER..];
    let (offset_size, ref_size) = (usize::from(t[6]), usize::from(t[7]));
    let objects = usize::try_from(u64::from_be_bytes(t[8..16].try_into().ok()?)).ok()?;
    let top = usize::try_from(u64::from_be_bytes(t[16..24].try_into().ok()?)).ok()?;
    let table = usize::try_from(u64::from_be_bytes(t[24..32].try_into().ok()?)).ok()?;
    if !(1..=8).contains(&offset_size)
        || !(1..=8).contains(&ref_size)
        || objects > 64
        || top >= objects
    {
        return None;
    }
    let uint = |at: usize, size: usize| -> Option<usize> {
        let b = p.get(at..at.checked_add(size)?)?;
        usize::try_from(b.iter().fold(0u64, |acc, x| (acc << 8) | u64::from(*x))).ok()
    };
    // Where object `i` starts, from the offset table.
    let object = |i: usize| -> Option<usize> {
        if i >= objects {
            return None;
        }
        uint(table.checked_add(i.checked_mul(offset_size)?)?, offset_size)
            .filter(|o| *o >= 8 && *o < p.len() - TRAILER)
    };
    let at = object(top)?;
    let marker = *p.get(at)?;
    // A dictionary: 0xD in the high half, the number of pairs in the low half. Fifteen
    // or more pairs are written another way, which `RunTime` never needs.
    if marker >> 4 != 0xD || marker & 0xF == 0xF {
        return None;
    }
    let pairs = usize::from(marker & 0xF);
    let mut out = Vec::with_capacity(pairs);
    for i in 0..pairs {
        let key_ref = uint(at + 1 + i * ref_size, ref_size)?;
        let value_ref = uint(at + 1 + (pairs + i) * ref_size, ref_size)?;
        let (Some(k), Some(v)) = (object(key_ref), object(value_ref)) else {
            return None;
        };
        // A key: ASCII text, 0x5 in the high half, its length in the low half.
        let km = *p.get(k)?;
        if km >> 4 != 0x5 || km & 0xF == 0xF {
            continue;
        }
        let key = p.get(k + 1..k + 1 + usize::from(km & 0xF))?;
        // A whole number: 0x1 in the high half, the low half n for 2^n bytes.
        let vm = *p.get(v)?;
        if vm >> 4 != 0x1 || vm & 0xF > 3 {
            continue;
        }
        let size = 1usize << (vm & 0xF);
        let bytes = p.get(v + 1..v + 1 + size)?;
        // One, two and four bytes are unsigned; eight bytes are a signed number.
        let number = match <[u8; 8]>::try_from(bytes) {
            Ok(eight) => i64::from_be_bytes(eight),
            Err(_) => bytes.iter().fold(0i64, |acc, x| (acc << 8) | i64::from(*x)),
        };
        out.push((String::from_utf8_lossy(key).into_owned(), number));
    }
    Some(out)
}

/// Builders for tests here and in the sources that read the block.
#[cfg(test)]
pub(crate) mod testutil {
    /// A `RunTime` property list as the camera lays it out: four keys, then four
    /// whole numbers, the offset table and the trailer.
    pub fn run_time_plist(value: i64, timescale: u32) -> Vec<u8> {
        let mut p = b"bplist00".to_vec();
        let mut offsets = Vec::new();
        let mut put = |p: &mut Vec<u8>, bytes: &[u8]| {
            offsets.push(p.len() as u8);
            p.extend_from_slice(bytes);
        };
        // Object 0: the dictionary, keys 1..=4, values 5..=8.
        put(&mut p, &[0xD4, 1, 2, 3, 4, 5, 6, 7, 8]);
        for key in ["flags", "value", "timescale", "epoch"] {
            let mut k = vec![0x50 | key.len() as u8];
            k.extend_from_slice(key.as_bytes());
            put(&mut p, &k);
        }
        put(&mut p, &[0x10, 1]);
        let mut v = vec![0x13];
        v.extend_from_slice(&value.to_be_bytes());
        put(&mut p, &v);
        let mut s = vec![0x12];
        s.extend_from_slice(&timescale.to_be_bytes());
        put(&mut p, &s);
        put(&mut p, &[0x10, 0]);
        let table = p.len() as u64;
        p.extend_from_slice(&offsets);
        p.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 1]);
        p.extend_from_slice(&(offsets.len() as u64).to_be_bytes());
        p.extend_from_slice(&0u64.to_be_bytes());
        p.extend_from_slice(&table.to_be_bytes());
        p
    }

    /// An Apple MakerNote with the given entries: (tag, type, data). Data longer than
    /// four bytes is placed after the directory, as the camera does.
    pub fn block(entries: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
        let mut b = b"Apple iOS\0\0\x01MM".to_vec();
        b.extend_from_slice(&(entries.len() as u16).to_be_bytes());
        let mut data_at = b.len() + 12 * entries.len() + 4;
        let mut tail = Vec::new();
        for (tag, ty, data) in entries {
            b.extend_from_slice(&tag.to_be_bytes());
            b.extend_from_slice(&ty.to_be_bytes());
            b.extend_from_slice(&(data.len() as u32).to_be_bytes());
            if data.len() <= 4 {
                let mut v = data.clone();
                v.resize(4, 0);
                b.extend_from_slice(&v);
            } else {
                b.extend_from_slice(&(data_at as u32).to_be_bytes());
                tail.extend_from_slice(data);
                data_at += data.len();
            }
        }
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend(tail);
        b
    }

    pub fn ascii(s: &str) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        v
    }

    pub const PHOTO: &str = "11111111-2222-4333-8444-555555555555";
    pub const REQUEST: &str = "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE";
    pub const CONTENT: &str = "01234567-89AB-4CDE-8F01-23456789ABCD";
    pub const TICKS: i64 = 64_783_384_749_708;

    /// The block as the iPad wrote it for a still: run time, an entry that is not
    /// read, the capture request and the photo identifier; with `live`, the content
    /// identifier too.
    pub fn camera(live: bool) -> Vec<u8> {
        let mut e = vec![
            (0x0001, 9, vec![0, 0, 0, 15]),
            (0x0003, 7, run_time_plist(TICKS, 1_000_000_000)),
            (0x0008, 10, vec![0xFF; 24]),
        ];
        if live {
            e.push((0x0011, 2, ascii(CONTENT)));
        }
        e.push((0x0020, 2, ascii(REQUEST)));
        e.push((0x002b, 2, ascii(PHOTO)));
        block(&e)
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    #[test]
    fn the_blocks_of_a_still_and_of_a_live_photo_are_read() {
        let still = parse(&camera(false)).unwrap();
        assert_eq!(still.entries, 5);
        assert_eq!(
            still.run_time,
            Some(RunTime {
                value: TICKS,
                timescale: Some(1_000_000_000)
            })
        );
        assert_eq!(still.photo_identifier.as_deref(), Some(PHOTO));
        assert_eq!(still.capture_request_id.as_deref(), Some(REQUEST));
        assert_eq!(still.content_identifier, None);
        assert!(!still.incomplete());

        let live = parse(&camera(true)).unwrap();
        assert_eq!(live.entries, 6);
        assert_eq!(live.content_identifier.as_deref(), Some(CONTENT));
        assert_eq!(live.photo_identifier.as_deref(), Some(PHOTO));
        assert!(!live.incomplete());
    }

    #[test]
    fn another_makers_block_is_not_apples() {
        assert_eq!(parse(b""), None);
        assert_eq!(parse(b"Nikon\0\x02\x10\0\0MM\0\x2A\0\0\0\x08"), None);
        assert_eq!(parse(b"Apple iOS"), None);
        // The header without the byte-order mark the directory is read by.
        assert_eq!(parse(b"Apple iOS\0\0\x01II\x00\x00"), None);
    }

    #[test]
    fn an_empty_block_of_apples_has_nothing_and_is_complete() {
        let n = parse(&block(&[])).unwrap();
        assert_eq!(n, AppleMakerNote::default());
        assert!(!n.incomplete());
    }

    #[test]
    fn a_block_cut_short_keeps_what_came_before_and_says_so() {
        let whole = camera(true);
        // Cut inside the directory: the first entries are read, the rest is gone.
        let cut = &whole[..DIRECTORY_AT + 2 + 12 * 2 + 5];
        let n = parse(cut).unwrap();
        assert!(n.truncated && n.incomplete());
        assert_eq!(n.photo_identifier, None);
        // Cut after the directory, inside the values: the entries are all there and
        // the ones whose values are gone are named.
        let dir_end = DIRECTORY_AT + 2 + 12 * 6 + 4;
        let n = parse(&whole[..dir_end + 10]).unwrap();
        assert!(!n.truncated);
        assert_eq!(
            n.unread,
            [
                "RunTime",
                "ContentIdentifier",
                "ImageCaptureRequestID",
                "PhotoIdentifier"
            ]
        );
        // Header only, no entry count.
        let n = parse(&whole[..DIRECTORY_AT]).unwrap();
        assert!(n.truncated);
        assert_eq!(n.entries, 0);
    }

    #[test]
    fn a_count_no_camera_writes_is_not_walked() {
        let mut b = b"Apple iOS\0\0\x01MM".to_vec();
        b.extend_from_slice(&0xFFFFu16.to_be_bytes());
        b.resize(4096, 0);
        let n = parse(&b).unwrap();
        assert!(n.truncated);
        assert_eq!(n.entries, 0);
    }

    #[test]
    fn a_value_that_points_outside_the_block_is_unread_not_guessed() {
        let mut b = block(&[(0x002b, 2, ascii(PHOTO))]);
        // The entry's offset: far past the end, then at the very end.
        let off_at = DIRECTORY_AT + 2 + 8;
        b[off_at..off_at + 4].copy_from_slice(&0xFFFF_FFF0u32.to_be_bytes());
        let n = parse(&b).unwrap();
        assert_eq!(n.photo_identifier, None);
        assert_eq!(n.unread, ["PhotoIdentifier"]);
        let end = b.len() as u32;
        b[off_at..off_at + 4].copy_from_slice(&end.to_be_bytes());
        assert_eq!(parse(&b).unwrap().unread, ["PhotoIdentifier"]);
    }

    #[test]
    fn a_field_of_the_wrong_type_or_with_no_text_is_unread() {
        // The identifier as a number, as bytes that are not text, and as an empty text.
        for entry in [
            (0x002b, 4, vec![0, 0, 0, 7]),
            (0x002b, 2, vec![0xFF, 0xFE, 0x80, 0x81, 0x82, 0]),
            (0x002b, 2, vec![0]),
            (0x002b, 2, vec![b'x'; MAX_TEXT + 1]),
        ] {
            let n = parse(&block(std::slice::from_ref(&entry))).unwrap();
            assert_eq!(n.photo_identifier, None, "{entry:?}");
            assert_eq!(n.unread, ["PhotoIdentifier"], "{entry:?}");
        }
        // Run time as text, and as a property list that is not one.
        for entry in [
            (0x0003, 2, ascii("soon")),
            (
                0x0003,
                7,
                b"bplist00 and then nothing that parses at all, forty bytes".to_vec(),
            ),
            (0x0003, 7, vec![0; 3]),
        ] {
            let n = parse(&block(std::slice::from_ref(&entry))).unwrap();
            assert_eq!(n.run_time, None, "{entry:?}");
            assert_eq!(n.unread, ["RunTime"], "{entry:?}");
        }
    }

    #[test]
    fn a_short_text_value_is_read_from_the_entry_itself() {
        let n = parse(&block(&[(0x0011, 2, ascii("abc"))])).unwrap();
        assert_eq!(n.content_identifier.as_deref(), Some("abc"));
    }

    #[test]
    fn the_property_list_reader_takes_whole_numbers_and_skips_the_rest() {
        let p = run_time_plist(-5, 1000);
        let got = plist_numbers(&p).unwrap();
        assert_eq!(
            got,
            [
                ("flags".to_string(), 1),
                ("value".to_string(), -5),
                ("timescale".to_string(), 1000),
                ("epoch".to_string(), 0)
            ]
        );
        // A list without the value key is not a run time.
        let mut renamed = p.clone();
        let at = renamed.windows(5).position(|w| w == b"value").unwrap();
        renamed[at..at + 5].copy_from_slice(b"walue");
        assert_eq!(run_time(&renamed), None);
        // Every truncation either fails or still parses; none may panic.
        for cut in 0..p.len() {
            let _ = plist_numbers(&p[..cut]);
        }
        // A trailer that points the offset table, or the top object, outside the list.
        let n = p.len();
        for (at, value) in [
            (n - 8, u64::MAX),
            (n - 8, n as u64),
            (n - 16, 99),
            (n - 24, 1000),
        ] {
            let mut bad = p.clone();
            bad[at..at + 8].copy_from_slice(&value.to_be_bytes());
            assert_eq!(plist_numbers(&bad), None, "trailer field at {at} = {value}");
        }
    }

    #[test]
    fn no_slice_of_a_real_layout_makes_the_reader_panic() {
        // Every prefix, and every single-byte change in the directory and the trailer
        // of the property list: the reader may refuse, it may not crash.
        let whole = camera(true);
        for cut in 0..whole.len() {
            let _ = parse(&whole[..cut]);
        }
        for at in 0..whole.len() {
            for flip in [0x00u8, 0x7F, 0xFF] {
                let mut b = whole.clone();
                b[at] = flip;
                let _ = parse(&b);
            }
        }
    }
}
