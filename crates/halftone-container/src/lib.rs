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
//! - [`marking::MarkingMetadata`] (`marking_metadata`): the IPTC `DigitalSourceType`
//!   self-declaration in XMP (JPEG APP1 + ExtendedXMP, PNG iTXt, WebP), and the
//!   legacy IPTC IIM block from APP13, as explicit fields.
//! - [`data_mining::DataMiningReservation`] (`data_mining_reservation`): the IPTC / PLUS
//!   `Data Mining` property in XMP: whether the file's metadata reserves rights
//!   against data mining and AI training. Unsigned, like the marking field.
//! - [`vendor_ai::VendorAiFlag`] (`vendor_ai_flag`): a vendor's own yes/no field saying
//!   the file contains AI-generated content, outside the IPTC vocabulary.
//! - [`rights::RightsStatements`] (`rights_statements`): copyright notice, usage terms,
//!   licence, credit and release fields, quoted as written; copies that disagree.
//! - [`identifiers::EmbeddedIdentifiers`] (`embedded_identifiers`): the privacy listing:
//!   person names, contact details, account and tracking records, device and document
//!   identifiers, location. All three read their fields through [`embedded`].
//! - [`gcontainer::ContainerDirectory`] (`container_directory`): the Motion Photo / Ultra HDR
//!   container directory checked against the bytes the file holds.
//! - [`mpf::MpfIndex`] (`mpf_index`): the MPF index (CIPA DC-007) checked against the
//!   bytes; the only declaration of an iPhone JPEG's gain map.
//!
//! All sources report *encoder path* and *self-identification* facts. None of them
//! claims authorship on its own; the rationale strings say so.

pub mod data_mining;
pub mod double;
pub mod embedded;
pub mod exif;
pub mod fingerprints;
pub mod gcontainer;
pub mod identifiers;
pub mod jpeg;
pub mod marking;
pub mod mpf;
pub mod png;
pub mod png_rules;
pub mod rights;
pub mod signatures;
pub mod vendor_ai;
pub mod webp;
