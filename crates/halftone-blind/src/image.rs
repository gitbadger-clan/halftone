//! Frozen vision-encoder features + linear head (UniversalFakeDetect lineage,
//! Ojha et al. 2023). Encoder and head are separate ONNX graphs so the head can be
//! retrained and re-calibrated without re-shipping the encoder.

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId};

/// Feature-probe detector backed by a model pack.
#[derive(Debug)]
pub struct FeatureProbe {
    /// Pack id, e.g. `blind-image-dinov2-probe`.
    pub pack: String,
}

impl EvidenceSource for FeatureProbe {
    fn id(&self) -> SourceId {
        SourceId {
            name: "feature_probe".into(),
            version: self.pack.clone(),
        }
    }
    fn layer(&self) -> Layer {
        Layer::Blind
    }
    fn supports(&self, a: &Asset) -> bool {
        a.modality == Modality::Image
    }
    fn assess(&self, _a: &Asset) -> halftone_core::Result<Evidence> {
        // TODO: packs::load(&self.pack)? → preprocess → encoder → head → logit
        //       → threshold from pack calibration.json → Evidence with Statistic.
        Ok(Evidence::not_applicable(
            self.layer(),
            self.id(),
            "no blind model pack installed",
        ))
    }
}
