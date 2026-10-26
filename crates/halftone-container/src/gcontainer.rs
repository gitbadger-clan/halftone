//! The Google container directory (`Container:Directory` in the primary image's
//! XMP): what a Motion Photo or Ultra HDR JPEG says it contains, checked against
//! the bytes it actually contains.
//!
//! Layout rule (Motion Photo format 1.0, developer.android.com/media/platform/
//! motion-photo-format): items are stored in directory order, tightly packed. An
//! item starts at the end of the primary image's encoding, plus the primary's
//! `Padding` if given, plus the `Length` of every earlier secondary item. A
//! secondary item with `Length` 0 shares the previous item's resource. Only the
//! first item may carry `Padding`. A `MotionPhoto` item ends the file: no bytes
//! may follow it.
//!
//! The directory is unsigned metadata the writer put there. A directory that
//! agrees with the bytes says nothing about whether the content was edited; one
//! that disagrees says the file no longer matches its own description, not why.
//! Pure: no I/O, never panics on hostile input.

use crate::marking::jpeg_xmp_packets;

/// Namespace of `Container:Directory` and `Container:Item`.
pub const CONTAINER_NS: &str = "http://ns.google.com/photos/1.0/container/";
/// Namespace of the `Item:*` attributes.
pub const ITEM_NS: &str = "http://ns.google.com/photos/1.0/container/item/";
/// Directories with more items than this are rejected as implausible.
const MAX_ITEMS: usize = 64;

/// One `Container:Item` as written in the directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// `Item:Semantic`, e.g. `Primary`, `GainMap`, `MotionPhoto`.
    pub semantic: String,
    /// `Item:Mime`, e.g. `image/jpeg`, `video/mp4`.
    pub mime: String,
    /// `Item:Length` in bytes, if present.
    pub length: Option<u64>,
    /// `Item:Padding` in bytes, if present.
    pub padding: Option<u64>,
}

/// The directory as written, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    /// The items, in directory order.
    pub items: Vec<Item>,
}

/// Whether the bytes where an item should start look like its declared type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Magic {
    /// The signature for the declared MIME type is there (JPEG `FF D8 FF`;
    /// MP4/QuickTime `ftyp` at offset 4).
    Matches,
    /// Some other bytes are there.
    Differs,
    /// Not checked: the item is not in the file, shares its resource, or its MIME
    /// type has no signature here.
    NotChecked,
}

/// Where one secondary item should be, and what is there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    /// Position in the directory (0 is the primary).
    pub index: usize,
    /// `Item:Semantic`.
    pub semantic: String,
    /// `Item:Mime`.
    pub mime: String,
    /// Where the item should start, by the layout rule.
    pub offset: u64,
    /// Declared length (0 for an item sharing the previous resource).
    pub length: u64,
    /// How many of its bytes the file has (`length` when complete).
    pub present_bytes: u64,
    /// What the bytes at `offset` look like.
    pub magic: Magic,
}

/// The directory checked against the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The directory as written.
    pub directory: Directory,
    /// End of the primary image's encoding (just past its EOI).
    pub primary_end: u64,
    /// Every secondary item's expected place and what is found there.
    pub placements: Vec<Placement>,
    /// Where the declared items end.
    pub declared_end: u64,
    /// The file's length.
    pub file_len: u64,
    /// Each way the file differs from its own directory, in plain words. Empty
    /// when every declared item is where the directory says, complete, and the
    /// file ends where the items end.
    pub disagreements: Vec<String>,
}

impl Layout {
    /// The file matches its directory in every checked respect.
    pub fn agrees(&self) -> bool {
        self.disagreements.is_empty()
    }
}

/// Read a JPEG's container directory and check it against the file.
/// `Ok(None)`: no container directory (most files). `Err`: the file is not a JPEG
/// or the directory cannot be read.
pub fn analyze(file: &[u8]) -> Result<Option<Layout>, String> {
    let packets = jpeg_xmp_packets(file)?;
    let mut found = None;
    for p in &packets {
        let text = String::from_utf8_lossy(p);
        if let Some(d) = parse_directory(&text)? {
            if found.is_some() {
                return Err("more than one container directory".into());
            }
            found = Some(d);
        }
    }
    let Some(directory) = found else {
        return Ok(None);
    };
    let primary_end =
        jpeg_primary_end(file).ok_or("the primary image's end (EOI) could not be found")?;
    Ok(Some(resolve(directory, primary_end as u64, file)))
}

/// Parse `Container:Directory` from one XMP packet. `Ok(None)` when the packet
/// has no directory. Prefixes are resolved through their `xmlns` declarations, so
/// a writer using other prefixes than `Container`/`Item` is read correctly.
pub fn parse_directory(xmp: &str) -> Result<Option<Directory>, String> {
    let (Some(c), Some(i)) = (prefix_for(xmp, CONTAINER_NS), prefix_for(xmp, ITEM_NS)) else {
        return Ok(None);
    };
    let open = format!("<{c}:Directory");
    let Some(start) = find_tag(xmp, &open, 0) else {
        return Ok(None);
    };
    let close = format!("</{c}:Directory>");
    let end = xmp[start..]
        .find(&close)
        .map(|e| start + e)
        .ok_or("Container:Directory is not closed")?;
    let body = &xmp[start..end];
    let item_open = format!("<{c}:Item");
    let item_close = format!("</{c}:Item>");
    let mut items = Vec::new();
    let mut at = 0;
    while let Some(s) = find_tag(body, &item_open, at) {
        if items.len() == MAX_ITEMS {
            return Err(format!("more than {MAX_ITEMS} container items"));
        }
        let tag_end = body[s..]
            .find('>')
            .map(|e| s + e)
            .ok_or("Container:Item tag is not closed")?;
        let tag = &body[s..=tag_end];
        let (inner, next) = if tag.ends_with("/>") {
            ("", tag_end + 1)
        } else {
            let e = body[tag_end..]
                .find(&item_close)
                .map(|e| tag_end + e)
                .ok_or("Container:Item is not closed")?;
            (&body[tag_end + 1..e], e + item_close.len())
        };
        let field = |name: &str| attribute(tag, i, name).or_else(|| element(inner, i, name));
        let number = |name: &str| -> Result<Option<u64>, String> {
            field(name)
                .map(|v| {
                    v.trim()
                        .parse::<u64>()
                        .map_err(|_| format!("Item:{name} is not a non-negative integer: {v:?}"))
                })
                .transpose()
        };
        items.push(Item {
            semantic: field("Semantic").ok_or("a container item has no Item:Semantic")?,
            mime: field("Mime").ok_or("a container item has no Item:Mime")?,
            length: number("Length")?,
            padding: number("Padding")?,
        });
        at = next;
    }
    if items.is_empty() {
        return Err("Container:Directory has no items".into());
    }
    Ok(Some(Directory { items }))
}

/// Apply the layout rule and compare with the file.
pub fn resolve(directory: Directory, primary_end: u64, file: &[u8]) -> Layout {
    let file_len = file.len() as u64;
    let mut disagreements = Vec::new();
    let items = &directory.items;
    let Some(first) = items.first() else {
        return Layout {
            primary_end,
            placements: Vec::new(),
            declared_end: primary_end,
            file_len,
            disagreements: vec!["the container directory has no items".into()],
            directory,
        };
    };
    if first.semantic != "Primary" {
        disagreements.push(format!(
            "the first directory item is {:?}, not the primary image",
            first.semantic
        ));
    }
    if let Some(n) = items.iter().skip(1).position(|it| it.semantic == "Primary") {
        disagreements.push(format!("item {} is a second primary image", n + 1));
    }
    if let Some(n) = items
        .iter()
        .skip(1)
        .position(|it| it.padding.unwrap_or(0) != 0)
    {
        disagreements.push(format!(
            "item {} declares padding, which only the first item may",
            n + 1
        ));
    }
    let mut at = primary_end.saturating_add(first.padding.unwrap_or(0));
    let mut stopped = false;
    let mut prev_start = at;
    let mut placements = Vec::new();
    for (index, it) in items.iter().enumerate().skip(1) {
        let Some(length) = it.length else {
            disagreements.push(format!(
                "item {index} ({}) has no Item:Length, so its place cannot be computed",
                it.semantic
            ));
            stopped = true;
            break;
        };
        // Length 0: shares the previous item's resource.
        let offset = if length == 0 { prev_start } else { at };
        let present = if offset >= file_len {
            0
        } else {
            length.min(file_len - offset)
        };
        let magic = if length == 0 || present == 0 {
            Magic::NotChecked
        } else {
            magic_at(file, offset, &it.mime)
        };
        if length != 0 {
            if present == 0 {
                disagreements.push(format!(
                    "item {index} ({}, {} bytes) is declared at byte {offset}, but the file ends at byte {file_len}",
                    it.semantic,
                    thousands(length)
                ));
            } else if present < length {
                disagreements.push(format!(
                    "item {index} ({}) is cut short: {} of {} bytes present",
                    it.semantic,
                    thousands(present),
                    thousands(length)
                ));
            }
            if magic == Magic::Differs {
                disagreements.push(format!(
                    "the bytes at {offset}, where item {index} ({}) should start, are not {}",
                    it.semantic, it.mime
                ));
            }
            prev_start = offset;
            at = offset.saturating_add(length);
        }
        placements.push(Placement {
            index,
            semantic: it.semantic.clone(),
            mime: it.mime.clone(),
            offset,
            length,
            present_bytes: present,
            magic,
        });
    }
    let declared_end = at;
    // Only meaningful when every item was placed: a missing or cut-short item
    // already puts the declared end past the file.
    if !stopped && declared_end < file_len {
        let after = if items.iter().any(|it| it.semantic == "MotionPhoto") {
            ", where the motion clip should end the file"
        } else {
            ""
        };
        disagreements.push(format!(
            "the declared items end at byte {}{after}, but the file has {} bytes",
            thousands(declared_end),
            thousands(file_len)
        ));
    }
    Layout {
        directory,
        primary_end,
        placements,
        declared_end,
        file_len,
        disagreements,
    }
}

/// End of the first complete JPEG in `b` (just past its EOI), walking marker
/// segments and skipping entropy-coded data, so a later embedded JPEG (a gain map,
/// a thumbnail) is never mistaken for the end of the primary.
pub fn jpeg_primary_end(b: &[u8]) -> Option<usize> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    loop {
        if *b.get(i)? != 0xFF {
            return None;
        }
        let mut m = i + 1;
        while *b.get(m)? == 0xFF {
            m += 1;
        }
        let marker = b[m];
        i = m + 1;
        match marker {
            0xD9 => return Some(i),
            0x01 | 0xD0..=0xD7 => continue,
            _ => {}
        }
        let len = usize::from(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]));
        if len < 2 {
            return None;
        }
        i = i.checked_add(len).filter(|&n| n <= b.len())?;
        if marker == 0xDA {
            // Entropy-coded data runs until a marker that is not a stuffed 0xFF00,
            // a restart marker, or fill bytes.
            loop {
                let p = i + b.get(i..)?.iter().position(|&c| c == 0xFF)?;
                match *b.get(p + 1)? {
                    0x00 | 0xD0..=0xD7 => i = p + 2,
                    0xFF => i = p + 1,
                    _ => {
                        i = p;
                        break;
                    }
                }
            }
        }
    }
}

/// Which signature, if any, the bytes at `offset` carry for `mime`.
fn magic_at(file: &[u8], offset: u64, mime: &str) -> Magic {
    let Ok(o) = usize::try_from(offset) else {
        return Magic::NotChecked;
    };
    let at = file.get(o..).unwrap_or(&[]);
    match mime {
        "image/jpeg" => {
            if at.starts_with(&[0xFF, 0xD8, 0xFF]) {
                Magic::Matches
            } else {
                Magic::Differs
            }
        }
        "video/mp4" | "video/quicktime" => {
            if at.get(4..8) == Some(b"ftyp".as_slice()) {
                Magic::Matches
            } else {
                Magic::Differs
            }
        }
        _ => Magic::NotChecked,
    }
}

/// The prefix bound to `uri` by an `xmlns:prefix="uri"` declaration.
fn prefix_for<'a>(xmp: &'a str, uri: &str) -> Option<&'a str> {
    let mut rest = xmp;
    while let Some(p) = rest.find("xmlns:") {
        let decl = &rest[p + 6..];
        let eq = decl.find('=')?;
        let name = decl[..eq].trim();
        let value = decl[eq + 1..].trim_start();
        if let Some(q) = value.chars().next().filter(|c| *c == '"' || *c == '\'') {
            let v = &value[1..];
            if v.split(q).next() == Some(uri)
                && !name.is_empty()
                && !name.contains(char::is_whitespace)
            {
                return Some(name);
            }
        }
        rest = decl;
    }
    None
}

/// Start of `<prefix:Name` at or after `from`, as a whole tag name (not a prefix
/// of a longer one such as `<Container:Items`).
fn find_tag(s: &str, open: &str, from: usize) -> Option<usize> {
    let mut at = from;
    loop {
        let p = at + s.get(at..)?.find(open)?;
        match s[p + open.len()..].chars().next() {
            Some(c) if c.is_whitespace() || c == '/' || c == '>' => return Some(p),
            None => return None,
            _ => at = p + open.len(),
        }
    }
}

/// `prefix:name="value"` (or single-quoted) inside a start tag.
fn attribute(tag: &str, prefix: &str, name: &str) -> Option<String> {
    let key = format!("{prefix}:{name}=");
    let mut at = 0;
    while let Some(p) = tag[at..].find(&key) {
        let s = at + p;
        let before = tag[..s].chars().next_back();
        let value = &tag[s + key.len()..];
        if before.is_some_and(char::is_whitespace) {
            let q = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            return value[1..].split(q).next().map(str::to_string);
        }
        at = s + key.len();
    }
    None
}

/// `<prefix:name>value</prefix:name>` inside an item's body.
fn element(body: &str, prefix: &str, name: &str) -> Option<String> {
    let open = format!("<{prefix}:{name}>");
    let close = format!("</{prefix}:{name}>");
    let s = body.find(&open)? + open.len();
    let e = body[s..].find(&close)? + s;
    Some(body[s..e].trim().to_string())
}

/// 3296847 -> "3,296,847".
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn primary_end_skips_thumbnails_stuffing_restarts_and_fill() {
        let j = jpeg(None);
        assert_eq!(jpeg_primary_end(&j), Some(j.len()));
        let mut two = j.clone();
        two.extend(jpeg(None));
        assert_eq!(
            jpeg_primary_end(&two),
            Some(j.len()),
            "the first EOI, not the last"
        );
        assert_eq!(jpeg_primary_end(&j[..j.len() - 1]), None, "no EOI");
        assert_eq!(jpeg_primary_end(b"\xFF\xD8"), None);
        assert_eq!(jpeg_primary_end(b"not a jpeg"), None);
    }

    #[test]
    fn intact_motion_photo_agrees() {
        let (f, primary, gain, clip) = motion_photo();
        let l = analyze(&f).unwrap().unwrap();
        assert!(l.agrees(), "{:?}", l.disagreements);
        assert_eq!(l.primary_end, primary);
        let at: Vec<_> = l
            .placements
            .iter()
            .map(|p| (p.semantic.as_str(), p.offset, p.magic))
            .collect();
        assert_eq!(
            at,
            vec![
                ("GainMap", primary, Magic::Matches),
                ("MotionPhoto", primary + gain, Magic::Matches)
            ]
        );
        assert_eq!(l.declared_end, primary + gain + clip);
        assert_eq!(l.declared_end, l.file_len);
    }

    #[test]
    fn clip_removed_is_declared_but_not_in_the_file() {
        let (f, primary, gain, clip) = motion_photo();
        let l = analyze(&f[..(primary + gain) as usize]).unwrap().unwrap();
        assert_eq!(l.placements[0].magic, Magic::Matches);
        assert_eq!(l.placements[1].present_bytes, 0);
        assert_eq!(
            l.disagreements,
            vec![format!(
                "item 2 (MotionPhoto, {clip} bytes) is declared at byte {}, but the file ends at byte {}",
                primary + gain,
                primary + gain
            )]
        );
    }

    #[test]
    fn gain_map_and_clip_removed() {
        let (f, primary, _, _) = motion_photo();
        let l = analyze(&f[..primary as usize]).unwrap().unwrap();
        assert_eq!(l.disagreements.len(), 2, "{:?}", l.disagreements);
        assert!(l.disagreements[0].starts_with("item 1 (GainMap"));
        assert!(l.disagreements[1].starts_with("item 2 (MotionPhoto"));
    }

    #[test]
    fn clip_cut_short_reports_how_much_is_left() {
        let (f, primary, gain, _) = motion_photo();
        let cut = (primary + gain + 100) as usize;
        let l = analyze(&f[..cut]).unwrap().unwrap();
        assert_eq!(l.placements[1].present_bytes, 100);
        assert_eq!(
            l.disagreements,
            vec!["item 2 (MotionPhoto) is cut short: 100 of 300 bytes present".to_string()]
        );
    }

    #[test]
    fn bytes_after_the_clip_are_reported() {
        let (mut f, ..) = motion_photo();
        f.extend_from_slice(b"appended");
        let l = analyze(&f).unwrap().unwrap();
        assert_eq!(l.disagreements.len(), 1);
        assert!(l.disagreements[0].contains("where the motion clip should end the file"));
    }

    #[test]
    fn a_wrong_declared_length_shows_as_wrong_bytes_at_the_next_item() {
        // The directory says the gain map is 4 bytes longer than it is: the clip's
        // expected start lands inside it, and the file is 4 bytes short.
        let gain = jpeg(None);
        let clip = mp4(300);
        let dir = xmp(&[
            ("Primary", "image/jpeg", Some(0), None),
            ("GainMap", "image/jpeg", Some(gain.len() as u64 + 4), None),
            ("MotionPhoto", "video/mp4", Some(clip.len() as u64), None),
        ]);
        let mut f = jpeg(Some(&dir));
        f.extend(&gain);
        f.extend(&clip);
        let l = analyze(&f).unwrap().unwrap();
        assert_eq!(l.placements[1].magic, Magic::Differs);
        assert!(
            l.disagreements
                .iter()
                .any(|d| d.contains("are not video/mp4"))
        );
        assert!(
            l.disagreements
                .iter()
                .any(|d| d.contains("cut short: 296 of 300"))
        );
    }

    #[test]
    fn primary_padding_is_honoured() {
        let clip = mp4(64);
        let dir = xmp(&[
            ("Primary", "image/jpeg", Some(0), Some(5)),
            ("MotionPhoto", "video/mp4", Some(64), None),
        ]);
        let mut f = jpeg(Some(&dir));
        let primary = f.len() as u64;
        f.extend_from_slice(&[0; 5]);
        f.extend(&clip);
        let l = analyze(&f).unwrap().unwrap();
        assert!(l.agrees(), "{:?}", l.disagreements);
        assert_eq!(l.placements[0].offset, primary + 5);
    }

    #[test]
    fn padding_on_a_later_item_is_a_disagreement() {
        let dir = xmp(&[
            ("Primary", "image/jpeg", Some(0), None),
            ("MotionPhoto", "video/mp4", Some(64), Some(3)),
        ]);
        let mut f = jpeg(Some(&dir));
        f.extend(mp4(64));
        let l = analyze(&f).unwrap().unwrap();
        assert_eq!(
            l.disagreements,
            vec!["item 1 declares padding, which only the first item may".to_string()]
        );
    }

    #[test]
    fn length_zero_shares_the_previous_resource() {
        let dir = xmp(&[
            ("Primary", "image/jpeg", Some(0), None),
            ("MotionPhoto", "video/mp4", Some(64), None),
            ("Depth", "video/mp4", Some(0), None),
        ]);
        let mut f = jpeg(Some(&dir));
        let primary = f.len() as u64;
        f.extend(mp4(64));
        let l = analyze(&f).unwrap().unwrap();
        assert!(l.agrees(), "{:?}", l.disagreements);
        assert_eq!(l.placements[1].offset, primary);
        assert_eq!(l.placements[1].magic, Magic::NotChecked);
    }

    #[test]
    fn missing_length_stops_the_layout() {
        let dir = xmp(&[
            ("Primary", "image/jpeg", Some(0), None),
            ("MotionPhoto", "video/mp4", None, None),
        ]);
        let l = analyze(&jpeg(Some(&dir))).unwrap().unwrap();
        assert!(l.placements.is_empty());
        assert!(l.disagreements[0].contains("has no Item:Length"));
    }

    #[test]
    fn trailing_bytes_are_reported_alongside_other_disagreements() {
        let dir = xmp(&[
            ("Primary", "image/jpeg", Some(0), None),
            ("MotionPhoto", "video/mp4", Some(64), Some(3)),
        ]);
        let mut f = jpeg(Some(&dir));
        f.extend(mp4(64));
        f.extend_from_slice(b"tail");
        let l = analyze(&f).unwrap().unwrap();
        assert_eq!(l.disagreements.len(), 2, "{:?}", l.disagreements);
        assert!(l.disagreements[1].contains("where the motion clip should end the file"));
    }

    #[test]
    fn an_empty_directory_passed_to_resolve_does_not_panic() {
        let l = resolve(Directory { items: Vec::new() }, 10, &[0; 20]);
        assert_eq!(
            l.disagreements,
            vec!["the container directory has no items".to_string()]
        );
    }

    #[test]
    fn no_directory_is_none() {
        assert_eq!(analyze(&jpeg(None)).unwrap(), None);
        let other = r#"<x:xmpmeta xmlns:dc="http://purl.org/dc/elements/1.1/"/>"#;
        assert_eq!(analyze(&jpeg(Some(other))).unwrap(), None);
        assert!(analyze(b"not a jpeg").is_err());
    }

    #[test]
    fn other_prefixes_and_element_fields_are_read() {
        let x = format!(
            r#"<rdf:Description xmlns:GC="{CONTAINER_NS}" xmlns:GI='{ITEM_NS}'><GC:Directory><rdf:Seq>
               <rdf:li><GC:Item rdf:parseType="Resource"><GI:Semantic>Primary</GI:Semantic><GI:Mime>image/jpeg</GI:Mime></GC:Item></rdf:li>
               <rdf:li><GC:Item rdf:parseType="Resource"><GI:Semantic>MotionPhoto</GI:Semantic><GI:Mime>video/mp4</GI:Mime><GI:Length> 64 </GI:Length></GC:Item></rdf:li>
               </rdf:Seq></GC:Directory></rdf:Description>"#
        );
        let d = parse_directory(&x).unwrap().unwrap();
        assert_eq!(d.items.len(), 2);
        assert_eq!(d.items[1].semantic, "MotionPhoto");
        assert_eq!(d.items[1].length, Some(64));
    }

    #[test]
    fn unreadable_directories_are_errors() {
        let bad_len = xmp(&[("Primary", "image/jpeg", Some(0), None)])
            .replace(r#"Item:Length="0""#, r#"Item:Length="-1""#);
        assert!(parse_directory(&bad_len).is_err());
        let unclosed =
            xmp(&[("Primary", "image/jpeg", None, None)]).replace("</Container:Directory>", "");
        assert!(parse_directory(&unclosed).is_err());
        let no_mime =
            xmp(&[("Primary", "image/jpeg", None, None)]).replace(r#"Item:Mime="image/jpeg" "#, "");
        assert!(parse_directory(&no_mime).is_err());
        let empty = format!(
            r#"<d xmlns:Container="{CONTAINER_NS}" xmlns:Item="{ITEM_NS}"><Container:Directory></Container:Directory></d>"#
        );
        assert!(parse_directory(&empty).is_err());
    }

    #[test]
    fn hostile_bytes_never_panic() {
        let (f, ..) = motion_photo();
        let mut seed: u64 = 0x5EED_0000_0000_0001;
        for _ in 0..5_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let mut g = f.clone();
            let n = 1 + (seed % 8) as usize;
            for k in 0..n {
                let pos = ((seed >> (k * 7)) as usize) % g.len();
                g[pos] = (seed >> (k * 5)) as u8;
            }
            let cut = (seed >> 40) as usize % (g.len() + 1);
            let _ = analyze(&g[..cut]);
            let _ = parse_directory(&String::from_utf8_lossy(&g[..cut]));
        }
    }
}
