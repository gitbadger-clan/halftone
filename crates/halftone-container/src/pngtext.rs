//! PNG text chunks, decoded in one place for every source that reads them.
//!
//! A PNG keeps text in three chunk types: `tEXt` (plain), `zTXt` (zlib-compressed)
//! and `iTXt` (plain or compressed, by a flag). Until DIFFERENTIAL.md D-021 was
//! closed, the compressed forms were not read, so three things went unseen: a
//! generation tool's parameters in a compressed chunk (`png_writer`), an XMP packet
//! stored compressed, and IPTC data, which has no chunk of its own and travels as
//! hex text in a "raw profile" chunk that writers usually compress.
//!
//! [`decode`] turns one chunk's data into its keyword and text. [`raw_profile`]
//! turns a raw-profile text into the bytes it spells out.
//!
//! Inflating is bounded by [`MAX_INFLATED`]: a small chunk can be written to expand
//! without end. A chunk that would grow past the bound, or whose stream is damaged,
//! is not half-read; it comes back as [`Unread`] with the keyword, so a source can
//! say that something is there and why it was not read.
//!
//! Which raw profiles are understood follows what ExifTool reads, measured on files
//! made for the purpose (2026-11-19, ExifTool 13.64): the keywords `Raw profile type
//! iptc` and `Raw profile type 8bim`, holding either a Photoshop image-resource
//! block (`8BIM...`) or bare IIM datasets. A block with the `Photoshop 3.0` header a
//! JPEG segment carries is not read by ExifTool and is not read here. When a file has
//! two such profiles ExifTool reports the first as `IPTC` and the second as `IPTC2`;
//! the sources read the first.

/// Longest text a compressed chunk may inflate to, in bytes.
pub const MAX_INFLATED: usize = 4 * 1024 * 1024;

/// The keyword an XMP packet is stored under.
pub const XMP_KEYWORD: &str = "XML:com.adobe.xmp";

/// The keywords whose text is a raw profile holding IPTC data.
pub const IPTC_PROFILE_KEYWORDS: [&str; 2] = ["Raw profile type iptc", "Raw profile type 8bim"];

/// Why a compressed chunk's text is not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// It would inflate past [`MAX_INFLATED`].
    TooLarge,
    /// The compressed stream is damaged or cut short.
    Damaged,
    /// The chunk names a compression method other than zlib (0).
    UnknownMethod,
}

impl Unread {
    /// The reason as a clause for a rationale.
    pub fn why(self) -> &'static str {
        match self {
            Self::TooLarge => "it would expand past the size this tool reads",
            Self::Damaged => "its compressed data is damaged",
            Self::UnknownMethod => "it uses a compression method other than zlib",
        }
    }
}

/// One text chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChunk {
    /// The keyword. The format defines it as Latin-1; one that is valid UTF-8 is read
    /// as UTF-8, which is what a writer that ignores the rule means and what this
    /// crate did before it had this module.
    pub keyword: String,
    /// Whether the text was stored compressed.
    pub compressed: bool,
    /// The text's bytes (Latin-1 for `tEXt` and `zTXt`, UTF-8 for `iTXt`), or why
    /// they could not be had.
    pub text: Result<Vec<u8>, Unread>,
}

impl TextChunk {
    /// The text as a string, whatever its encoding: UTF-8 when valid, else Latin-1.
    /// `None` when the chunk could not be read.
    pub fn text_lossy(&self) -> Option<String> {
        let bytes = self.text.as_ref().ok()?;
        Some(match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => bytes.iter().map(|&b| b as char).collect(),
        })
    }
}

fn inflate(stream: &[u8]) -> Result<Vec<u8>, Unread> {
    use miniz_oxide::inflate::TINFLStatus;
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(stream, MAX_INFLATED).map_err(|e| {
        if e.status == TINFLStatus::HasMoreOutput {
            Unread::TooLarge
        } else {
            Unread::Damaged
        }
    })
}

/// Decode the data of a `tEXt`, `zTXt` or `iTXt` chunk. `None` for any other chunk
/// type and for data too malformed to have a keyword.
pub fn decode(chunk_type: &[u8], data: &[u8]) -> Option<TextChunk> {
    let nul = data.iter().position(|&c| c == 0)?;
    let keyword = match std::str::from_utf8(&data[..nul]) {
        Ok(s) => s.to_string(),
        Err(_) => data[..nul].iter().map(|&b| b as char).collect(),
    };
    let rest = &data[nul + 1..];
    match chunk_type {
        b"tEXt" => Some(TextChunk {
            keyword,
            compressed: false,
            text: Ok(rest.to_vec()),
        }),
        b"zTXt" => {
            // Compression method, then the stream.
            let (&method, stream) = rest.split_first()?;
            Some(TextChunk {
                keyword,
                compressed: true,
                text: if method == 0 {
                    inflate(stream)
                } else {
                    Err(Unread::UnknownMethod)
                },
            })
        }
        b"iTXt" => {
            // Compression flag, method, language tag NUL, translated keyword NUL, text.
            let (&flag, rest) = rest.split_first()?;
            let (&method, rest) = rest.split_first()?;
            let lang_end = rest.iter().position(|&c| c == 0)?;
            let rest = &rest[lang_end + 1..];
            let translated_end = rest.iter().position(|&c| c == 0)?;
            let body = &rest[translated_end + 1..];
            let text = match (flag, method) {
                (0, _) => Ok(body.to_vec()),
                (_, 0) => inflate(body),
                _ => Err(Unread::UnknownMethod),
            };
            Some(TextChunk {
                keyword,
                compressed: flag != 0,
                text,
            })
        }
        _ => None,
    }
}

/// Every text chunk of a PNG, in file order. Stops quietly at the first chunk that
/// does not fit the file; a source that must report a truncated file walks the
/// chunks itself and calls [`decode`].
pub fn chunks(png: &[u8]) -> Vec<TextChunk> {
    let mut out = Vec::new();
    let mut i = 8;
    while i + 12 <= png.len() {
        let len = u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]) as usize;
        let ty = &png[i + 4..i + 8];
        let start = i + 8;
        let Some(end) = start.checked_add(len).filter(|&e| e + 4 <= png.len()) else {
            break;
        };
        if let Some(chunk) = decode(ty, &png[start..end]) {
            out.push(chunk);
        }
        if ty == b"IEND" {
            break;
        }
        i = end + 4;
    }
    out
}

/// The bytes a raw-profile text spells out. The text is a line with the profile's
/// name, a line with its length in bytes, and the bytes as hexadecimal over any
/// number of lines:
///
/// ```text
///
/// IPTC profile
///       80
/// 3842494d04040000000000431c0250000b...
/// ```
///
/// Every hexadecimal digit after the length line is decoded. The length line has to
/// be a number but is not held against the digits: ExifTool warns when the two
/// disagree and reads what is there, and so does this. `None` when the text is not
/// of that shape or a character is neither a hexadecimal digit nor white space.
pub fn raw_profile(text: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(text).ok()?;
    let mut lines = text.lines().skip_while(|l| l.trim().is_empty());
    let _name = lines.next()?;
    let _stated: u64 = lines.next()?.trim().parse().ok()?;
    let mut out = Vec::new();
    let mut high: Option<u8> = None;
    for c in lines.flat_map(str::bytes) {
        if c.is_ascii_whitespace() {
            continue;
        }
        let digit = (c as char).to_digit(16)? as u8;
        match high.take() {
            None => high = Some(digit),
            Some(h) => out.push(h << 4 | digit),
        }
    }
    // A last digit without its pair is not a byte.
    Some(out)
}

#[cfg(test)]
pub(crate) mod testutil {
    //! Builders for the tests of the sources that read text chunks.

    /// The data of a `zTXt` chunk.
    pub fn ztxt(keyword: &str, text: &[u8]) -> Vec<u8> {
        let mut d = keyword.as_bytes().to_vec();
        d.extend_from_slice(&[0, 0]);
        d.extend(miniz_oxide::deflate::compress_to_vec_zlib(text, 6));
        d
    }

    /// The data of an `iTXt` chunk, compressed or not.
    pub fn itxt(keyword: &str, text: &[u8], compressed: bool) -> Vec<u8> {
        let mut d = keyword.as_bytes().to_vec();
        d.extend_from_slice(&[0, u8::from(compressed), 0, 0, 0]);
        if compressed {
            d.extend(miniz_oxide::deflate::compress_to_vec_zlib(text, 6));
        } else {
            d.extend_from_slice(text);
        }
        d
    }

    /// A raw-profile text for `bytes`, as ExifTool and ImageMagick write it.
    pub fn raw_profile_text(name: &str, bytes: &[u8]) -> Vec<u8> {
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let lines: Vec<&str> = hex
            .as_bytes()
            .chunks(72)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        format!("\n{name}\n{:8}\n{}\n", bytes.len(), lines.join("\n")).into_bytes()
    }

    /// IIM record-2 datasets as bytes.
    pub fn iim(sets: &[(u8, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (dataset, value) in sets {
            out.extend_from_slice(&[0x1C, 2, *dataset]);
            out.extend_from_slice(&(value.len() as u16).to_be_bytes());
            out.extend_from_slice(value);
        }
        out
    }

    /// A PNG holding the given chunks between `IHDR` and `IEND`. No image data and
    /// no checksums: the readers under test need neither.
    pub fn png(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        fn chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut v = (data.len() as u32).to_be_bytes().to_vec();
            v.extend_from_slice(ty);
            v.extend_from_slice(data);
            v.extend_from_slice(&[0; 4]);
            v
        }
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&16u32.to_be_bytes());
        ihdr.extend_from_slice(&16u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        out.extend(chunk(b"IHDR", &ihdr));
        for (ty, data) in chunks {
            out.extend(chunk(ty, data));
        }
        out.extend(chunk(b"IEND", &[]));
        out
    }

    /// The data of a `tEXt` chunk.
    pub fn text(keyword: &str, text: &[u8]) -> Vec<u8> {
        let mut d = keyword.as_bytes().to_vec();
        d.push(0);
        d.extend_from_slice(text);
        d
    }

    /// The data of a `zTXt` chunk whose compressed stream is cut short.
    pub fn ztxt_damaged(keyword: &str, text: &[u8]) -> Vec<u8> {
        let mut d = ztxt(keyword, text);
        d.truncate(d.len() - 6);
        d
    }

    /// A Photoshop image-resource block holding those datasets (resource `0x0404`).
    pub fn irb(iim: &[u8]) -> Vec<u8> {
        let mut out = b"8BIM\x04\x04\0\0".to_vec();
        out.extend_from_slice(&(iim.len() as u32).to_be_bytes());
        out.extend_from_slice(iim);
        if iim.len() % 2 == 1 {
            out.push(0);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    #[test]
    fn the_three_chunk_types_give_keyword_and_text() {
        let plain = decode(b"tEXt", b"Comment\0hello").unwrap();
        assert_eq!(
            (plain.keyword.as_str(), plain.compressed),
            ("Comment", false)
        );
        assert_eq!(plain.text.as_deref(), Ok(&b"hello"[..]));

        let z = decode(b"zTXt", &ztxt("parameters", b"a lighthouse, Steps: 20")).unwrap();
        assert_eq!((z.keyword.as_str(), z.compressed), ("parameters", true));
        assert_eq!(z.text_lossy().as_deref(), Some("a lighthouse, Steps: 20"));

        for compressed in [false, true] {
            let i = decode(
                b"iTXt",
                &itxt("Title", "Schlo\u{df}".as_bytes(), compressed),
            )
            .unwrap();
            assert_eq!(i.compressed, compressed);
            assert_eq!(i.text_lossy().as_deref(), Some("Schlo\u{df}"));
        }
        // An iTXt with a language tag and a translated keyword.
        let mut tagged = b"Title\0\0\0de\0Titel\0".to_vec();
        tagged.extend_from_slice(b"Leuchtturm");
        assert_eq!(
            decode(b"iTXt", &tagged).unwrap().text_lossy().as_deref(),
            Some("Leuchtturm")
        );
        // Latin-1 text that is not UTF-8 is still text.
        assert_eq!(
            decode(b"tEXt", b"Author\0Andr\xe9")
                .unwrap()
                .text_lossy()
                .as_deref(),
            Some("Andr\u{e9}")
        );
        assert_eq!(decode(b"IDAT", b"x\0y"), None);
        assert_eq!(decode(b"tEXt", b"no terminator"), None);
        assert_eq!(decode(b"zTXt", b"keyword\0"), None);
        assert_eq!(decode(b"iTXt", b"keyword\0\x01\x00no-more-nuls"), None);
    }

    #[test]
    fn a_chunk_that_cannot_be_inflated_keeps_its_keyword_and_says_why() {
        // A stream that expands past the bound: a few kilobytes of zeros times a lot.
        let bomb = ztxt("parameters", &vec![0u8; MAX_INFLATED + 1]);
        assert!(
            bomb.len() < 16 * 1024,
            "the bomb is small: {} bytes",
            bomb.len()
        );
        let c = decode(b"zTXt", &bomb).unwrap();
        assert_eq!(c.keyword, "parameters");
        assert_eq!(c.text, Err(Unread::TooLarge));
        assert_eq!(c.text_lossy(), None);
        // Exactly at the bound is read.
        let edge = decode(b"zTXt", &ztxt("k", &vec![b'a'; MAX_INFLATED])).unwrap();
        assert_eq!(edge.text.map(|t| t.len()), Ok(MAX_INFLATED));

        let mut cut = ztxt(
            "prompt",
            b"a long enough text to have a body, a long enough text",
        );
        cut.truncate(cut.len() - 6);
        assert_eq!(decode(b"zTXt", &cut).unwrap().text, Err(Unread::Damaged));
        assert_eq!(
            decode(b"zTXt", b"prompt\0\0not zlib at all").unwrap().text,
            Err(Unread::Damaged)
        );
        assert_eq!(
            decode(b"zTXt", b"prompt\0\x07abc").unwrap().text,
            Err(Unread::UnknownMethod)
        );
        assert_eq!(
            decode(b"iTXt", b"prompt\0\x01\x07\0\0abc").unwrap().text,
            Err(Unread::UnknownMethod)
        );
        for u in [Unread::TooLarge, Unread::Damaged, Unread::UnknownMethod] {
            assert!(!u.why().is_empty());
        }
    }

    #[test]
    fn no_cut_of_a_chunk_or_a_file_panics() {
        let body = b"a long enough text to have a body, a long enough text";
        let cases: [(&[u8; 4], Vec<u8>); 4] = [
            (b"tEXt", text("Comment", body)),
            (b"zTXt", ztxt("Comment", body)),
            (b"iTXt", itxt("Comment", body, false)),
            (b"iTXt", itxt("Comment", body, true)),
        ];
        for (ty, data) in &cases {
            for n in 0..data.len() {
                // Whatever is cut, a keyword that is there stays listed.
                if let Some(c) = decode(*ty, &data[..n]) {
                    assert_eq!(c.keyword, "Comment");
                }
            }
        }
        let file = png(&cases);
        for n in 0..file.len() {
            let _ = chunks(&file[..n]);
        }
        assert_eq!(chunks(&file).len(), 4);
        // A keyword outside ASCII: UTF-8 when it is, Latin-1 otherwise.
        assert_eq!(
            decode(b"tEXt", "Café\0x".as_bytes()).unwrap().keyword,
            "Café"
        );
        assert_eq!(decode(b"tEXt", b"Caf\xE9\0x").unwrap().keyword, "Café");
    }

    #[test]
    fn a_raw_profile_text_gives_back_its_bytes() {
        let bytes = irb(&iim(&[(110, b"Example Agency"), (80, b"Ada Example")]));
        for name in ["IPTC profile", "iptc", "8bim"] {
            assert_eq!(
                raw_profile(&raw_profile_text(name, &bytes)).as_deref(),
                Some(&bytes[..])
            );
        }
        // What ExifTool 13.64 wrote for one dataset, byte for byte.
        let exiftool = b"\nIPTC profile\n      38\n3842494d040400000000001a1c026e000e4578616d706c65204167656e63791c02000002\n0004\n";
        let got = raw_profile(exiftool).unwrap();
        assert_eq!(got.len(), 38);
        assert!(got.starts_with(b"8BIM\x04\x04"));
        // Upper-case digits and no trailing newline are the same bytes.
        assert_eq!(
            raw_profile(b"\nx\n2\nAB\ncd").as_deref(),
            Some(&[0xAB, 0xCD][..])
        );
        // The length line is not held against the digits, in either direction
        // (measured: ExifTool warns "wrong size" and reads what is there).
        for stated in ["1", "40"] {
            let text = format!("\nx\n{stated}\nabcd\n");
            assert_eq!(
                raw_profile(text.as_bytes()).as_deref(),
                Some(&[0xAB, 0xCD][..])
            );
        }
        // A last digit without its pair is not a byte.
        assert_eq!(raw_profile(b"\nx\n2\nabc\n").as_deref(), Some(&[0xAB][..]));
        assert_eq!(raw_profile(b"\nx\n0\n").as_deref(), Some(&[][..]));
    }

    #[test]
    fn a_text_that_is_not_a_raw_profile_is_refused() {
        for bad in [
            &b""[..],
            b"\n",
            b"\niptc\n",
            b"\niptc\nmany\nabcd\n", // the length line is not a number
            b"\niptc\n2\nabzz\n",    // not hexadecimal
            b"\xff\xfe",
        ] {
            assert_eq!(raw_profile(bad), None, "{:?}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn chunks_walks_a_file_and_stops_at_what_does_not_fit() {
        fn chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut v = (data.len() as u32).to_be_bytes().to_vec();
            v.extend_from_slice(ty);
            v.extend_from_slice(data);
            v.extend_from_slice(&[0; 4]); // the CRC is not checked
            v
        }
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend(chunk(b"IHDR", &[0; 13]));
        png.extend(chunk(b"tEXt", b"Software\0x"));
        png.extend(chunk(b"zTXt", &ztxt("Comment", b"y")));
        png.extend(chunk(b"IEND", &[]));
        png.extend(chunk(b"tEXt", b"After\0the end"));
        let got: Vec<String> = chunks(&png).into_iter().map(|c| c.keyword).collect();
        assert_eq!(got, ["Software", "Comment"]);
        for n in 0..png.len() {
            let _ = chunks(&png[..n]); // never panics on a cut file
        }
        assert!(chunks(b"").is_empty());
    }
}
