//! The trait every layer implements, and the runner that executes them.

use std::time::Instant;

use crate::{Asset, Evidence, Inspection, Layer, SourceId, ToolInfo, SCHEMA_VERSION};

/// One evidence source. Implementations live in the layer crates.
///
/// Contract:
/// - `supports` must be cheap (header/modality check only).
/// - `assess` must never panic on hostile input; return `Err` or `Inconclusive`.
/// - Statistical sources must fill `statistic` and `calibration`.
pub trait EvidenceSource: Send + Sync {
    /// Stable identity and version.
    fn id(&self) -> SourceId;
    /// Which layer this belongs to.
    fn layer(&self) -> Layer;
    /// Whether this source applies to the asset at all.
    fn supports(&self, asset: &Asset) -> bool;
    /// Produce evidence.
    fn assess(&self, asset: &Asset) -> crate::Result<Evidence>;
}

/// Ordered collection of sources. Runs cheapest layer first.
#[derive(Default)]
pub struct Registry {
    sources: Vec<Box<dyn EvidenceSource>>,
}

impl Registry {
    /// Empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a source. Sources are re-sorted by layer on each run.
    pub fn with(mut self, source: impl EvidenceSource + 'static) -> Self {
        self.sources.push(Box::new(source));
        self
    }

    /// Add a boxed source.
    pub fn push(&mut self, source: Box<dyn EvidenceSource>) {
        self.sources.push(source);
    }

    /// Run every applicable source and collect an [`Inspection`].
    /// A source error becomes `Inconclusive` evidence with the error in `rationale`,
    /// so one broken layer never hides the others.
    pub fn inspect(&self, asset: &Asset, tool: ToolInfo) -> Inspection {
        let mut order: Vec<&dyn EvidenceSource> = self.sources.iter().map(|s| s.as_ref()).collect();
        order.sort_by_key(|s| s.layer());

        let mut evidence = Vec::with_capacity(order.len());
        for src in order {
            let id = src.id();
            let layer = src.layer();
            if !src.supports(asset) {
                evidence.push(Evidence::not_applicable(
                    layer,
                    id,
                    "source does not support this asset",
                ));
                continue;
            }
            let t0 = Instant::now();
            let mut ev = match src.assess(asset) {
                Ok(ev) => ev,
                Err(e) => {
                    tracing::warn!(source = %id.name, error = %e, "source failed");
                    Evidence {
                        layer,
                        source: id,
                        status: crate::Status::Inconclusive,
                        statistic: None,
                        calibration: None,
                        rationale: format!("source failed: {e}"),
                        details: serde_json::Value::Null,
                        duration_ms: 0,
                    }
                }
            };
            ev.duration_ms = t0.elapsed().as_millis() as u64;
            evidence.push(ev);
        }

        Inspection {
            schema_version: SCHEMA_VERSION.to_string(),
            tool,
            asset: asset.info(),
            evidence,
            created_at: now_rfc3339(),
        }
    }

    /// Sources registered, for `halftone sources`.
    pub fn ids(&self) -> Vec<(Layer, SourceId)> {
        self.sources.iter().map(|s| (s.layer(), s.id())).collect()
    }
}

/// UTC timestamp without pulling in `chrono`; good enough for a stamp.
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil-from-days (Howard Hinnant). Correct for all dates after 1970.
    let days = (secs / 86_400) as i64;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Modality, Status};

    struct Stub(Layer, &'static str, Status);
    impl EvidenceSource for Stub {
        fn id(&self) -> SourceId {
            SourceId {
                name: self.1.into(),
                version: "test".into(),
            }
        }
        fn layer(&self) -> Layer {
            self.0
        }
        fn supports(&self, a: &Asset) -> bool {
            a.modality == Modality::Image
        }
        fn assess(&self, _: &Asset) -> crate::Result<Evidence> {
            Ok(Evidence {
                layer: self.0,
                source: self.id(),
                status: self.2,
                statistic: None,
                calibration: None,
                rationale: "stub".into(),
                details: serde_json::Value::Null,
                duration_ms: 0,
            })
        }
    }

    struct Failing;
    impl EvidenceSource for Failing {
        fn id(&self) -> SourceId {
            SourceId {
                name: "boom".into(),
                version: "test".into(),
            }
        }
        fn layer(&self) -> Layer {
            Layer::Blind
        }
        fn supports(&self, _: &Asset) -> bool {
            true
        }
        fn assess(&self, _: &Asset) -> crate::Result<Evidence> {
            Err(crate::Error::Parse("bad".into()))
        }
    }

    fn tool() -> ToolInfo {
        ToolInfo {
            name: "halftone".into(),
            version: "test".into(),
        }
    }

    #[test]
    fn runs_in_layer_order_and_survives_errors() {
        let reg = Registry::new()
            .with(Stub(Layer::Blind, "blind", Status::Absent))
            .with(Failing)
            .with(Stub(Layer::Manifest, "c2pa", Status::Present));
        let asset = Asset::from_bytes(vec![0xFF, 0xD8, 0xFF, 0xE0], None).unwrap();
        let out = reg.inspect(&asset, tool());
        let names: Vec<_> = out
            .evidence
            .iter()
            .map(|e| e.source.name.as_str())
            .collect();
        assert_eq!(names, ["c2pa", "blind", "boom"]);
        assert_eq!(out.evidence[2].status, Status::Inconclusive);
        assert_eq!(out.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn unsupported_becomes_not_applicable() {
        let reg = Registry::new().with(Stub(Layer::Container, "jpeg", Status::Present));
        let asset = Asset::text("hello", None);
        let out = reg.inspect(&asset, tool());
        assert_eq!(out.evidence[0].status, Status::NotApplicable);
    }

    #[test]
    fn timestamp_shape() {
        let t = now_rfc3339();
        assert_eq!(t.len(), 20);
        assert!(t.ends_with('Z'));
    }
}
