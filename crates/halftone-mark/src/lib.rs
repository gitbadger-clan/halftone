//! Layer 2 — keyed watermarks. Both directions:
//! - `detect`: decoders for schemes whose key/decoder we hold
//!   (`invisible-watermark` DWT-DCT, AudioSeal, user-supplied SynthID-Text keys).
//! - `embed`: applying our own mark. Not started; signing is not part of `ht` yet.
//!
//! Detection is a hypothesis test: report bit accuracy vs. the expected payload
//! and a p-value under a random-bits null. Always fill `calibration`.

pub mod dwtdct;

pub use dwtdct::DwtDct;
