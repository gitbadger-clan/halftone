//! JPEG structure: the metadata segments before the first scan, and the end of
//! the first complete image. Reading only; no interpretation.

use crate::MAX_XMP_BYTES;

/// What a JPEG's metadata segments hold, as read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JpegMetadata {
    /// XMP packets in file order: each standard packet (APP1), then the reassembled
    /// ExtendedXMP when it was present and complete.
    pub xmp_packets: Vec<Vec<u8>>,
    /// An ExtendedXMP chunk was present.
    pub xmp_extended: bool,
    /// ExtendedXMP was present but could not be fully reassembled.
    pub xmp_extended_incomplete: bool,
    /// An APP11 segment starts with a JUMBF box (`JP`): a C2PA manifest container.
    pub c2pa_jumbf: bool,
    /// The body of each APP13 Photoshop image-resource block, after its
    /// `Photoshop 3.0` signature, in file order.
    pub photoshop_irb: Vec<Vec<u8>>,
}

/// ExtendedXMP under reassembly: one GUID, declared full length, (offset, data) chunks.
struct ExtendedXmp {
    guid: Vec<u8>,
    full: usize,
    chunks: Vec<(usize, Vec<u8>)>,
}

/// Walk JPEG segments up to SOS and collect what the metadata segments hold: the
/// standard XMP packets, the reassembled ExtendedXMP, whether a C2PA JUMBF segment
/// is present, and the Photoshop IRB bodies. Reads only; interpreting any of it is
/// the caller's job. Bounds-checked; never panics.
pub fn metadata(b: &[u8]) -> Result<JpegMetadata, String> {
    const XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
    const EXT: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
    const IRB: &[u8] = b"Photoshop 3.0\0";
    if !b.starts_with(&[0xFF, 0xD8]) {
        return Err("not a JPEG".into());
    }
    let mut s = JpegMetadata::default();
    let mut packets = Vec::new();
    let mut ext: Option<ExtendedXmp> = None;
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
        let seg = &b[i + 2..i + len];
        match marker {
            0xE1 => {
                if let Some(rest) = seg.strip_prefix(XMP) {
                    packets.push(rest.to_vec());
                } else if let Some(rest) = seg.strip_prefix(EXT) {
                    s.xmp_extended = true;
                    // GUID (32 ASCII bytes), full length u32, offset u32, data.
                    if rest.len() >= 40 {
                        let guid = rest[..32].to_vec();
                        let full =
                            u32::from_be_bytes([rest[32], rest[33], rest[34], rest[35]]) as usize;
                        let off =
                            u32::from_be_bytes([rest[36], rest[37], rest[38], rest[39]]) as usize;
                        let data = rest[40..].to_vec();
                        let e = ext.get_or_insert_with(|| ExtendedXmp {
                            guid: guid.clone(),
                            full,
                            chunks: Vec::new(),
                        });
                        if e.guid == guid && e.full == full && full <= MAX_XMP_BYTES {
                            e.chunks.push((off, data));
                        } else {
                            s.xmp_extended_incomplete = true;
                        }
                    } else {
                        s.xmp_extended_incomplete = true;
                    }
                }
            }
            0xEB => s.c2pa_jumbf |= seg.starts_with(b"JP"),
            0xED => {
                if let Some(rest) = seg.strip_prefix(IRB) {
                    s.photoshop_irb.push(rest.to_vec());
                }
            }
            _ => {}
        }
        i += len;
    }
    if let Some(ExtendedXmp {
        full, mut chunks, ..
    }) = ext
    {
        chunks.sort_by_key(|c| c.0);
        // Check that the chunks tile the declared length before allocating: the
        // length comes from the file, so it is never an allocation size on its own
        // (a 1 KB file once made this allocate 4 GB; found by the fuzz target).
        let mut covered = 0usize;
        let mut ok = full <= MAX_XMP_BYTES;
        for (off, data) in &chunks {
            if !ok || *off != covered || off + data.len() > full {
                ok = false;
                break;
            }
            covered += data.len();
        }
        if ok && covered == full {
            let mut buf = Vec::with_capacity(full);
            for (_, data) in chunks {
                buf.extend_from_slice(&data);
            }
            packets.push(buf);
        } else {
            s.xmp_extended_incomplete = true;
        }
    }
    s.xmp_packets = packets;
    Ok(s)
}

/// End of the first complete JPEG in `b` (just past its EOI), walking marker
/// segments and skipping entropy-coded data, so a later embedded JPEG (a gain map,
/// a thumbnail) is never mistaken for the end of the primary.
pub fn primary_end(b: &[u8]) -> Option<usize> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut s = vec![0xFF, marker];
        s.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        s.extend_from_slice(payload);
        s
    }

    fn ext_chunk(guid: &[u8; 32], full: u32, off: u32, data: &[u8]) -> Vec<u8> {
        let mut p = b"http://ns.adobe.com/xmp/extension/\0".to_vec();
        p.extend_from_slice(guid);
        p.extend_from_slice(&full.to_be_bytes());
        p.extend_from_slice(&off.to_be_bytes());
        p.extend_from_slice(data);
        segment(0xE1, &p)
    }

    #[test]
    fn reads_every_kind_of_metadata_segment_in_order() {
        let guid = [b'A'; 32];
        let mut f = vec![0xFF, 0xD8];
        f.extend(segment(0xE1, b"http://ns.adobe.com/xap/1.0/\0<x:one/>"));
        f.extend(ext_chunk(&guid, 6, 3, b"def"));
        f.extend(ext_chunk(&guid, 6, 0, b"abc"));
        f.extend(segment(0xEB, b"JP\0\0jumb"));
        f.extend(segment(0xED, b"Photoshop 3.0\0first"));
        f.extend(segment(0xED, b"Photoshop 3.0\0second"));
        f.extend(segment(0xED, b"not an IRB"));
        f.extend(segment(0xDA, &[1, 1, 0, 0, 63, 0]));
        f.extend_from_slice(&[0x12, 0xFF, 0xD9]);
        let m = metadata(&f).unwrap();
        assert_eq!(
            m.xmp_packets,
            vec![b"<x:one/>".to_vec(), b"abcdef".to_vec()]
        );
        assert!(m.xmp_extended && !m.xmp_extended_incomplete);
        assert!(m.c2pa_jumbf);
        assert_eq!(m.photoshop_irb, vec![b"first".to_vec(), b"second".to_vec()]);
    }

    #[test]
    fn an_incomplete_extended_packet_is_flagged_and_left_out() {
        let mut f = vec![0xFF, 0xD8];
        f.extend(ext_chunk(&[b'B'; 32], 6, 0, b"abc"));
        f.extend_from_slice(&[0xFF, 0xD9]);
        let m = metadata(&f).unwrap();
        assert!(m.xmp_extended && m.xmp_extended_incomplete);
        assert!(m.xmp_packets.is_empty());
    }

    #[test]
    fn errors_keep_their_wording() {
        assert_eq!(metadata(b"PNG").unwrap_err(), "not a JPEG");
        assert_eq!(
            metadata(&[0xFF, 0xD8, 0x00, 0x00]).unwrap_err(),
            "marker expected at 2"
        );
        // A single trailing byte is not enough for a marker: ignored, as it always was.
        assert!(metadata(&[0xFF, 0xD8, 0x00]).is_ok());
        assert_eq!(
            metadata(&[0xFF, 0xD8, 0xFF, 0xE1, 0x00]).unwrap_err(),
            "truncated segment header"
        );
        assert_eq!(
            metadata(&[0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x01]).unwrap_err(),
            "bad segment length"
        );
    }
}
