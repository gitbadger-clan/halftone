//! Layer 4 — container forensics. Deterministic, fast, no models.
//!
//! Each check is its own [`halftone_core::EvidenceSource`] so it gets a separate,
//! independently calibratable verdict — never a merged "container score":
//! - [`jpeg::QuantTables`] (`jpeg_quant`): quantization + Huffman tables, chroma
//!   subsampling, marker inventory → encoder class and a writer fingerprint looked up
//!   in [`fingerprints::FingerprintDb`].
//! - [`double::DoubleCompression`] (`jpeg_double`): double-quantization comb in the
//!   luma DCT-coefficient histograms, via the baseline decoder in [`jpeg::coeffs`].
//! - [`png::PngWriter`] (`png_writer`): PNG chunk inventory → built-in writer rules
//!   ([`png_rules`]) and exact-hash DB lookup; embedded generation-parameters text.
//! - [`webp::WebpWriter`] (`webp_writer`): WebP chunk inventory and XMP.
//! - [`exif::ExifConsistency`] (`exif_consistency`): self-identifying metadata, camera
//!   contradictions, EXIF-vs-frame dimension conflicts.
//!
//! All sources report *encoder path* and *self-identification* facts. None of them
//! claims authorship on its own; the rationale strings say so.

pub mod double;
pub mod exif;
pub mod fingerprints;
pub mod jpeg;
pub mod png;
pub mod png_rules;
pub mod signatures;
pub mod webp;
