//! Layer 4 — container forensics. Deterministic, fast, no models.
//!
//! Each check is its own [`halftone_core::EvidenceSource`] so it gets a separate,
//! independently calibratable verdict — never a merged "container score":
//! - [`jpeg::QuantTables`]: quantization tables + chroma subsampling → encoder class
//!   (libjpeg-family re-encode vs Adobe vs camera/proprietary).
//! - [`png::PngWriter`]: PNG chunk inventory + embedded generation-parameters text.
//! - [`exif::ExifConsistency`]: EXIF/XMP self-identification and camera-metadata
//!   contradictions.
//! - `jpeg::DoubleCompression` (planned): DCT-histogram periodicity — needs
//!   coefficient-level entropy decoding, tracked separately.

pub mod exif;
pub mod jpeg;
pub mod png;
pub mod signatures;
