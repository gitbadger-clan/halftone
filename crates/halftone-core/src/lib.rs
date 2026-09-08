//! Halftone core: the evidence model every layer implements.
//!
//! Design rules (do not relax without a schema version bump):
//! - Every source produces one [`Evidence`] with its own [`Status`] and calibration.
//! - There is no merged score. An [`Inspection`] is a list of evidence, nothing more.
//! - Every evidence records the source name + version + calibration set hash so old
//!   verdicts stay interpretable after models are swapped.

pub mod asset;
pub mod batch;
pub mod dst;
pub mod evidence;
pub mod registry;

pub use asset::{Asset, AssetInfo, Modality};
pub use batch::{render_matrix, summarize, Batch, FileRow};
pub use evidence::{
    CalibrationRef, Evidence, Inspection, Layer, SourceId, Statistic, Status, ToolInfo,
    SCHEMA_VERSION,
};
pub use registry::{EvidenceSource, Registry};

/// Errors from evidence sources.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The source cannot handle this asset (wrong modality / format).
    #[error("unsupported asset: {0}")]
    Unsupported(String),
    /// The asset could not be parsed.
    #[error("parse error: {0}")]
    Parse(String),
    /// A required model pack is missing or failed verification.
    #[error("model pack unavailable: {0}")]
    Pack(String),
    /// Any other failure inside a source.
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
    /// I/O.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
