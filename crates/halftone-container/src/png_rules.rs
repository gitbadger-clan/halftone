//! Rule-based PNG writer recognition, built in — no harvesting needed.
//!
//! PNG carries no quantization tables, so the writer signal is the *ancillary chunk
//! inventory*: which optional chunks a writer emits, in what order, with which text
//! keywords and which ICC profile name. That is a small, readable rule rather than an
//! opaque hash, so the rules live here as data with an honest confidence level.
//!
//! The inventory alone over-attributes: many encoders write exactly IHDR/IDAT/IEND, and
//! several add only pHYs. Two more signals separate them, both readable from the first
//! bytes of the image data without decompressing anything:
//!
//! - **how the data is split into IDAT chunks** ([`IdatRule`]). An encoder that flushes
//!   a fixed buffer writes equal blocks; one that compresses the whole image first
//!   writes a single chunk.
//! - **how the compressed stream starts** ([`PngRule::zlib`], [`PngRule::deflate_block`],
//!   [`PngRule::deflate_prefix`]): the zlib header, the coding of the first deflate
//!   block, and, for an encoder with a hard-coded Huffman table, a constant run of bytes.
//!
//! Measured 2026-11-12 on one 512×512 image saved by each encoder (and on small, wide
//! and synthetic images where a rule depends on size):
//!
//! | encoder | ancillary | IDAT | zlib | first block |
//! |---|---|---|---|---|
//! | Pillow 10.1, 10.4, 11.3, 12.0, 13.0 (also imageio, matplotlib) | none unless asked | blocks of max(65,536, 4 × width) | by level | dynamic |
//! | Rust `png` 0.18 fast mode (`image` crate default; candle, `ht survive`) | none | one chunk | `78 01` | constant 52-byte fdeflate header |
//! | Rust `png` 0.18 default / best | none | one chunk | `78 9c` / `78 da` | dynamic |
//! | lodepng 20261102, pngjs 7.0, pypng, hand-written `zlib.compress` | none | one chunk | `78 01` or `78 9c` | dynamic |
//! | stb_image_write | none | one chunk | `78 5e` | fixed |
//! | OpenCV 5.0 (libpng) | none | 8,192-byte blocks | `78 01` | dynamic |
//! | ImageMagick 6.9.12 | cHRM, bKGD (none with `-strip`) | 32,768-byte blocks | `78 da` | dynamic |
//! | FFmpeg 6.1 | pHYs | 4,096-byte blocks | `78 9c` | dynamic |
//! | libvips via sharp 0.35 | pHYs | 8,192-byte blocks | `78 9c` | dynamic |
//! | Skia via @napi-rs/canvas 1.1 | sBIT, sRGB | 8,192-byte blocks | `78 9c` | dynamic |
//! | libvips via sharp 0.35 keeping metadata | iCCP `icc`, eXIf, pHYs, XMP text | 8,192-byte blocks | `78 9c` | dynamic |
//! | Pillow 13.0 with `icc_profile` | iCCP `ICC Profile` | 64 KiB blocks | `78 9c` | dynamic |
//! | macOS Preview (16 corpus files) | gAMA, cHRM, eXIf, iTXt | 16,384-byte blocks | `78 01` | dynamic |
//!
//! Below the block size every encoder writes one chunk, so a small file names a writer
//! only if its stream does (the fdeflate header, stb's fixed block). A rule's name must
//! not claim more than its conditions separate: where several encoders share a shape,
//! the name says so and lists what was measured.

use serde::Serialize;

use crate::fingerprints::WriterClass;
use crate::png::PngInfo;

/// How the rule was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Checked against real files from the named writer during development.
    Verified,
    /// From the writer's documented or widely reported layout; not yet checked here.
    Documented,
}

/// 64 KiB: the smallest block Pillow's encoder flushes at.
pub const PILLOW_BLOCK: usize = 65_536;

/// The block Pillow flushes at for an image this wide: 64 KiB, or four bytes per
/// pixel of one row when that is more (measured: 80,000 bytes at 20,000 px wide).
pub fn pillow_block(width: u32) -> usize {
    PILLOW_BLOCK.max(width as usize * 4)
}

/// The first 16 bytes of every compressed stream the fdeflate crate writes for
/// compressible data: the start of its hard-coded Huffman table (fdeflate 0.3.7; the
/// constant run is 52 bytes long). On incompressible data fdeflate falls back to
/// stored blocks and this prefix is absent.
pub const FDEFLATE_HEAD: &[u8] = &[
    0xed, 0xc0, 0x03, 0xa0, 0x24, 0x59, 0x96, 0xc6, 0xf1, 0xff, 0x77, 0xee, 0x8d, 0xc8, 0xcc, 0xa7,
];

/// A condition on how the image data is split into IDAT chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdatRule {
    /// At least two chunks, all but the last exactly [`pillow_block`] bytes.
    Pillow,
    /// [`IdatRule::Pillow`], or a single chunk no longer than that block: what Pillow
    /// writes for any image, small ones included.
    PillowOrSmall,
    /// At least two chunks, all but the last exactly this many bytes.
    Blocks(usize),
    /// [`IdatRule::Blocks`], or a single chunk of at most that many bytes.
    BlocksOrSmall(usize),
    /// A single chunk, whatever its size.
    Single,
    /// A single chunk longer than [`pillow_block`]: every measured block-writing
    /// encoder would have split it.
    SingleLarge,
    /// A single chunk of at most [`pillow_block`] bytes.
    SingleSmall,
}

impl IdatRule {
    fn holds(self, info: &PngInfo) -> bool {
        let one = info.idat_chunks == 1;
        let pillow = pillow_block(info.width);
        match self {
            IdatRule::Pillow => info.idat_block == Some(pillow),
            IdatRule::PillowOrSmall => {
                info.idat_block == Some(pillow) || (one && info.idat_max <= pillow)
            }
            IdatRule::Blocks(n) => info.idat_block == Some(n),
            IdatRule::BlocksOrSmall(n) => info.idat_block == Some(n) || (one && info.idat_max <= n),
            IdatRule::Single => one,
            IdatRule::SingleLarge => one && info.idat_max > pillow,
            IdatRule::SingleSmall => one && info.idat_max <= pillow,
        }
    }
}

/// One writer rule. All listed conditions must hold.
#[derive(Debug, Clone, Serialize)]
pub struct PngRule {
    /// Writer name as it appears in rationales.
    pub name: &'static str,
    /// Writer class.
    pub class: WriterClass,
    /// Confidence.
    pub confidence: Confidence,
    /// Chunk types that must all be present.
    pub all_of: &'static [&'static str],
    /// At least one of these chunk types must be present (empty = no requirement).
    pub any_of: &'static [&'static str],
    /// Chunk types that must all be absent.
    pub none_of: &'static [&'static str],
    /// At least one of these text keywords must be present (empty = no requirement).
    pub any_text: &'static [&'static str],
    /// If set, the iCCP profile name must contain this (case-insensitive).
    pub icc_contains: Option<&'static str>,
    /// If non-empty, the XMP packet (`iTXt XML:com.adobe.xmp`) must contain at least one.
    pub xmp_contains_any: &'static [&'static str],
    /// If set, the shape the IDAT chunks must have.
    pub idat: Option<IdatRule>,
    /// If set, the two-byte zlib header the image data must start with.
    pub zlib: Option<[u8; 2]>,
    /// If set, how the first deflate block must be coded (`stored`, `fixed`, `dynamic`).
    pub deflate_block: Option<&'static str>,
    /// If non-empty, the bytes the compressed stream must start with after the zlib
    /// header.
    pub deflate_prefix: &'static [u8],
    /// What the rule is based on.
    pub notes: &'static str,
}

/// Built-in rules, most specific first. First match wins.
pub const RULES: &[PngRule] = &[
    PngRule {
        name: "ComfyUI (Pillow, workflow/prompt text chunks)",
        class: WriterClass::Generator,
        confidence: Confidence::Verified,
        all_of: &["tEXt"],
        any_of: &[],
        none_of: &["pHYs", "gAMA", "sRGB", "cHRM"],
        any_text: &["workflow", "prompt"],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: None,
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "ComfyUI SaveImage node writes the graph JSON into tEXt `workflow`/`prompt` via Pillow.",
    },
    PngRule {
        name: "Stable Diffusion WebUI (Pillow, `parameters` text chunk)",
        class: WriterClass::Generator,
        confidence: Confidence::Verified,
        all_of: &["tEXt"],
        any_of: &[],
        none_of: &["pHYs", "gAMA", "sRGB", "cHRM"],
        any_text: &["parameters"],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: None,
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "AUTOMATIC1111/Forge write the generation parameters into tEXt `parameters`.",
    },
    PngRule {
        name: "macOS screenshot (Apple ImageIO, XMP UserComment=Screenshot)",
        class: WriterClass::Screenshot,
        confidence: Confidence::Documented,
        all_of: &["iTXt"],
        any_of: &[],
        none_of: &["gAMA", "sRGB"],
        any_text: &["XML:com.adobe.xmp"],
        icc_contains: None,
        xmp_contains_any: &[">Screenshot<"],
        idat: None,
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "Since macOS Monterey, screencapture writes an XMP packet with exif:UserComment = Screenshot, plus pHYs and iCCP (Display P3).",
    },
    PngRule {
        name: "Apple ImageIO (macOS/iOS export or screenshot)",
        class: WriterClass::Screenshot,
        confidence: Confidence::Documented,
        all_of: &["iDOT"],
        any_of: &[],
        none_of: &[],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: None,
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "Apple's private iDOT parallel-decode chunk, written by ImageIO on macOS/iOS.",
    },
    PngRule {
        name: "Adobe Photoshop / Lightroom (XMP CreatorTool)",
        class: WriterClass::Editor,
        confidence: Confidence::Documented,
        all_of: &["iTXt"],
        any_of: &[],
        none_of: &["iDOT"],
        any_text: &["XML:com.adobe.xmp"],
        icc_contains: None,
        xmp_contains_any: &[
            "CreatorTool=\"Adobe",
            "<xmp:CreatorTool>Adobe",
            "Adobe Photoshop",
            "Adobe Lightroom",
        ],
        idat: None,
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "Adobe writers name themselves in xmp:CreatorTool inside the iTXt XMP packet.",
    },
    PngRule {
        name: "Windows Imaging Component (Snipping Tool, Paint)",
        class: WriterClass::Screenshot,
        confidence: Confidence::Documented,
        all_of: &["sRGB", "gAMA", "pHYs"],
        any_of: &[],
        none_of: &["iDOT", "tEXt", "iTXt", "iCCP"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: None,
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "WIC writes sRGB, gAMA, pHYs and nothing else; libpng-based apps usually add tEXt.",
    },
    PngRule {
        name: "macOS Preview saving as PNG (Apple ImageIO: gAMA, cHRM, eXIf and XMP; 16 KiB blocks)",
        class: WriterClass::Editor,
        confidence: Confidence::Verified,
        all_of: &["gAMA", "cHRM", "eXIf", "iTXt"],
        any_of: &[],
        none_of: &["iDOT", "sRGB", "iCCP", "pHYs"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &["xmptk=\"XMP Core"],
        idat: Some(IdatRule::BlocksOrSmall(16_384)),
        zlib: Some([0x78, 0x01]),
        deflate_block: None,
        deflate_prefix: &[],
        notes: "16 corpus files (2026): images copied in a browser or app, pasted into Preview and saved as PNG. The XMP toolkit string is the one Apple writes, there is no iDOT, and IDAT is flushed every 16,384 bytes with zlib header 78 01. Not measured on other macOS versions or on other ImageIO callers.",
    },
    PngRule {
        name: "libpng-based application (GIMP, browsers, many tools)",
        class: WriterClass::Library,
        confidence: Confidence::Documented,
        all_of: &["gAMA"],
        any_of: &[],
        none_of: &["iDOT"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: None,
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "Default libpng writers emit gAMA (often with sRGB/cHRM/pHYs and tEXt Software).",
    },
    // --- Bare streams: IHDR/IDAT/IEND and nothing else. The stream decides first,
    // then the chunk shape; the last two name no single writer. ---
    PngRule {
        name: "the Rust `png` crate in fast mode (bare IHDR/IDAT/IEND, one chunk, fdeflate stream; what the `image` crate writes by default)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &[],
        any_of: &[],
        none_of: &[
            "pHYs", "gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "tEXt", "iTXt", "zTXt", "eXIf", "sBIT",
            "bKGD", "tIME",
        ],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::Single),
        zlib: Some([0x78, 0x01]),
        deflate_block: None,
        deflate_prefix: FDEFLATE_HEAD,
        notes: "png 0.18.1 with fdeflate 0.3.7: candle's Stable Diffusion example and ht survive's PNG output, 15 kB to 23 MB. The stream opens with fdeflate's hard-coded Huffman table, so this holds for small files too. Incompressible data makes fdeflate use stored blocks; such a file falls to the one-shot rule.",
    },
    PngRule {
        name: "stb_image_write (bare IHDR/IDAT/IEND, one chunk, fixed-Huffman stream)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &[],
        any_of: &[],
        none_of: &[
            "pHYs", "gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "tEXt", "iTXt", "zTXt", "eXIf", "sBIT",
            "bKGD", "tIME",
        ],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::Single),
        zlib: Some([0x78, 0x5e]),
        deflate_block: Some("fixed"),
        deflate_prefix: &[],
        notes: "stb_image_write as of 2026-11: zlib header 78 5e and a fixed-Huffman first block, which no other measured encoder writes.",
    },
    PngRule {
        name: "Pillow default (bare IHDR/IDAT/IEND, image data in 64 KiB blocks)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &[],
        any_of: &[],
        none_of: &[
            "pHYs", "gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "tEXt", "iTXt", "zTXt", "eXIf", "sBIT",
            "bKGD", "tIME",
        ],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::Pillow),
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "Pillow writes no ancillary chunks unless asked and flushes IDAT at max(65,536, 4 x width) bytes; measured on 10.1, 10.4, 11.3, 12.0 and 13.0, at default, optimize and every compress_level, and at 20,000 px wide. imageio writes through Pillow.",
    },
    PngRule {
        name: "libpng at its default 8 KiB buffer with no ancillary chunks (bare IHDR/IDAT/IEND; OpenCV writes this, and so can any libpng program)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &[],
        any_of: &[],
        none_of: &[
            "pHYs", "gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "tEXt", "iTXt", "zTXt", "eXIf", "sBIT",
            "bKGD", "tIME",
        ],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::Blocks(8_192)),
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "OpenCV 5.0 cv2.imwrite at default (zlib 78 01) and at level 9. Also seen on a Google Flow image saved from the browser (zlib 78 5e), so the name is the library, not a program.",
    },
    PngRule {
        name: "ImageMagick with metadata stripped (bare IHDR/IDAT/IEND, 32 KiB blocks)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &[],
        any_of: &[],
        none_of: &[
            "pHYs", "gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "tEXt", "iTXt", "zTXt", "eXIf", "sBIT",
            "bKGD", "tIME",
        ],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::Blocks(32_768)),
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "ImageMagick 6.9.12 `convert -strip`. ImageMagick 7 not measured.",
    },
    PngRule {
        name: "a one-shot PNG encoder (bare IHDR/IDAT/IEND, all image data in one chunk; lodepng, pngjs, pypng, a hand-written zlib writer and the Rust `png` crate outside fast mode all write this)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &[],
        any_of: &[],
        none_of: &[
            "pHYs", "gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "tEXt", "iTXt", "zTXt", "eXIf", "sBIT",
            "bKGD", "tIME",
        ],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::SingleLarge),
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "One chunk longer than any measured block-writer would leave unsplit. The encoders listed were each measured and cannot be told apart from the first bytes: their zlib headers overlap (78 01 for lodepng and pngjs, 78 9c for pypng, hand-written writers and the Rust crate at default).",
    },
    PngRule {
        name: "a bare IHDR/IDAT/IEND stream whose writer cannot be told (image data this small is one chunk in every encoder)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &[],
        any_of: &[],
        none_of: &[
            "pHYs", "gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "tEXt", "iTXt", "zTXt", "eXIf", "sBIT",
            "bKGD", "tIME",
        ],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::SingleSmall),
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "Pillow, libpng-based writers and one-shot encoders all write one chunk below their block size; measured on Pillow and the Rust `png` crate. The layout names no writer.",
    },
    // --- pHYs and nothing else that names a colour space. ---
    PngRule {
        name: "Pillow with pHYs (dpi set, no colour chunks)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &["pHYs"],
        any_of: &[],
        none_of: &["gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "iTXt"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::PillowOrSmall),
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "Pillow `save(dpi=...)` adds only pHYs; matplotlib adds a Software text chunk as well. Other pHYs-only writers flush smaller blocks (FFmpeg 4 KiB, libvips 8 KiB), so the Pillow block size is required unless the file is too small to show it.",
    },
    PngRule {
        name: "FFmpeg's PNG encoder (pHYs only, 4 KiB blocks)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &["pHYs"],
        any_of: &[],
        none_of: &[
            "gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "tEXt", "iTXt", "zTXt", "sBIT", "bKGD", "eXIf",
        ],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::Blocks(4_096)),
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "FFmpeg 6.1.1, `ffmpeg -i in out.png`.",
    },
    PngRule {
        name: "libvips, as sharp uses it (pHYs only, 8 KiB blocks)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &["pHYs"],
        any_of: &[],
        none_of: &[
            "gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "tEXt", "iTXt", "zTXt", "sBIT", "bKGD", "eXIf",
        ],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::Blocks(8_192)),
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "sharp 0.35.5 `.png().toFile()`. Any libpng program that writes pHYs alone at the default buffer would look the same.",
    },
    // --- Other measured inventories. ---
    PngRule {
        name: "Skia's PNG encoder (sBIT and sRGB, 8 KiB blocks; measured on a Skia canvas binding)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &["sBIT", "sRGB"],
        any_of: &[],
        none_of: &["gAMA", "cHRM", "iCCP", "iDOT", "pHYs"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::BlocksOrSmall(8_192)),
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "@napi-rs/canvas 1.1.1 `toBuffer(\"image/png\")`. Chrome and Android also encode through Skia, but neither was measured: the name claims the binding only.",
    },
    PngRule {
        name: "ImageMagick (cHRM and bKGD, 32 KiB blocks)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &["cHRM", "bKGD"],
        any_of: &[],
        none_of: &["iDOT", "iCCP", "sBIT"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::BlocksOrSmall(32_768)),
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "ImageMagick 6.9.12 `convert in.png out.png`. Other versions add tIME and date text chunks, which this rule allows.",
    },
    // --- Shapes measured on the generator corpus (2026-11). Each name says what
    // the files show and where they were seen; none claims a service pipeline. ---
    PngRule {
        name: "libvips keeping metadata, as sharp writes with keepMetadata (eXIf and pHYs, 8 KiB blocks)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &["eXIf", "pHYs"],
        any_of: &[],
        none_of: &["gAMA", "cHRM", "sRGB", "iDOT", "sBIT", "bKGD", "tEXt"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::Blocks(8_192)),
        zlib: Some([0x78, 0x9c]),
        deflate_block: None,
        deflate_prefix: &[],
        notes: "sharp 0.35.5 keepExif/keepMetadata/withMetadata: IHDR, iCCP named `icc` when kept, eXIf, pHYs, then XMP text. 21 Canva downloads have this layout, the one with an ICC profile under the same name `icc`.",
    },
    PngRule {
        name: "Pillow with an embedded ICC profile (iCCP named `ICC Profile`, as Pillow names every profile)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &["iCCP"],
        any_of: &[],
        none_of: &["gAMA", "cHRM", "sRGB", "iDOT", "sBIT", "bKGD", "pHYs"],
        any_text: &[],
        icc_contains: Some("ICC Profile"),
        xmp_contains_any: &[],
        idat: Some(IdatRule::PillowOrSmall),
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "Pillow 13.0 `save(icc_profile=...)`: the iCCP chunk is always named `ICC Profile`. Seen on an Ideogram transparent-background download.",
    },
    PngRule {
        name: "libpng at its default 8 KiB buffer with an XMP chunk and nothing else (seen on Adobe Firefly Image 5 downloads)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &["iTXt"],
        any_of: &[],
        none_of: &[
            "pHYs", "gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "tEXt", "zTXt", "eXIf", "sBIT", "bKGD",
            "tIME", "caBX",
        ],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::Blocks(8_192)),
        zlib: Some([0x78, 0x01]),
        deflate_block: None,
        deflate_prefix: &[],
        notes: "10 Adobe Firefly Image 5 web downloads: IHDR, iTXt, IDAT, IEND, zlib header 78 01. The encoder behind them is not known; the shape is that of libpng.",
    },
    PngRule {
        name: "libpng at 8 KiB blocks with a compressed text profile and XMP, zlib header 78 5e (seen on Google-generated PNGs served by resellers)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &["zTXt", "iTXt"],
        any_of: &[],
        none_of: &[
            "pHYs", "gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "eXIf", "sBIT", "bKGD",
        ],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::Blocks(8_192)),
        zlib: Some([0x78, 0x5e]),
        deflate_block: None,
        deflate_prefix: &[],
        notes: "2 corpus files, the output of a Google model downloaded from Adobe Firefly and from an aggregator: IHDR, zTXt, iTXt, caBX. The zTXt chunk is where ExifTool finds their IPTC record (DIFFERENTIAL.md D-021); it is compressed, so this crate does not read it.",
    },
    PngRule {
        name: "Pillow with text metadata",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &[],
        any_of: &["tEXt", "iTXt", "zTXt"],
        none_of: &["pHYs", "gAMA", "sRGB", "cHRM", "iCCP", "iDOT"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        idat: Some(IdatRule::PillowOrSmall),
        zlib: None,
        deflate_block: None,
        deflate_prefix: &[],
        notes: "Pillow with pnginfo: tEXt/iTXt/zTXt before IDAT, still no colour chunks, and Pillow's block size unless the file is too small to show it.",
    },
];

/// Result of matching.
#[derive(Debug, Clone, Serialize)]
pub struct RuleMatch {
    /// Writer name.
    pub name: &'static str,
    /// Class.
    pub class: WriterClass,
    /// Confidence.
    pub confidence: Confidence,
}

/// First rule that matches `info`.
pub fn match_rules(info: &PngInfo) -> Option<RuleMatch> {
    RULES.iter().find(|r| matches(r, info)).map(|r| RuleMatch {
        name: r.name,
        class: r.class,
        confidence: r.confidence,
    })
}

fn matches(r: &PngRule, info: &PngInfo) -> bool {
    if !r.all_of.iter().all(|c| info.has(c)) {
        return false;
    }
    if !r.any_of.is_empty() && !r.any_of.iter().any(|c| info.has(c)) {
        return false;
    }
    if r.none_of.iter().any(|c| info.has(c)) {
        return false;
    }
    if !r.any_text.is_empty()
        && !r
            .any_text
            .iter()
            .any(|k| info.text.iter().any(|t| t.keyword.eq_ignore_ascii_case(k)))
    {
        return false;
    }
    if !r.xmp_contains_any.is_empty() {
        let xmp = info
            .text
            .iter()
            .find(|t| t.keyword == "XML:com.adobe.xmp")
            .map(|t| t.value.as_str());
        match xmp {
            Some(x) if r.xmp_contains_any.iter().any(|n| x.contains(n)) => {}
            _ => return false,
        }
    }
    if r.idat.is_some_and(|shape| !shape.holds(info)) {
        return false;
    }
    if r.zlib.is_some_and(|z| info.zlib_header != Some(z)) {
        return false;
    }
    if r.deflate_block
        .is_some_and(|b| info.deflate_block() != Some(b))
    {
        return false;
    }
    if !info.deflate_head.starts_with(r.deflate_prefix) {
        return false;
    }
    if let Some(needle) = r.icc_contains {
        match &info.icc_name {
            Some(n) if n.to_lowercase().contains(&needle.to_lowercase()) => {}
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::png::TextEntry;

    fn info_v(chunks: &[&str], text: &[(&str, &str)]) -> PngInfo {
        PngInfo {
            chunks: chunks.iter().map(|s| s.to_string()).collect(),
            text: text
                .iter()
                .map(|(k, v)| TextEntry {
                    keyword: k.to_string(),
                    value: v.to_string(),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn info(chunks: &[&str], text: &[&str]) -> PngInfo {
        let t: Vec<(&str, &str)> = text.iter().map(|k| (*k, "xxxxxxxxxxxxxxxxxxxx")).collect();
        info_v(chunks, &t)
    }

    fn name(i: &PngInfo) -> Option<&'static str> {
        match_rules(i).map(|m| m.name)
    }

    #[test]
    fn rules_pick_expected_writers() {
        assert!(
            name(&info(&["IHDR", "tEXt", "IDAT", "IEND"], &["parameters"]))
                .unwrap()
                .starts_with("Stable Diffusion")
        );
        assert!(
            name(&info(
                &["IHDR", "tEXt", "tEXt", "IDAT", "IEND"],
                &["prompt", "workflow"]
            ))
            .unwrap()
            .starts_with("ComfyUI")
        );
        assert!(
            name(&info(
                &["IHDR", "iDOT", "pHYs", "iCCP", "IDAT", "IEND"],
                &[]
            ))
            .unwrap()
            .starts_with("Apple")
        );
        assert!(
            name(&info(
                &["IHDR", "sRGB", "gAMA", "pHYs", "IDAT", "IEND"],
                &[]
            ))
            .unwrap()
            .starts_with("Windows")
        );
        assert!(
            name(&info(
                &["IHDR", "gAMA", "cHRM", "tEXt", "IDAT", "IEND"],
                &["Software"]
            ))
            .unwrap()
            .starts_with("libpng")
        );
    }

    /// The layout one encoder was measured to write: ancillary chunks, image width,
    /// IDAT chunk lengths, zlib header and the first bytes of the deflate stream.
    fn measured(anc: &[&str], width: u32, idat: &[usize], zlib: [u8; 2], head: &[u8]) -> PngInfo {
        let mut chunks = vec!["IHDR"];
        chunks.extend(anc);
        chunks.extend(idat.iter().map(|_| "IDAT"));
        chunks.push("IEND");
        let even = idat.len() >= 2
            && idat[..idat.len() - 1].iter().all(|&n| n == idat[0])
            && idat[idat.len() - 1] <= idat[0];
        PngInfo {
            width,
            idat_chunks: idat.len(),
            idat_max: idat.iter().copied().max().unwrap_or(0),
            idat_block: even.then(|| idat[0]),
            zlib_header: Some(zlib),
            deflate_head: head.to_vec(),
            ..info(&chunks, &[])
        }
    }

    /// `n` full blocks of `size` and a shorter last one.
    fn blocks(n: usize, size: usize) -> Vec<usize> {
        let mut v = vec![size; n];
        v.push(size / 3);
        v
    }

    // First deflate bytes as measured: a dynamic block from zlib, fdeflate's constant
    // header, stb's fixed block, a stored block.
    const DYNAMIC: &[u8] = &[0x74, 0xfd, 0x59, 0x93, 0x6d, 0xcb, 0x91, 0x26];
    const FIXED: &[u8] = &[0x63, 0xb4, 0xb1, 0x35, 0xfb, 0xf3, 0x87, 0x91];
    const STORED: &[u8] = &[0x00, 0xff, 0xff, 0x00, 0x00, 0x00, 0xf7, 0xc1];

    #[test]
    fn every_measured_encoder_gets_the_rule_its_layout_supports() {
        const Z_FAST: [u8; 2] = [0x78, 0x01];
        const Z_DEF: [u8; 2] = [0x78, 0x9c];
        const Z_BEST: [u8; 2] = [0x78, 0xda];
        let b64 = blocks(6, 65_536);
        let cases: Vec<(&str, PngInfo, &str)> = vec![
            (
                "Pillow 10.1-13.0 default",
                measured(&[], 512, &b64, Z_DEF, DYNAMIC),
                "Pillow default",
            ),
            (
                "Pillow optimize",
                measured(&[], 512, &b64, Z_BEST, DYNAMIC),
                "Pillow default",
            ),
            (
                "Pillow compress_level=1",
                measured(&[], 512, &b64, Z_FAST, DYNAMIC),
                "Pillow default",
            ),
            (
                "Pillow level 0",
                measured(&[], 512, &blocks(12, 65_536), Z_FAST, STORED),
                "Pillow default",
            ),
            (
                "Pillow, 20,000 px wide",
                measured(&[], 20_000, &blocks(30, 80_000), Z_DEF, STORED),
                "Pillow default",
            ),
            (
                "Pillow dpi",
                measured(&["pHYs"], 512, &b64, Z_DEF, DYNAMIC),
                "Pillow with pHYs",
            ),
            (
                "matplotlib",
                measured(&["tEXt", "pHYs"], 512, &b64, Z_DEF, DYNAMIC),
                "Pillow with pHYs",
            ),
            (
                "Rust png fast (candle)",
                measured(&[], 512, &[440_932], Z_FAST, FDEFLATE_HEAD),
                "the Rust `png` crate in fast mode",
            ),
            (
                "Rust png fast, 96 px",
                measured(&[], 96, &[14_743], Z_FAST, FDEFLATE_HEAD),
                "the Rust `png` crate in fast mode",
            ),
            (
                "Rust png fast on noise",
                measured(&[], 3200, &[23_044_166], Z_FAST, STORED),
                "a one-shot PNG encoder",
            ),
            (
                "Rust png default",
                measured(&[], 512, &[405_701], Z_DEF, DYNAMIC),
                "a one-shot PNG encoder",
            ),
            (
                "Rust png best",
                measured(&[], 512, &[400_485], Z_BEST, DYNAMIC),
                "a one-shot PNG encoder",
            ),
            (
                "lodepng",
                measured(&[], 512, &[404_699], Z_FAST, DYNAMIC),
                "a one-shot PNG encoder",
            ),
            (
                "pngjs",
                measured(&[], 512, &[418_944], Z_FAST, DYNAMIC),
                "a one-shot PNG encoder",
            ),
            (
                "pypng / hand-written zlib",
                measured(&[], 512, &[516_242], Z_DEF, DYNAMIC),
                "a one-shot PNG encoder",
            ),
            (
                "stb_image_write",
                measured(&[], 512, &[553_610], [0x78, 0x5e], FIXED),
                "stb_image_write",
            ),
            (
                "OpenCV",
                measured(&[], 512, &blocks(54, 8_192), Z_FAST, DYNAMIC),
                "libpng at its default 8 KiB buffer",
            ),
            (
                "ImageMagick -strip",
                measured(&[], 512, &blocks(12, 32_768), Z_BEST, DYNAMIC),
                "ImageMagick with metadata stripped",
            ),
            (
                "ImageMagick",
                measured(&["cHRM", "bKGD"], 512, &blocks(12, 32_768), Z_BEST, DYNAMIC),
                "ImageMagick (cHRM and bKGD",
            ),
            (
                "FFmpeg",
                measured(&["pHYs"], 512, &blocks(126, 4_096), Z_DEF, DYNAMIC),
                "FFmpeg's PNG encoder",
            ),
            (
                "sharp / libvips",
                measured(&["pHYs"], 512, &blocks(63, 8_192), Z_DEF, DYNAMIC),
                "libvips, as sharp uses it",
            ),
            (
                "Skia canvas",
                measured(&["sBIT", "sRGB"], 512, &blocks(55, 8_192), Z_DEF, DYNAMIC),
                "Skia's PNG encoder",
            ),
            (
                "Pillow, 96 px",
                measured(&[], 96, &[15_096], Z_DEF, DYNAMIC),
                "a bare IHDR/IDAT/IEND stream whose writer cannot be told",
            ),
        ];
        for (encoder, layout, want) in &cases {
            let got = name(layout).unwrap_or("(no rule)");
            assert!(
                got.starts_with(want),
                "{encoder}: got `{got}`, want `{want}…`"
            );
        }
        // No rule names Pillow for a layout Pillow was not measured to write.
        for (encoder, layout, _) in cases
            .iter()
            .filter(|c| !c.0.contains("Pillow") && c.0 != "matplotlib")
        {
            assert!(!name(layout).unwrap_or("").contains("Pillow"), "{encoder}");
        }
    }

    /// A measured layout that also carries an XMP packet and, optionally, an ICC name.
    fn with_meta(mut i: PngInfo, xmp: &str, icc: Option<&str>) -> PngInfo {
        if !xmp.is_empty() {
            i.text = info_v(&["IHDR", "IEND"], &[("XML:com.adobe.xmp", xmp)]).text;
        }
        i.icc_name = icc.map(str::to_string);
        i
    }

    #[test]
    fn layouts_measured_on_the_generator_corpus_get_their_rule() {
        const APPLE_XMP: &str = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"XMP Core 6.0.0\">";
        const ADOBE_XMP: &str =
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Adobe XMP Core 7.0-c000\">";
        let z01 = [0x78, 0x01];
        let z9c = [0x78, 0x9c];
        let z5e = [0x78, 0x5e];
        let preview = &["gAMA", "cHRM", "eXIf", "iTXt"];
        let canva = &["caBX", "eXIf", "pHYs", "iTXt"];
        let cases: Vec<(&str, PngInfo, &str)> = vec![
            // 16 clipboard images saved by Preview: 49 to 441 blocks of 16 KiB.
            (
                "Preview save",
                with_meta(
                    measured(preview, 1024, &blocks(370, 16_384), z01, DYNAMIC),
                    APPLE_XMP,
                    None,
                ),
                "macOS Preview saving as PNG",
            ),
            (
                "Preview save, small",
                with_meta(
                    measured(preview, 64, &[9_000], z01, DYNAMIC),
                    APPLE_XMP,
                    None,
                ),
                "macOS Preview saving as PNG",
            ),
            // The same chunks from a writer that is not Apple's stay with the generic rule.
            (
                "gAMA/cHRM, Adobe toolkit",
                with_meta(
                    measured(preview, 1024, &blocks(40, 16_384), z01, DYNAMIC),
                    ADOBE_XMP,
                    None,
                ),
                "libpng-based application",
            ),
            (
                "gAMA/cHRM, 8 KiB blocks",
                with_meta(
                    measured(preview, 1024, &blocks(40, 8_192), z01, DYNAMIC),
                    APPLE_XMP,
                    None,
                ),
                "libpng-based application",
            ),
            // 21 Canva downloads, and sharp 0.35 keeping metadata.
            (
                "Canva download",
                with_meta(
                    measured(canva, 1024, &blocks(250, 8_192), z9c, DYNAMIC),
                    ADOBE_XMP,
                    None,
                ),
                "libvips keeping metadata",
            ),
            (
                "Canva free tier",
                with_meta(
                    measured(
                        &["iCCP", "eXIf", "pHYs"],
                        1024,
                        &blocks(166, 8_192),
                        z9c,
                        DYNAMIC,
                    ),
                    "",
                    Some("icc"),
                ),
                "libvips keeping metadata",
            ),
            (
                "sharp keepMetadata",
                with_meta(
                    measured(
                        &["iCCP", "eXIf", "pHYs", "zTXt"],
                        512,
                        &blocks(55, 8_192),
                        z9c,
                        DYNAMIC,
                    ),
                    "",
                    Some("icc"),
                ),
                "libvips keeping metadata",
            ),
            (
                "sharp keepExif",
                measured(&["eXIf", "pHYs"], 512, &blocks(55, 8_192), z9c, DYNAMIC),
                "libvips keeping metadata",
            ),
            (
                "sharp default",
                measured(&["pHYs"], 512, &blocks(63, 8_192), z9c, DYNAMIC),
                "libvips, as sharp uses it (pHYs only",
            ),
            // Pillow names every embedded profile "ICC Profile"; libvips names it "icc".
            (
                "Pillow icc_profile",
                with_meta(
                    measured(&["iCCP"], 512, &blocks(6, 65_536), z9c, DYNAMIC),
                    "",
                    Some("ICC Profile"),
                ),
                "Pillow with an embedded ICC profile",
            ),
            (
                "Ideogram no-bg",
                with_meta(
                    measured(&["iCCP"], 1024, &[65_536, 65_536, 30_000], z9c, DYNAMIC),
                    "",
                    Some("ICC Profile"),
                ),
                "Pillow with an embedded ICC profile",
            ),
            (
                "Pillow icc_profile, small",
                with_meta(
                    measured(&["iCCP"], 64, &[7_034], z9c, DYNAMIC),
                    "",
                    Some("ICC Profile"),
                ),
                "Pillow with an embedded ICC profile",
            ),
            // 10 Firefly Image 5 downloads; 2 Google-generated PNGs from resellers.
            (
                "Firefly Image 5",
                with_meta(
                    measured(&["iTXt"], 1024, &blocks(544, 8_192), z01, DYNAMIC),
                    APPLE_XMP,
                    None,
                ),
                "libpng at its default 8 KiB buffer with an XMP chunk and nothing else",
            ),
            (
                "Google PNG via reseller",
                with_meta(
                    measured(
                        &["zTXt", "iTXt", "caBX"],
                        1024,
                        &blocks(219, 8_192),
                        z5e,
                        DYNAMIC,
                    ),
                    ADOBE_XMP,
                    None,
                ),
                "libpng at 8 KiB blocks with a compressed text profile and XMP",
            ),
            // Unchanged by the new rules.
            (
                "Firefly fill (Pillow)",
                with_meta(
                    measured(&["iTXt", "eXIf"], 1024, &blocks(20, 65_536), z9c, DYNAMIC),
                    APPLE_XMP,
                    None,
                ),
                "Pillow with text metadata",
            ),
            (
                "OpenAI (Rust fast) with manifest",
                measured(&["caBX"], 1024, &[1_954_010], z01, FDEFLATE_HEAD),
                "the Rust `png` crate in fast mode",
            ),
            (
                "OpenAI transparent (Pillow)",
                measured(&["caBX"], 1024, &blocks(31, 65_536), z9c, DYNAMIC),
                "Pillow default",
            ),
            (
                "Google Flow browser save",
                measured(&[], 1024, &blocks(718, 8_192), z5e, DYNAMIC),
                "libpng at its default 8 KiB buffer with no ancillary chunks",
            ),
        ];
        for (what, layout, want) in &cases {
            let got = name(layout).unwrap_or("(no rule)");
            assert!(got.starts_with(want), "{what}: got `{got}`, want `{want}…`");
        }
        // An ICC profile under libvips' name is not Pillow's, whatever the blocks.
        let icc = with_meta(
            measured(&["iCCP"], 512, &blocks(6, 65_536), z9c, DYNAMIC),
            "",
            Some("icc"),
        );
        assert_eq!(name(&icc), None);
    }

    #[test]
    fn shapes_nobody_was_measured_to_write_get_no_rule() {
        let z = [0x78, 0x9c];
        for other in [
            &[16_384, 16_384, 100][..],
            &[65_536, 100, 65_536],
            &[100, 65_536],
            &[],
        ] {
            assert_eq!(
                name(&measured(&[], 512, other, z, DYNAMIC)),
                None,
                "{other:?}"
            );
        }
        // pHYs with blocks no pHYs-only writer uses, and text chunks without Pillow's
        // block size, are not Pillow.
        assert_eq!(
            name(&measured(&["pHYs"], 512, &blocks(9, 16_384), z, DYNAMIC)),
            None
        );
        assert_eq!(
            name(&measured(&["tEXt"], 512, &[440_932], z, DYNAMIC)),
            None
        );
        // Pillow's block is four bytes per pixel of a row once that exceeds 64 KiB:
        // 64 KiB blocks in a 20,000 px wide image are not Pillow's.
        assert_eq!(pillow_block(512), 65_536);
        assert_eq!(pillow_block(20_000), 80_000);
        assert_eq!(
            name(&measured(&[], 20_000, &blocks(30, 65_536), z, DYNAMIC)),
            None
        );
    }

    #[test]
    fn xmp_content_separates_mac_screenshot_from_photoshop() {
        let mac = info_v(
            &["IHDR", "pHYs", "iCCP", "iTXt", "IDAT", "IEND"],
            &[(
                "XML:com.adobe.xmp",
                "<x:xmpmeta><exif:UserComment><rdf:Alt><rdf:li>Screenshot</rdf:li></rdf:Alt></exif:UserComment></x:xmpmeta>",
            )],
        );
        assert!(
            name(&mac).unwrap().starts_with("macOS screenshot"),
            "{:?}",
            name(&mac)
        );
        let ps = info_v(
            &["IHDR", "pHYs", "iTXt", "IDAT", "IEND"],
            &[(
                "XML:com.adobe.xmp",
                "<rdf:Description xmp:CreatorTool=\"Adobe Photoshop 26.0\"/>",
            )],
        );
        assert!(name(&ps).unwrap().starts_with("Adobe"), "{:?}", name(&ps));
        // An XMP packet that names nobody is not evidence of either.
        assert_eq!(
            name(&info(
                &["IHDR", "pHYs", "iTXt", "IDAT", "IEND"],
                &["XML:com.adobe.xmp"]
            )),
            None
        );
    }

    #[test]
    fn every_rule_is_reachable_by_a_synthetic_layout() {
        for r in RULES {
            let mut chunks: Vec<&str> = vec!["IHDR"];
            chunks.extend(r.all_of.iter().copied());
            chunks.extend(r.any_of.first().copied());
            if !r.any_text.is_empty() && !chunks.contains(&"tEXt") && !chunks.contains(&"iTXt") {
                chunks.push("tEXt");
            }
            chunks.extend(["IDAT", "IEND"]);
            let needle = r
                .xmp_contains_any
                .first()
                .copied()
                .unwrap_or("xxxxxxxxxxxxxxxxxxxx");
            let mut text: Vec<(&str, &str)> = r.any_text.iter().map(|k| (*k, needle)).collect();
            if !r.xmp_contains_any.is_empty() && r.any_text.is_empty() {
                text.push(("XML:com.adobe.xmp", needle));
            }
            let mut i = info_v(&chunks, &text);
            i.icc_name = r.icc_contains.map(str::to_string);
            i.width = 512;
            let set = |i: &mut PngInfo, lens: &[usize]| {
                let at = i.chunks.len() - 2; // the single IDAT placed above
                i.chunks
                    .splice(at..=at, lens.iter().map(|_| "IDAT".to_string()));
                i.idat_chunks = lens.len();
                i.idat_max = lens.iter().copied().max().unwrap_or(0);
                i.idat_block = (lens.len() >= 2).then(|| lens[0]);
            };
            match r.idat {
                Some(IdatRule::Pillow | IdatRule::PillowOrSmall) => {
                    set(&mut i, &[65_536, 65_536, 9])
                }
                Some(IdatRule::Blocks(n) | IdatRule::BlocksOrSmall(n)) => set(&mut i, &[n, n, 9]),
                Some(IdatRule::SingleLarge) => set(&mut i, &[70_000]),
                Some(IdatRule::Single | IdatRule::SingleSmall) | None => set(&mut i, &[9_000]),
            }
            i.zlib_header = r.zlib.or(Some([0x78, 0x9c]));
            i.deflate_head = match r.deflate_block {
                Some("fixed") => FIXED.to_vec(),
                Some("stored") => STORED.to_vec(),
                _ if !r.deflate_prefix.is_empty() => r.deflate_prefix.to_vec(),
                _ => DYNAMIC.to_vec(),
            };
            assert!(
                matches(r, &i),
                "rule `{}` cannot match its own layout",
                r.name
            );
        }
    }
}
