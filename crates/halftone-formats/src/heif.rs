//! HEIF (ISO/IEC 23008-12) and AVIF metadata items: the brands in `ftyp`, the
//! item inventory in `meta` (`iinf`/`infe`), and the bytes of each XMP item, located
//! through `iloc` in the file or in `idat`. Also whether a top-level C2PA `uuid`
//! box is present. No pixel decoding: HEVC and AV1 payloads are never touched.
//!
//! Reading only, bounds-checked, never panics. EXIF in HEIF is read by
//! `kamadak-exif` directly and is not duplicated here.

use crate::MAX_XMP_BYTES;
use serde::Serialize;

/// Extended type of the top-level `uuid` box that holds a C2PA manifest store in
/// ISO BMFF files (C2PA 2.x, BMFF embedding).
pub const C2PA_UUID: [u8; 16] = [
    0xD8, 0xFE, 0xC3, 0xD6, 0x1B, 0x0E, 0x48, 0x3C, 0x92, 0x97, 0x58, 0x28, 0x87, 0x7E, 0xC4, 0x81,
];
/// Boxes walked at most per container, so a file of empty boxes cannot spin.
const MAX_BOXES: usize = 4096;
/// Items read at most from `iinf` / `iloc`.
const MAX_ITEMS: usize = 65_536;
/// Extents read at most per item.
const MAX_EXTENTS: usize = 256;

/// One item from `iinf`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// `item_ID`.
    pub id: u32,
    /// `item_type` (e.g. `hvc1`, `grid`, `Exif`, `mime`, `av01`); empty for `infe`
    /// versions 0 and 1, which have none.
    pub item_type: String,
    /// `content_type` of a `mime` item (`application/rdf+xml` for XMP).
    pub content_type: Option<String>,
    /// `item_name`, often empty.
    pub name: String,
}

/// What a HEIF file's metadata boxes hold, as read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HeifMetadata {
    /// `ftyp` major brand (e.g. `heic`, `mif1`, `avif`).
    pub major_brand: String,
    /// `ftyp` compatible brands, in order.
    pub compatible_brands: Vec<String>,
    /// A top-level `meta` box was found.
    pub has_meta: bool,
    /// `hdlr` handler type of `meta` (`pict` for still images).
    pub handler: Option<String>,
    /// `pitm` primary item ID.
    pub primary_item: Option<u32>,
    /// Every item `iinf` lists, in order.
    pub items: Vec<Item>,
    /// Bytes of each XMP item (`mime` with `application/rdf+xml`), in `iinf` order.
    pub xmp_packets: Vec<Vec<u8>>,
    /// XMP items listed in `iinf` whose bytes could not be read, with why.
    pub xmp_unreadable: Vec<String>,
    /// A top-level `uuid` box with [`C2PA_UUID`] is present.
    pub c2pa_uuid_box: bool,
}

/// One box: its type, the payload (after the header), and the payload's position
/// in the buffer the box was read from.
struct BoxRef<'a> {
    kind: [u8; 4],
    payload: &'a [u8],
    payload_at: usize,
}

/// The boxes of `b`, in order. `base` is `b`'s position in the file, so payload
/// positions come out absolute. A size of 0 runs to the end of `b`.
fn boxes(b: &[u8], base: usize) -> Result<Vec<BoxRef<'_>>, String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if out.len() >= MAX_BOXES {
            return Err(format!("more than {MAX_BOXES} boxes in one container"));
        }
        let hdr = b
            .get(i..i + 8)
            .ok_or_else(|| format!("truncated box header at {}", base + i))?;
        let size32 = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
        let kind = [hdr[4], hdr[5], hdr[6], hdr[7]];
        let (head, size) = match size32 {
            0 => (8usize, (b.len() - i) as u64),
            1 => {
                let l = b
                    .get(i + 8..i + 16)
                    .ok_or_else(|| format!("truncated largesize at {}", base + i))?;
                (16, u64::from_be_bytes(l.try_into().unwrap_or([0; 8])))
            }
            n => (8, u64::from(n)),
        };
        let size = usize::try_from(size).map_err(|_| "box size overflows usize")?;
        if size < head || i.checked_add(size).is_none_or(|e| e > b.len()) {
            return Err(format!(
                "box '{}' at {} has size {size}, outside its container",
                fourcc(&kind),
                base + i
            ));
        }
        out.push(BoxRef {
            kind,
            payload: &b[i + head..i + size],
            payload_at: base + i + head,
        });
        i += size;
    }
    Ok(out)
}

fn fourcc(k: &[u8]) -> String {
    k.iter()
        .map(|&c| {
            if c.is_ascii_graphic() || c == b' ' {
                c as char
            } else {
                '?'
            }
        })
        .collect()
}

/// Big-endian cursor over a box payload.
struct Rd<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Rd<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self
            .b
            .get(self.at..self.at.checked_add(n).ok_or("length overflow")?)
            .ok_or("box payload truncated")?;
        self.at += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    /// An unsigned integer of 0, 4 or 8 bytes (the `iloc` field sizes).
    fn uint(&mut self, size: u8) -> Result<u64, String> {
        match size {
            0 => Ok(0),
            4 => self.u32().map(u64::from),
            8 => {
                let s = self.take(8)?;
                Ok(u64::from_be_bytes(s.try_into().unwrap_or([0; 8])))
            }
            n => Err(format!("iloc field size {n} (only 0, 4 and 8 are defined)")),
        }
    }
    /// A NUL-terminated UTF-8 string; a missing terminator ends at the payload's end.
    fn cstr(&mut self) -> String {
        let rest = self.b.get(self.at..).unwrap_or(&[]);
        let n = rest.iter().position(|&c| c == 0).unwrap_or(rest.len());
        self.at += (n + 1).min(rest.len());
        String::from_utf8_lossy(&rest[..n]).into_owned()
    }
    /// Full-box header: (version, flags).
    fn full(&mut self) -> Result<(u8, u32), String> {
        let v = self.u32()?;
        Ok(((v >> 24) as u8, v & 0x00FF_FFFF))
    }
}

/// One extent from `iloc`, resolved against its construction method.
struct Extent {
    method: u8,
    offset: u64,
    length: u64,
}

/// Is this an ISO BMFF file whose brands say HEIF or AVIF (still image)? Cheap:
/// looks at `ftyp` only.
pub fn is_heif(file: &[u8]) -> bool {
    brands(file).is_some_and(|(major, compat)| {
        std::iter::once(major.as_str())
            .chain(compat.iter().map(String::as_str))
            .any(|b| matches!(b, "heic" | "heix" | "mif1" | "mif2" | "avif" | "MiHE"))
    })
}

/// `ftyp`'s major and compatible brands, when the file starts with one.
fn brands(file: &[u8]) -> Option<(String, Vec<String>)> {
    if file.get(4..8)? != b"ftyp" {
        return None;
    }
    let size = u32::from_be_bytes(file.get(..4)?.try_into().ok()?) as usize;
    let p = file.get(8..size)?;
    if p.len() < 8 {
        return None;
    }
    Some((
        fourcc(&p[..4]),
        p[8..].chunks_exact(4).map(fourcc).collect(),
    ))
}

/// Read the metadata items of a HEIF/AVIF file. `Err`: not ISO BMFF starting with
/// `ftyp`, or a box structure that cannot be walked. A file without `meta` is `Ok`
/// with `has_meta: false`. An XMP item whose bytes cannot be located is recorded
/// in `xmp_unreadable` instead of failing the whole read.
pub fn metadata(file: &[u8]) -> Result<HeifMetadata, String> {
    let top = boxes(file, 0)?;
    let ftyp = top
        .first()
        .filter(|b| &b.kind == b"ftyp" && b.payload.len() >= 8)
        .ok_or("not an ISO BMFF file (no ftyp box first)")?;
    let mut m = HeifMetadata {
        major_brand: fourcc(&ftyp.payload[..4]),
        compatible_brands: ftyp.payload[8..].chunks_exact(4).map(fourcc).collect(),
        ..HeifMetadata::default()
    };
    m.c2pa_uuid_box = top
        .iter()
        .any(|b| &b.kind == b"uuid" && b.payload.starts_with(&C2PA_UUID));

    let Some(meta) = top.iter().find(|b| &b.kind == b"meta") else {
        return Ok(m);
    };
    m.has_meta = true;
    // `meta` is a full box: version/flags, then children.
    let children_at = 4;
    let kids = boxes(
        meta.payload
            .get(children_at..)
            .ok_or("meta box truncated")?,
        meta.payload_at + children_at,
    )?;

    let mut locations: Vec<(u32, Vec<Extent>)> = Vec::new();
    let mut idat: Option<(&[u8], usize)> = None;
    for k in &kids {
        match &k.kind {
            b"hdlr" => {
                let mut r = Rd::new(k.payload);
                r.full()?;
                r.u32()?; // pre_defined
                m.handler = Some(fourcc(r.take(4)?));
            }
            b"pitm" => {
                let mut r = Rd::new(k.payload);
                let (v, _) = r.full()?;
                m.primary_item = Some(if v == 0 {
                    u32::from(r.u16()?)
                } else {
                    r.u32()?
                });
            }
            b"iinf" => m.items = parse_iinf(k)?,
            b"iloc" => locations = parse_iloc(k.payload)?,
            b"idat" => idat = Some((k.payload, k.payload_at)),
            _ => {}
        }
    }

    for item in &m.items {
        let is_xmp = item.item_type == "mime"
            && item
                .content_type
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case("application/rdf+xml"));
        if !is_xmp {
            continue;
        }
        let Some((_, extents)) = locations.iter().find(|(id, _)| *id == item.id) else {
            m.xmp_unreadable
                .push(format!("XMP item {} has no iloc entry", item.id));
            continue;
        };
        match item_bytes(extents, file, idat.map(|i| i.0), MAX_XMP_BYTES) {
            Ok(b) => m.xmp_packets.push(b),
            Err(e) => m.xmp_unreadable.push(format!("XMP item {}: {e}", item.id)),
        }
    }
    Ok(m)
}

fn parse_iinf(k: &BoxRef<'_>) -> Result<Vec<Item>, String> {
    let mut r = Rd::new(k.payload);
    let (v, _) = r.full()?;
    let count = if v == 0 {
        u32::from(r.u16()?)
    } else {
        r.u32()?
    } as usize;
    if count > MAX_ITEMS {
        return Err(format!(
            "iinf declares {count} items (at most {MAX_ITEMS} read)"
        ));
    }
    let at = r.at;
    let mut items = Vec::with_capacity(count.min(1024));
    for infe in boxes(&k.payload[at..], k.payload_at + at)? {
        if &infe.kind != b"infe" {
            continue;
        }
        let mut r = Rd::new(infe.payload);
        let (v, _) = r.full()?;
        let item = if v >= 2 {
            let id = if v == 2 {
                u32::from(r.u16()?)
            } else {
                r.u32()?
            };
            r.u16()?; // item_protection_index
            let item_type = fourcc(r.take(4)?);
            let name = r.cstr();
            let content_type = (item_type == "mime").then(|| r.cstr());
            Item {
                id,
                item_type,
                content_type,
                name,
            }
        } else {
            // Versions 0/1: item_ID, protection index, name, content_type, encoding.
            let id = u32::from(r.u16()?);
            r.u16()?;
            let name = r.cstr();
            let content_type = Some(r.cstr()).filter(|s| !s.is_empty());
            Item {
                id,
                item_type: String::new(),
                content_type,
                name,
            }
        };
        items.push(item);
        if items.len() >= MAX_ITEMS {
            break;
        }
    }
    Ok(items)
}

fn parse_iloc(p: &[u8]) -> Result<Vec<(u32, Vec<Extent>)>, String> {
    let mut r = Rd::new(p);
    let (v, _) = r.full()?;
    if v > 2 {
        return Err(format!("iloc version {v} (0 to 2 are defined)"));
    }
    let a = r.u8()?;
    let b = r.u8()?;
    let (offset_size, length_size) = (a >> 4, a & 0xF);
    let (base_offset_size, index_size) = (b >> 4, if v >= 1 { b & 0xF } else { 0 });
    let count = if v < 2 { u32::from(r.u16()?) } else { r.u32()? } as usize;
    if count > MAX_ITEMS {
        return Err(format!(
            "iloc declares {count} items (at most {MAX_ITEMS} read)"
        ));
    }
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let id = if v < 2 { u32::from(r.u16()?) } else { r.u32()? };
        let method = if v >= 1 { (r.u16()? & 0xF) as u8 } else { 0 };
        r.u16()?; // data_reference_index
        let base = r.uint(base_offset_size)?;
        let n = r.u16()? as usize;
        if n > MAX_EXTENTS {
            return Err(format!(
                "iloc item {id} has {n} extents (at most {MAX_EXTENTS} read)"
            ));
        }
        let mut extents = Vec::with_capacity(n);
        for _ in 0..n {
            if index_size > 0 {
                r.uint(index_size)?;
            }
            let off = r.uint(offset_size)?;
            let len = r.uint(length_size)?;
            extents.push(Extent {
                method,
                offset: base.saturating_add(off),
                length: len,
            });
        }
        out.push((id, extents));
    }
    Ok(out)
}

/// Concatenate an item's extents. Method 0 reads from the file, method 1 from
/// `idat`'s payload; method 2 (item offset) is not supported. A zero length means
/// "to the end of the source". Checks every bound before allocating.
fn item_bytes(
    extents: &[Extent],
    file: &[u8],
    idat: Option<&[u8]>,
    cap: usize,
) -> Result<Vec<u8>, String> {
    let mut spans: Vec<&[u8]> = Vec::with_capacity(extents.len());
    let mut total = 0usize;
    for e in extents {
        let src = match e.method {
            0 => file,
            1 => idat.ok_or("construction method 1 but no idat box")?,
            m => return Err(format!("construction method {m} is not supported")),
        };
        let start = usize::try_from(e.offset).map_err(|_| "extent offset overflows")?;
        let end = if e.length == 0 {
            src.len()
        } else {
            start
                .checked_add(usize::try_from(e.length).map_err(|_| "extent length overflows")?)
                .ok_or("extent end overflows")?
        };
        let s = src.get(start..end).ok_or_else(|| {
            format!(
                "extent {start}..{end} lies outside its source ({} bytes)",
                src.len()
            )
        })?;
        total = total.checked_add(s.len()).ok_or("item size overflows")?;
        if total > cap {
            return Err(format!("item is larger than {cap} bytes"));
        }
        spans.push(s);
    }
    if spans.is_empty() {
        return Err("item has no extents".into());
    }
    Ok(spans.concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = u32::try_from(payload.len() + 8)
            .unwrap()
            .to_be_bytes()
            .to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(payload);
        v
    }

    fn full(kind: &[u8; 4], version: u8, payload: &[u8]) -> Vec<u8> {
        let mut p = vec![version, 0, 0, 0];
        p.extend_from_slice(payload);
        bx(kind, &p)
    }

    fn infe(id: u16, item_type: &[u8; 4], content_type: Option<&str>) -> Vec<u8> {
        let mut p = id.to_be_bytes().to_vec();
        p.extend_from_slice(&[0, 0]);
        p.extend_from_slice(item_type);
        p.push(0); // empty name
        if let Some(c) = content_type {
            p.extend_from_slice(c.as_bytes());
            p.push(0);
        }
        full(b"infe", 2, &p)
    }

    const XMP: &[u8] = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
        <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
        <rdf:Description xmlns:Iptc4xmpExt=\"http://iptc.org/std/Iptc4xmpExt/2008-02-29/\" \
        Iptc4xmpExt:DigitalSourceType=\"http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia\"/>\
        </rdf:RDF></x:xmpmeta>";

    /// An iPhone-like HEIC: ftyp, meta (hdlr, pitm, iinf with a grid, an Exif and
    /// an XMP item, iloc), mdat with the XMP bytes in it. `in_idat` puts the XMP
    /// in `idat` instead (construction method 1, iloc version 1).
    fn heic(in_idat: bool, c2pa: bool) -> Vec<u8> {
        let ftyp = bx(b"ftyp", b"heic\0\0\0\0mif1MiHEmiafheic");
        let hdlr = full(b"hdlr", 0, b"\0\0\0\0pict\0\0\0\0\0\0\0\0\0\0\0\0\0");
        let pitm = full(b"pitm", 0, &1u16.to_be_bytes());
        let mut iinf_p = 3u16.to_be_bytes().to_vec();
        iinf_p.extend(infe(1, b"grid", None));
        iinf_p.extend(infe(2, b"Exif", None));
        iinf_p.extend(infe(3, b"mime", Some("application/rdf+xml")));
        let iinf = full(b"iinf", 0, &iinf_p);
        let idat = bx(b"idat", XMP);

        // iloc: offset 4, length 4, base 0; one item (3) with one extent. The XMP
        // offset is patched once the layout is known.
        let iloc_len_v0 = 4 + 8 + 2 + 2 + 2 + 2 + 2 + 4 + 4;
        let build = |xmp_at: u32| {
            let version = if in_idat { 1 } else { 0 };
            let mut p = vec![0x44, 0x00];
            p.extend_from_slice(&1u16.to_be_bytes());
            p.extend_from_slice(&3u16.to_be_bytes());
            if in_idat {
                p.extend_from_slice(&1u16.to_be_bytes()); // construction method 1
            }
            p.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
            p.extend_from_slice(&1u16.to_be_bytes()); // extent_count
            p.extend_from_slice(&xmp_at.to_be_bytes());
            p.extend_from_slice(&u32::try_from(XMP.len()).unwrap().to_be_bytes());
            full(b"iloc", version, &p)
        };
        let iloc_len = iloc_len_v0 + if in_idat { 2 } else { 0 };
        let mut meta_kids = [hdlr, pitm, iinf].concat();
        if in_idat {
            meta_kids.extend_from_slice(&idat);
        }
        let meta_len = 8 + 4 + meta_kids.len() + iloc_len;
        let uuid = if c2pa {
            let mut p = C2PA_UUID.to_vec();
            p.extend_from_slice(b"\0\0\0\0manifest");
            bx(b"uuid", &p)
        } else {
            Vec::new()
        };
        let mdat_payload_at = ftyp.len() + meta_len + uuid.len() + 8;
        let iloc = build(if in_idat {
            0
        } else {
            u32::try_from(mdat_payload_at).unwrap()
        });
        assert_eq!(iloc.len(), iloc_len);
        meta_kids.extend(iloc);
        let meta = full(b"meta", 0, &meta_kids);
        assert_eq!(meta.len(), meta_len);
        let mdat = bx(b"mdat", if in_idat { b"hevc" } else { XMP });
        [ftyp, meta, uuid, mdat].concat()
    }

    #[test]
    fn reads_brands_items_and_the_xmp_item_from_mdat() {
        let f = heic(false, false);
        assert!(is_heif(&f));
        let m = metadata(&f).unwrap();
        assert_eq!(m.major_brand, "heic");
        assert_eq!(m.compatible_brands, ["mif1", "MiHE", "miaf", "heic"]);
        assert_eq!(m.handler.as_deref(), Some("pict"));
        assert_eq!(m.primary_item, Some(1));
        let types: Vec<_> = m.items.iter().map(|i| i.item_type.as_str()).collect();
        assert_eq!(types, ["grid", "Exif", "mime"]);
        assert_eq!(m.xmp_packets, [XMP.to_vec()]);
        assert!(m.xmp_unreadable.is_empty());
        assert!(!m.c2pa_uuid_box);
    }

    #[test]
    fn reads_the_xmp_item_from_idat_and_sees_a_c2pa_box() {
        let m = metadata(&heic(true, true)).unwrap();
        assert_eq!(m.xmp_packets, [XMP.to_vec()]);
        assert!(m.c2pa_uuid_box);
    }

    #[test]
    fn an_xmp_item_pointing_outside_the_file_is_unreadable_not_an_error() {
        let mut f = heic(false, false);
        let cut = f.len() - XMP.len() / 2;
        // Shrink mdat's declared size to match, so the box walk itself still works.
        let mdat_at = f.len() - XMP.len() - 8;
        let new_size = u32::try_from(cut - mdat_at).unwrap();
        f[mdat_at..mdat_at + 4].copy_from_slice(&new_size.to_be_bytes());
        f.truncate(cut);
        let m = metadata(&f).unwrap();
        assert!(m.xmp_packets.is_empty());
        assert!(
            m.xmp_unreadable[0].contains("outside its source"),
            "{:?}",
            m.xmp_unreadable
        );
    }

    #[test]
    fn non_heif_inputs() {
        assert!(!is_heif(b"\xFF\xD8\xFF\xE0"));
        assert!(!is_heif(&bx(b"ftyp", b"mp42\0\0\0\0isommp42")));
        assert!(metadata(b"\xFF\xD8\xFF\xE0\0\x10").is_err());
        let no_meta = metadata(&bx(b"ftyp", b"heic\0\0\0\0mif1")).unwrap();
        assert!(!no_meta.has_meta && no_meta.items.is_empty());
    }

    #[test]
    fn never_panics_on_any_prefix_or_single_byte_change() {
        let f = heic(false, true);
        for n in 0..f.len() {
            let _ = metadata(&f[..n]);
            let _ = is_heif(&f[..n]);
        }
        for i in 0..f.len() {
            for v in [0x00, 0x01, 0x7F, 0xFF] {
                let mut g = f.clone();
                g[i] = v;
                let _ = metadata(&g);
            }
        }
    }
}
