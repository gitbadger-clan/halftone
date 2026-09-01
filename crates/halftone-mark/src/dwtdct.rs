//! Port of the `invisible-watermark` DWT-DCT scheme (ShieldMnt/invisible-watermark),
//! the default mark in Stable Diffusion 1.x/2.x/SDXL reference pipelines.
//!
//! Signal lives in: block-DCT coefficients of the level-1 Haar LL band of each
//! channel. Key: none beyond the payload bytes ("SDV2" style). Survives: mild JPEG,
//! small resize. Scored by: bit accuracy over the 32/48-bit payload.

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId};

/// DWT-DCT decoder configured with an expected payload.
#[derive(Debug, Clone)]
pub struct DwtDct {
    /// Expected payload bits (e.g. b"SDV2" for SD 2.x reference outputs).
    pub payload: Vec<u8>,
}

impl EvidenceSource for DwtDct {
    fn id(&self) -> SourceId {
        SourceId { name: "dwtdct".into(), version: env!("CARGO_PKG_VERSION").into() }
    }
    fn layer(&self) -> Layer {
        Layer::Mark
    }
    fn supports(&self, a: &Asset) -> bool {
        a.modality == Modality::Image
    }
    fn assess(&self, _a: &Asset) -> halftone_core::Result<Evidence> {
        // TODO: decode → Haar DWT → 4x4 block DCT → threshold → bit accuracy → z-test.
        Ok(Evidence::not_applicable(self.layer(), self.id(), "dwtdct decoder not yet implemented"))
    }
}
