//! Pixel-domain, model-free forensics.
//!
//! This is the first crate on the pixel side of the line: it decodes image data
//! (via the `image` crate) but holds no models and downloads nothing. Every source
//! here is *statistical* — it fills [`halftone_core::Statistic`] and is expected to
//! run in "dark mode" (`threshold: None`, verdict `Inconclusive`, statistic reported)
//! until `halftone bench` has produced a calibration on a labelled corpus.
//!
//! Sources report under [`halftone_core::Layer::Container`] for now; if a distinct
//! `Pixel` layer is wanted later it is a one-variant enum addition plus a verdict
//! schema version bump.

pub mod lattice;
pub mod residual;

pub use lattice::LatticeSource;
