//! Layer 1 — signed provenance manifests (C2PA).
//!
//! Status semantics:
//! - `Present`: a manifest exists, signature validates, and the signer is on the
//!   configured trust list. `details` carries the parsed claim chain.
//! - `Absent`: no manifest. Say nothing about origin; most real photos have none.
//! - `Inconclusive`: manifest present but invalid/untrusted/stripped-and-re-embedded.
//!
//! Implementation: `c2pa` crate `Reader::from_stream`, then walk the manifest store.

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId};

/// C2PA manifest validator.
#[derive(Debug, Default)]
pub struct C2paSource {
    /// Path to a trust-anchor bundle; `None` = built-in C2PA trust list only.
    pub trust_anchors: Option<std::path::PathBuf>,
}

impl EvidenceSource for C2paSource {
    fn id(&self) -> SourceId {
        SourceId { name: "c2pa".into(), version: env!("CARGO_PKG_VERSION").into() }
    }
    fn layer(&self) -> Layer {
        Layer::Manifest
    }
    fn supports(&self, a: &Asset) -> bool {
        matches!(a.modality, Modality::Image | Modality::Video | Modality::Audio)
    }
    fn assess(&self, _a: &Asset) -> halftone_core::Result<Evidence> {
        // TODO: c2pa::Reader::from_stream(&a.mime, Cursor::new(&a.bytes))
        Ok(Evidence::not_applicable(self.layer(), self.id(), "c2pa source not yet implemented"))
    }
}
