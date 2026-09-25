//! Layer 1 — signed provenance manifests (C2PA).
//!
//! Status semantics:
//! - `Present`: a manifest exists, its signature validates, and the signer chains to a
//!   trusted anchor. `details` carries the parsed claim chain plus every
//!   `digitalSourceType` the active manifest declares ([`source_type`]): the term
//!   codes in `digital_source_type`, each occurrence with its assertion, action and
//!   JSON path in `digital_source_type_hits`, and `declares_ai` = any occurrence is a
//!   generative term of the shared vocabulary in [`halftone_core::dst`].
//! - `Absent`: no manifest. Say nothing about origin; most real photos have none.
//! - `Inconclusive`: manifest present but the signature fails, the signer is not
//!   trusted, the hard binding is broken (bytes changed after signing), or it can't be
//!   parsed. The reason codes are in `details.validation_status`.
//!
//! Implementation (c2pa 0.90): settings live in an explicit [`c2pa::Context`] — no
//! thread-local state — built once per assessment, then
//! `Reader::from_context(ctx).with_stream(mime, bytes)`. Everything after that is read
//! from `Reader::json()` so minor API churn in the crate does not break this layer.
//! The c2pa crate ships **no** trust anchors outside its own tests, and verifies
//! trust by default, so an unconfigured Reader treats every signer as untrusted.
//! [`trust::TrustConfig`] supplies the official list (`trust.trust_anchors`) and
//! any operator anchors (`trust.user_anchors`), and records where each came from.
//!
//! Network: off by default. A file whose only provenance is a reference to a remote
//! manifest (XMP `dcterms:provenance`, e.g. Adobe Firefly downloads) is reported as
//! `Inconclusive` with `details.remote_manifest_url`, and nothing is fetched. With
//! [`C2paSource::fetch_remote_manifests`] set, the reference is first read offline,
//! checked by [`check_remote_url`], and only then fetched over HTTPS with a bounded
//! timeout; a manifest obtained that way is validated like an embedded one and
//! reported with `details.fetched_from` and `details.fetched_at`, and the rationale
//! says the result depends on that server and the time of checking. Files with an
//! embedded manifest never touch the network either way. OCSP is never consulted.
//!
pub mod source_type;
// Declared only to pin the floor from DIFFERENTIAL.md D-007; nothing to import.
#[cfg(feature = "c2pa")]
use c2pa_cbor as _;
pub mod trust;
pub use trust::{InternalList, TrustConfig};

use halftone_core::{Asset, Evidence, EvidenceSource, Layer, Modality, SourceId};

// crates/halftone-c2pa/src/lib.rs
/// C2PA manifest validator.
#[derive(Debug, Default)]
pub struct C2paSource {
    /// Which trust lists and anchors the signer is judged against.
    pub trust: TrustConfig,
    /// Fetch a remote manifest when the file carries only a reference to one.
    /// `false` by default: inspection stays offline and a remote-only file is
    /// reported as `Inconclusive` with the URL.
    pub fetch_remote_manifests: bool,
}

/// Upper bound on one remote-manifest fetch, DNS to last byte.
pub const REMOTE_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Redirects followed when fetching a remote manifest (each target is HTTPS-only).
pub const REMOTE_FETCH_MAX_REDIRECTS: u32 = 3;

/// Decide whether a remote-manifest URL taken from a file may be fetched.
///
/// The URL is chosen by whoever wrote the file, so it is checked before any request:
/// `https://` only, a host name rather than an IP literal, and not `localhost`. This
/// keeps a file from steering the tool at loopback or LAN services by address; it
/// does not stop a public name that resolves to a private address (DNS rebinding),
/// so do not enable fetching in a service that inspects untrusted uploads.
pub fn check_remote_url(url: &str) -> Result<(), String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "only https:// references are fetched".to_string())?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err("URLs with credentials are not fetched".into());
    }
    if authority.starts_with('[') {
        return Err("IP-literal hosts are not fetched".into());
    }
    let host = authority.rsplit_once(':').map_or(authority, |(h, _port)| h);
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return Err("the URL has no host".into());
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return Err("localhost is not fetched".into());
    }
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        return Err("IP-literal hosts are not fetched".into());
    }
    Ok(())
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
    use super::trust::ResolvedTrust;
    use super::C2paSource;
    use halftone_core::{Asset, Evidence, EvidenceSource, Status};
    use std::io::Cursor;

    /// Build the validation context. A user PEM bundle is added to the built-in trust
    /// list via `trust.user_anchors`. If the c2pa settings API moves again, this is the
    /// only function to touch.
    ///
    /// `fetch` is `false` for every first read: a file must not be able to make the
    /// tool call out to a URL it carries unless the operator asked for it, and even
    /// then only after [`super::check_remote_url`]. OCSP is never consulted.
    fn context(resolved: &ResolvedTrust, fetch: bool) -> Result<c2pa::Context, String> {
        let mut settings = c2pa::Settings::new()
            .with_value("verify.remote_manifest_fetch", fetch)
            .map_err(|e| e.to_string())?
            .with_value("verify.ocsp_fetch", false)
            .map_err(|e| e.to_string())?;
        if !resolved.internal_pem.trim().is_empty() {
            settings = settings
                .with_value("trust.trust_anchors", resolved.internal_pem.clone())
                .map_err(|e| e.to_string())?;
        }
        if !resolved.custom_pem.trim().is_empty() {
            settings = settings
                .with_value("trust.user_anchors", resolved.custom_pem.clone())
                .map_err(|e| e.to_string())?;
        }
        let ctx = c2pa::Context::new()
            .with_settings(settings)
            .map_err(|e| e.to_string())?;
        if !fetch {
            return Ok(ctx);
        }
        // The crate's default agent has no overall timeout and follows ten redirects
        // to any scheme. One stuck server must not hang a batch.
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(super::REMOTE_FETCH_TIMEOUT))
                .max_redirects(super::REMOTE_FETCH_MAX_REDIRECTS)
                .https_only(true)
                .build(),
        );
        Ok(ctx.with_resolver(agent))
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

        // A broken trust configuration is a misconfiguration, not evidence about the
        // file: say so instead of silently judging against an empty anchor set.
        let resolved = match src.trust.resolve() {
            Ok(r) => r,
            Err(e) => {
                return mk(
                    Status::Inconclusive,
                    format!(
                        "Trust configuration could not be loaded, so signer trust was not \
                         evaluated: {e}"
                    ),
                    serde_json::json!({ "error": e }),
                )
            }
        };
        if resolved.is_empty() {
            tracing::warn!(
                "no trust anchors configured; every signer will be reported as untrusted"
            );
        }
        let ctx = match context(&resolved, false) {
            Ok(c) => c,
            Err(e) => {
                return mk(
                    Status::Inconclusive,
                    format!("c2pa settings rejected the trust configuration: {e}"),
                    serde_json::json!({ "error": e, "trust": resolved.details() }),
                )
            }
        };

        // First read is always offline. Only a remote-only file with fetching enabled
        // gets a second, networked read, so embedded manifests never touch the network.
        let first =
            c2pa::Reader::from_context(ctx).with_stream(&a.mime, Cursor::new(a.bytes.as_slice()));
        let (reader, fetched_from) = match first {
            Ok(r) => (r, None),
            Err(c2pa::Error::RemoteManifestUrl(url)) if src.fetch_remote_manifests => {
                if let Err(why) = super::check_remote_url(&url) {
                    return mk(
                        Status::Inconclusive,
                        format!(
                            "The file carries no manifest of its own, only a reference to a \
                             remote one ({url}). Fetching was enabled but this reference was \
                             refused: {why}. Nothing about the manifest was checked."
                        ),
                        serde_json::json!({ "remote_manifest_url": url, "fetch_refused": why }),
                    );
                }
                tracing::info!(%url, "fetching remote C2PA manifest");
                let fetched_at = halftone_core::now_rfc3339();
                let ctx = match context(&resolved, true) {
                    Ok(c) => c,
                    Err(e) => {
                        return mk(
                            Status::Inconclusive,
                            format!("c2pa settings rejected the fetch configuration: {e}"),
                            serde_json::json!({ "error": e, "remote_manifest_url": url }),
                        )
                    }
                };
                match c2pa::Reader::from_context(ctx)
                    .with_stream(&a.mime, Cursor::new(a.bytes.as_slice()))
                {
                    Ok(r) => {
                        let from = r.remote_url().map(str::to_string).unwrap_or(url);
                        (r, Some((from, fetched_at)))
                    }
                    Err(c2pa::Error::RemoteManifestFetch(why)) => {
                        return mk(
                            Status::Inconclusive,
                            format!(
                                "The file carries no manifest of its own, only a reference to \
                                 a remote one ({url}). Fetching was enabled and failed: {why}. \
                                 Nothing about the manifest was checked; a later attempt may \
                                 succeed or fail differently."
                            ),
                            serde_json::json!({
                                "remote_manifest_url": url,
                                "fetch_error": why,
                                "fetched_at": fetched_at,
                            }),
                        )
                    }
                    Err(e) => {
                        return mk(
                            Status::Inconclusive,
                            format!(
                                "A remote manifest was fetched from {url} but could not be \
                                 read: {e}"
                            ),
                            serde_json::json!({
                                "fetched_from": url,
                                "fetched_at": fetched_at,
                                "error": e.to_string(),
                            }),
                        )
                    }
                }
            }
            Err(c2pa::Error::JumbfNotFound) => {
                return mk(
                    Status::Absent,
                    "No C2PA manifest. Most files have none; this says nothing about origin."
                        .into(),
                    serde_json::Value::Null,
                )
            }
            Err(c2pa::Error::RemoteManifestUrl(url)) => {
                return mk(
                    Status::Inconclusive,
                    format!(
                        "The file carries no manifest of its own, only a reference to a \
                         remote one ({url}). Halftone does not fetch it: inspection stays \
                         offline unless remote fetching is enabled \
                         (`ht inspect --fetch-remote-manifests`), so the manifest is not \
                         verified here."
                    ),
                    serde_json::json!({ "remote_manifest_url": url }),
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
        // Claim v1 carries a `claim_generator` string; claim v2 (c2pa 2.x, e.g. OpenAI)
        // carries `claim_generator_info: [{name, version}]` and no string at all.
        let claim_generator = active
            .get("claim_generator")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| {
                let g = active.get("claim_generator_info")?.as_array()?.first()?;
                let name = g.get("name")?.as_str()?;
                Some(match g.get("version").and_then(|v| v.as_str()) {
                    Some(ver) => format!("{name} {ver}"),
                    None => name.to_string(),
                })
            });
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
        let source_type = crate::source_type::find(&active);
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
            (Some(g), Some(i)) => format!("from {g}, signed by {i}"),
            (Some(g), None) => format!("from {g}, signer unknown"),
            (None, Some(i)) => format!("signed by {i}"),
            (None, None) => "from an unknown claim generator".into(),
        };
        let ai_note = crate::source_type::describe(&source_type);

        let (status, rationale) = match state {
            c2pa::ValidationState::Trusted => (
                Status::Present,
                format!(
                    "Valid C2PA manifest {who}; signer chains to a trusted anchor and the \
                     content hash matches, so the file is unchanged since signing.{ai_note}"
                ),
            ),
            c2pa::ValidationState::Valid => (
                Status::Inconclusive,
                format!(
                    "C2PA manifest {who} is cryptographically valid and the content is \
                     unchanged since signing, but the signer does not chain to any configured \
                     trust anchor (see details.trust for which lists were used).{ai_note}"
                ),
            ),
            c2pa::ValidationState::Invalid => {
                // Distinguish the two things "Invalid" can mean to a reader: the
                // content or signature is broken, or the signing certificate has
                // expired and nothing in the manifest fixes when it was signed. The
                // second is a property of the signer's setup, not of the file's
                // integrity, and it is date-dependent: the same file validated before
                // the certificate's notAfter.
                let codes: Vec<&str> = validation_status
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.get("code").and_then(|c| c.as_str()))
                            .collect()
                    })
                    .unwrap_or_default();
                let expired = codes.contains(&"signingCredential.expired");
                let broken = codes.iter().any(|c| {
                    c.ends_with(".mismatch")
                        || c.starts_with("claimSignature.") && !c.ends_with(".validated")
                });
                let has_timestamp = assertion_labels.iter().any(|l| l == "c2pa.time-stamp");
                if expired && !broken {
                    (
                        Status::Inconclusive,
                        format!(
                            "C2PA manifest {who} is present and its content hash still matches, \
                             but the signing certificate has expired{}. The signature cannot be \
                             placed inside the certificate's validity window, so it no longer \
                             validates; it may have validated when the file was made. Whether a \
                             reader accepts it now depends on the date of checking. See \
                             validation_status.{ai_note}",
                            if has_timestamp {
                                ""
                            } else {
                                " and the manifest carries no trusted time-stamp"
                            }
                        ),
                    )
                } else {
                    (
                        Status::Inconclusive,
                        format!(
                            "C2PA manifest {who} is present but does not validate: the signature \
                             fails or the content was modified after signing. See validation_status.{ai_note}"
                        ),
                    )
                }
            }
        };

        // A fetched manifest is validated exactly like an embedded one (the hard
        // binding is still to these bytes); what changes is that the answer depends on
        // a server and on when it was asked. Say that first.
        let rationale = match &fetched_from {
            Some((url, at)) => format!(
                "Fetched on request: the file carries only a reference, and this manifest \
                 came from {url} at {at}; a later check can differ if the server changes \
                 or removes it. {rationale}"
            ),
            None => rationale,
        };

        let mut details = serde_json::json!({
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
            "declares_ai": source_type.declares_ai,
            "digital_source_type": source_type.codes,
            "digital_source_type_unknown": source_type.unknown,
            "digital_source_type_hits": source_type.hits,
            "vocabulary_version": halftone_core::dst::VOCABULARY_VERSION,
            "trust": resolved.details(),
        });
        // Only present when the manifest came over the network, so an embedded
        // manifest's details are unchanged from earlier versions.
        if let Some((url, at)) = fetched_from {
            details["fetched_from"] = serde_json::Value::String(url);
            details["fetched_at"] = serde_json::Value::String(at);
        }
        mk(status, rationale, details)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetching_is_off_by_default() {
        assert!(!C2paSource::default().fetch_remote_manifests);
    }

    #[test]
    fn adobe_reference_is_fetchable() {
        assert_eq!(
            check_remote_url(
                "https://cai-manifests.adobe.com/manifests/urn-c2pa-38e41b3e-433d-4f05-a6a8-50b1d214dba4-adobe"
            ),
            Ok(())
        );
        assert_eq!(check_remote_url("https://example.com:8443/m.c2pa"), Ok(()));
    }

    #[test]
    fn plain_http_is_refused() {
        // The c2pa-rs fixture libpng-test_with_url.png points here.
        assert!(check_remote_url("http://localhost:5000/libpng-test.c2pa").is_err());
        assert!(check_remote_url("http://cai-manifests.adobe.com/m").is_err());
        assert!(check_remote_url("ftp://example.com/m").is_err());
    }

    #[test]
    fn loopback_and_ip_literals_are_refused() {
        for url in [
            "https://localhost/m",
            "https://LOCALHOST./m",
            "https://api.localhost:8080/m",
            "https://127.0.0.1/m",
            "https://10.0.0.5:443/m",
            "https://169.254.169.254/latest/meta-data",
            "https://[::1]/m",
            "https://[fd00::1]:8443/m",
        ] {
            assert!(check_remote_url(url).is_err(), "{url} should be refused");
        }
    }

    #[test]
    fn credentials_and_empty_hosts_are_refused() {
        assert!(check_remote_url("https://user:pw@example.com/m").is_err());
        assert!(check_remote_url("https:///m").is_err());
        assert!(check_remote_url("https://").is_err());
    }
}
