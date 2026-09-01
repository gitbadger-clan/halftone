//! Layer 4 — container forensics. Deterministic, fast, no models.
//!
//! Sub-sources (each is its own [`EvidenceSource`] so they get separate verdicts):
//! - [`jpeg::QuantTables`]: match luma/chroma quant tables and subsampling against a
//!   fingerprint DB of camera firmwares, editors, and generator export paths.
//! - `jpeg::DoubleCompression` (planned): DCT-histogram periodicity.
//! - `png::ChunkOrder` (planned): chunk order/ancillary chunks vs. known writers.
//! - `exif::Consistency` (planned): make/model vs. quant tables vs. dimensions.

pub mod jpeg;
