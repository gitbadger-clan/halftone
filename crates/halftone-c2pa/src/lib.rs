//! Layer 1 — signed provenance manifests (C2PA).
//!
//! Status semantics:
//! - `Present`: a manifest exists, its signature validates, and the signer chains to a
//!   trusted anchor. `details` carries the parsed claim chain, and `declares_ai`
//!   says whether the manifest itself declares the asset as (partly) generated
//!   (`digitalSourceType` = `trainedAlgorithmicMedia` / `compositeWithTrainedAlgorithmicMedia`).
//! - `Absent`: no manifest. Say nothing about origin; most real photos have none.
//! - `Inconclusive`: manifest present but the signature fails, the signer is not
//!   trusted, the hard binding is broken (bytes changed after signing), or it can't be
//!   parsed. The reason codes are in `details.validation_status`.
//!
//! Implementation (c2pa 0.90): settings live in an explicit [`c2pa::Context`] — no
//! thread-local state — built once per assessment, then
//! `Reader::from_context(ctx).with_stream(mime, bytes)`. Everything after that is read
//! from `Reader::json()` so minor API churn in the crate does not break this layer.
//! Trust anchors go into `trust.user_anchors`, which *adds* to the crate's built-in
//! C2PA trust list rather than replacing it.

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId};

/// C2PA manifest validator.
#[derive(Debug, Default)]
pub struct C2paSource {
    /// Path to a PEM bundle of trust anchors; `None` = the c2pa crate's built-in list.
    pub trust_anchors: Option<std::path::PathBuf>,
}

impl EvidenceSource for C2paSource {
    fn id(&self) -> SourceId {
        SourceId {
            name: "c2pa".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
    fn layer(&self) -> Layer {
        Layer::Manifest
    }
    fn supports(&self, a: &Asset) -> bool {
        matches!(
            a.modality,
            Modality::Image | Modality::Video | Modality::Audio
        )
    }
    #[cfg(not(feature = "c2pa"))]
    fn assess(&self, _a: &Asset) -> halftone_core::Result<Evidence> {
        Ok(Evidence::not_applicable(
            self.layer(),
            self.id(),
            "built without the `c2pa` feature; manifest validation disabled",
        ))
    }
    #[cfg(feature = "c2pa")]
    fn assess(&self, a: &Asset) -> halftone_core::Result<Evidence> {
        Ok(imp::assess(self, a))
    }
}

#[cfg(feature = "c2pa")]
mod imp {
    use super::C2paSource;
    use halftone_core::{Asset, Evidence, EvidenceSource, Status};
    use std::io::Cursor;

    /// Build the validation context. A user PEM bundle is added to the built-in trust
    /// list via `trust.user_anchors`. If the c2pa settings API moves again, this is the
    /// only function to touch.
    fn context(src: &C2paSource) -> Result<c2pa::Context, String> {
        let mut settings = c2pa::Settings::new();
        if let Some(p) = &src.trust_anchors {
            let pem = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
            settings = settings
                .with_value("trust.user_anchors", pem)
                .map_err(|e| e.to_string())?;
        }
        c2pa::Context::new()
            .with_settings(settings)
            .map_err(|e| e.to_string())
    }

    /// Depth-first search for any string value containing `needle`.
    fn json_contains(v: &serde_json::Value, needle: &str) -> bool {
        match v {
            serde_json::Value::String(s) => s.contains(needle),
            serde_json::Value::Array(a) => a.iter().any(|x| json_contains(x, needle)),
            serde_json::Value::Object(o) => o.values().any(|x| json_contains(x, needle)),
            _ => false,
        }
    }

    pub(super) fn assess(src: &C2paSource, a: &Asset) -> Evidence {
        let mk = |status: Status, rationale: String, details: serde_json::Value| Evidence {
            layer: src.layer(),
            source: src.id(),
            status,
            statistic: None,
            calibration: None,
            rationale,
            details,
            duration_ms: 0,
        };

        let ctx = match context(src) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "could not load trust anchors; using built-in list only");
                c2pa::Context::new()
            }
        };

        let reader = match c2pa::Reader::from_context(ctx)
            .with_stream(&a.mime, Cursor::new(a.bytes.as_slice()))
        {
            Ok(r) => r,
            Err(c2pa::Error::JumbfNotFound) => {
                return mk(
                    Status::Absent,
                    "No C2PA manifest. Most files have none; this says nothing about origin."
                        .into(),
                    serde_json::Value::Null,
                )
            }
            Err(e) => {
                return mk(
                    Status::Inconclusive,
                    format!("A manifest container is present but could not be read: {e}"),
                    serde_json::json!({ "error": e.to_string() }),
                )
            }
        };

        let state = reader.validation_state();
        let js: serde_json::Value =
            serde_json::from_str(&reader.json()).unwrap_or(serde_json::Value::Null);
        let active_label = js
            .get("active_manifest")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let active = active_label
            .as_deref()
            .and_then(|l| js.get("manifests").and_then(|m| m.get(l)))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let claim_generator = active
            .get("claim_generator")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let title = active
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let issuer = active
            .get("signature_info")
            .and_then(|s| s.get("issuer"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let signed_at = active
            .get("signature_info")
            .and_then(|s| s.get("time"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let ingredients = active
            .get("ingredients")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let assertion_labels: Vec<String> = active
            .get("assertions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.get("label").and_then(|l| l.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let declares_ai = json_contains(&active, "trainedAlgorithmicMedia");
        let manifests = js
            .get("manifests")
            .and_then(|m| m.as_object())
            .map(|m| m.len())
            .unwrap_or(0);
        let validation_status = js
            .get("validation_status")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let who = match (&claim_generator, &issuer) {
            (Some(g), Some(i)) => format!("{g}, signed by {i}"),
            (Some(g), None) => g.clone(),
            (None, Some(i)) => format!("signed by {i}"),
            (None, None) => "unknown claim generator".into(),
        };
        let ai_note = if declares_ai {
            " The manifest itself declares the content as generated or composited with a trained algorithm."
        } else {
            ""
        };

        let (status, rationale) = match state {
            c2pa::ValidationState::Trusted => (
                Status::Present,
                format!(
                    "Valid C2PA manifest from {who}; signer chains to a trusted anchor and the \
                     content hash matches, so the file is unchanged since signing.{ai_note}"
                ),
            ),
            c2pa::ValidationState::Valid => (
                Status::Inconclusive,
                format!(
                    "C2PA manifest from {who} is cryptographically valid and the content is \
                     unchanged since signing, but the signer is not on the trust list — \
                     self-signed or unknown certificate.{ai_note}"
                ),
            ),
            c2pa::ValidationState::Invalid => (
                Status::Inconclusive,
                format!(
                    "C2PA manifest from {who} is present but does not validate: the signature \
                     fails or the content was modified after signing. See validation_status.{ai_note}"
                ),
            ),
        };

        mk(
            status,
            rationale,
            serde_json::json!({
                "validation_state": format!("{state:?}"),
                "validation_status": validation_status,
                "active_manifest": active_label,
                "manifests": manifests,
                "claim_generator": claim_generator,
                "title": title,
                "issuer": issuer,
                "signed_at": signed_at,
                "ingredients": ingredients,
                "assertions": assertion_labels,
                "declares_ai": declares_ai,
            }),
        )
    }
}
