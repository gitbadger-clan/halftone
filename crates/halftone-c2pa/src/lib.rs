//! Layer 1 — signed provenance manifests (C2PA).
//!
//! Status semantics:
//! - `Present`: a manifest exists, its signature validates, and the signer chains to a
//!   trusted anchor. `details` carries the parsed claim chain plus every
//!   `digitalSourceType` the active manifest declares ([`source_type`]): the term
//!   codes in `digital_source_type`, each occurrence with its assertion, action and
//!   JSON path in `digital_source_type_hits`, and `declares_ai` = any manifest on the parentOf
//!   lineage declares a generative term ([lineage]); declares_ai_active keeps the active manifest's
//!   own answer.
//! - `Absent`: no manifest. Say nothing about origin; most real photos have none.
//! - `Inconclusive`: manifest present but the signature fails, the signer is not
//!   trusted, the hard binding is broken (bytes changed after signing), or it can't be
//!   parsed. The reason codes are in `details.validation_status`.
//!
//! Multi-part files ([`multi_asset`], DIFFERENTIAL.md D-014): when c2pa reports the
//! whole-file hard binding as failed and the manifest carries a
//! `c2pa.hash.multi-asset` assertion, Halftone applies C2PA 2.4 §15.12.4 itself,
//! which c2pa 0.91 does not: each part (the still, a gain map, a motion clip) is
//! checked against its own signed hash, with c2pa's data-hash code. If every part
//! still in the file matches and only parts the signer marked optional are gone,
//! the content binding holds and the verdict is decided as for an intact file
//! (`Present` for a trusted signer); the rationale names what was removed. If a part
//! changed, the rationale names which one. `details.validation_state` stays what c2pa
//! reported, so it still compares with c2patool; `details.multi_asset` holds the
//! outcome, the status code and the per-part table. The fallback runs only when the
//! claim signature validated, nothing else failed, and every assertion it reads is
//! bound to the signed claim. Parts are named ("part 2, the motion clip") from what
//! the file itself declares at exactly the signed place (`parts[].declared_as`;
//! `declared_parts_agree` says whether every signed part lines up). Declarations are
//! unsigned: they name parts and never change an outcome.
//!
//! Shelf life ([`shelf_life`], DIFFERENTIAL.md D-010, D-012): for every manifest with
//! a signature, `details.signing_cert` carries the signer certificate's serial and
//! validity window and `details.time_stamp` the time-stamp status under the loaded
//! TSA list, read from c2pa's `validation_results` (`timeStamp.*` codes are success
//! or informational, never in `validation_status`). When no trusted time-stamp
//! fixes the signing time, the rationale of a `Trusted` or `Valid` manifest names
//! the time after which a check reports the certificate expired. The verdict does
//! not change; the date belongs to the verifier, not the file. `details.signed_at`
//! is the time the time-stamp token states, trusted or not.
//!
//! Implementation (c2pa 0.90): settings live in an explicit [`c2pa::Context`] — no
//! thread-local state — built once per assessment, then
//! `Reader::from_context(ctx).with_stream(mime, bytes)`. Everything after that is read
//! from `Reader::json()` so minor API churn in the crate does not break this layer.
//! The c2pa crate ships **no** trust anchors outside its own tests, and verifies
//! trust by default, so an unconfigured Reader treats every signer as untrusted.
//! [`trust::TrustConfig`] supplies the official lists and any operator anchors as
//! typed c2pa `trust.anchors`, each tagged manifest or TSA ([`AnchorKind`]), and
//! records where each came from.
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
//! Signals ([`signals`]): `details.signals` holds the C2PA Asset Signals Rubric 1.0
//! run over the whole store, from c2pa's crJSON export, with each node named by its
//! manifest label. A signal classifies what a signer declared; it is reported next
//! to the validation state and never changes the verdict.
//!
pub mod lineage;
pub mod multi_asset;
pub mod shelf_life;
pub mod signals;
pub mod source_type;
pub mod trust;
pub use trust::{AnchorKind, InternalList, TrustConfig};

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
    use super::C2paSource;
    use super::shelf_life::{self, SigningCert, TimeStampStatus};
    use super::trust::{AnchorKind, ResolvedTrust};
    use crate::multi_asset::{self, Locator, Outcome, PartHash, PartReport, PartResult, Report};
    use c2pa::settings::{TrustAnchor, TrustListKind};
    use halftone_core::{Asset, Evidence, EvidenceSource, Status};
    use std::io::Cursor;

    /// Checks one part of a multi-part file with c2pa's own data-hash code, so a
    /// part is hashed exactly as c2pa hashes the whole file (same exclusion rules).
    struct C2paPartHasher;

    impl multi_asset::PartHasher for C2paPartHasher {
        fn verify(&self, part: &PartHash, bytes: &[u8]) -> Result<bool, String> {
            let alg = part.alg.as_deref().unwrap_or("sha256");
            // c2pa hashes an unknown algorithm to an empty value, which would read as
            // a mismatch; refuse it instead.
            if !matches!(alg, "sha256" | "sha384" | "sha512") {
                return Err(format!("unsupported hash algorithm {alg}"));
            }
            let mut dh = c2pa::assertions::DataHash::new("part", alg);
            for &(start, length) in &part.exclusions {
                if start.saturating_add(length) > bytes.len() as u64 {
                    return Err("an exclusion range runs past the end of the part".into());
                }
                dh.add_exclusion(c2pa::HashRange::new(start, length));
            }
            dh.set_hash(part.hash.clone());
            match dh.verify_in_memory_hash(bytes, Some(alg)) {
                Ok(()) => Ok(true),
                Err(c2pa::Error::HashMismatch(_)) => Ok(false),
                Err(e) => Err(e.to_string()),
            }
        }
    }

    /// The active manifest's failure and success codes, each with its assertion URL.
    fn statuses(reader: &c2pa::Reader) -> (Vec<multi_asset::Status>, Vec<multi_asset::Status>) {
        let Some(s) = reader
            .validation_results()
            .and_then(|r| r.active_manifest())
        else {
            return (Vec::new(), Vec::new());
        };
        (
            s.failure()
                .iter()
                .map(|v| multi_asset::Status::new(v.code(), v.url()))
                .collect(),
            s.success()
                .iter()
                .map(|v| multi_asset::Status::new(v.code(), v.url()))
                .collect(),
        )
    }

    /// C2PA 2.4 §15.12.4 for the active manifest. `None` when it does not apply: the
    /// whole-file hard binding did not fail, or there is no multi-asset assertion.
    /// The assertions come from c2pa's detailed JSON, the only view that keeps the
    /// hash assertions (`Reader::json` drops them).
    fn multi_asset_fallback(
        reader: &c2pa::Reader,
        active_label: Option<&str>,
        bytes: &[u8],
    ) -> Option<Report> {
        let (failures, successes) = statuses(reader);
        if !multi_asset::hard_binding_failed(&failures) {
            return None;
        }
        let detailed: serde_json::Value = serde_json::from_str(&reader.detailed_json()).ok()?;
        let store = detailed
            .get("manifests")?
            .get(active_label?)?
            .get("assertion_store")?;
        let ma = store.get(multi_asset::LABEL)?;
        let labels = multi_asset::part_labels(ma);
        if let Err(why) = multi_asset::eligible(&failures, &successes, &labels) {
            return Some(Report::not_evaluated(why));
        }
        let mut report = multi_asset::evaluate(ma, store, bytes, &C2paPartHasher);
        // Names for the rationale, from what the file itself declares at exactly a
        // signed part's place (its container directory, today). Naming only: the
        // declaration is unsigned and never changes the outcome.
        multi_asset::name_from_declared(
            &mut report,
            &halftone_formats::parts::declared_parts(bytes),
        );
        Some(report)
    }

    /// "part 2 (motion clip, MP4, optional)": index, what the file declares at that
    /// place (when it declares something), what the bytes look like (when they are
    /// there), and whether the signer allowed it to be removed.
    fn part_name(p: &PartReport) -> String {
        let mut what: Vec<&str> = Vec::new();
        if let Some(name) = p.declared_as.as_deref() {
            what.push(halftone_formats::parts::describe(name));
        }
        match p.kind {
            Some("jpeg") => what.push("JPEG"),
            Some("mp4") => what.push("MP4"),
            _ => {}
        }
        what.push(if p.optional { "optional" } else { "required" });
        format!("part {} ({})", p.index, what.join(", "))
    }

    /// Names of the parts whose result satisfies `want`, in signed order.
    fn parts_where(r: &Report, want: impl Fn(&PartResult) -> bool) -> Vec<String> {
        r.parts
            .iter()
            .filter(|p| want(&p.result))
            .map(part_name)
            .collect()
    }

    /// 3296847 -> "3,296,847".
    fn thousands(n: u64) -> String {
        let digits = n.to_string();
        let mut out = String::with_capacity(digits.len() + digits.len() / 3);
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
        out
    }

    /// "part 2 (motion clip, MP4, optional) is cut short, 1,648,423 of 3,296,847 bytes present":
    /// an interrupted download and a deliberate trim leave different amounts.
    fn cut_short(p: &PartReport) -> String {
        match (&p.locator, p.present_bytes) {
            (Locator::Bytes { length, .. }, Some(have)) => format!(
                "{} is cut short, {} of {} bytes present",
                part_name(p),
                thousands(have),
                thousands(*length)
            ),
            _ => format!("{} is cut short", part_name(p)),
        }
    }

    /// Upper-case the first letter, for a list that starts a sentence.
    fn cap(s: String) -> String {
        let mut c = s.chars();
        match c.next() {
            Some(first) => first.to_uppercase().chain(c).collect(),
            None => s,
        }
    }

    /// "a", "a and b", "a, b and c".
    fn and_list(items: &[String]) -> String {
        match items {
            [] => String::new(),
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        }
    }

    /// The sentence appended to an Invalid rationale when the fallback ran and did
    /// not restore the binding: which part changed, or why it could not be judged.
    fn multi_asset_note(r: &Report) -> String {
        let rule = multi_asset::RULE;
        let matched = parts_where(r, |p| *p == PartResult::Match);
        let still = if matched.is_empty() {
            String::new()
        } else {
            format!(
                " {} still {}.",
                cap(and_list(&matched)),
                if matched.len() == 1 {
                    "matches"
                } else {
                    "match"
                }
            )
        };
        match r.outcome {
            Outcome::Mismatch => {
                let changed = parts_where(r, |p| *p == PartResult::Mismatch);
                let cut: Vec<String> = r
                    .parts
                    .iter()
                    .filter(|p| p.result == PartResult::Truncated)
                    .map(cut_short)
                    .collect();
                let mut what = Vec::new();
                if !changed.is_empty() {
                    what.push(format!(
                        "{} {} from what was signed",
                        and_list(&changed),
                        if changed.len() == 1 {
                            "differs"
                        } else {
                            "differ"
                        }
                    ));
                }
                if !cut.is_empty() {
                    what.push(and_list(&cut));
                }
                format!(
                    " The manifest's per-part hashes ({rule}) locate the change: {}.{still}",
                    what.join("; ")
                )
            }
            Outcome::Malformed | Outcome::MissingPart => format!(
                " Its multi-asset hash ({rule}) does not validate either: {} ({}).{still}",
                r.reason.as_deref().unwrap_or("no reason recorded"),
                r.code.unwrap_or("-")
            ),
            Outcome::NotEvaluated => format!(
                " The manifest also carries a multi-asset hash ({rule}), which was not \
                 evaluated: {}.",
                r.reason.as_deref().unwrap_or("no reason recorded")
            ),
            Outcome::Match => String::new(),
        }
    }

    /// One typed c2pa anchor per resolved bundle, kind preserved.
    ///
    /// c2pa 0.91 reads only `trust.anchors`. The older `trust.trust_anchors` /
    /// `trust.user_anchors` keys are still accepted by `Settings::with_value` but
    /// are no longer read by validation, so setting them silently trusts nothing
    /// (DIFFERENTIAL.md D-011). Typed structs make a future rename a compile error.
    pub(super) fn anchors(resolved: &ResolvedTrust) -> Vec<TrustAnchor> {
        resolved
            .anchors
            .iter()
            .map(|a| TrustAnchor {
                trust_anchors: a.pem.clone(),
                trust_uri: Some(a.uri.clone()),
                trust_kind: match a.kind {
                    AnchorKind::Manifest => TrustListKind::Manifest,
                    AnchorKind::Tsa => TrustListKind::TSA,
                },
                ..Default::default()
            })
            .collect()
    }

    /// Build the validation context. If the c2pa settings API moves again, this and
    /// [`anchors`] are the only functions to touch.
    ///
    /// Anchors go in first; the `with_value` calls after them re-validate the whole
    /// settings tree, so a bundle c2pa cannot parse is an error here rather than an
    /// anchor it quietly skips.
    ///
    /// `fetch` is `false` for every first read: a file must not be able to make the
    /// tool call out to a URL it carries unless the operator asked for it, and even
    /// then only after [`super::check_remote_url`]. OCSP is never consulted.
    pub(super) fn context(resolved: &ResolvedTrust, fetch: bool) -> Result<c2pa::Context, String> {
        let mut settings = c2pa::Settings::new();
        let anchors = anchors(resolved);
        if !anchors.is_empty() {
            settings.trust.anchors = Some(anchors);
        }
        // Accepted signing-certificate EKUs. c2pa 0.91 clears its built-in list
        // when the Store is built and re-adds only this (D-011).
        settings.trust.trust_config = Some(resolved.eku_config.clone());
        let settings = settings
            .with_value("verify.remote_manifest_fetch", fetch)
            .map_err(|e| e.to_string())?
            .with_value("verify.ocsp_fetch", false)
            .map_err(|e| e.to_string())?;
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
                );
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
                );
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
                        );
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
                        );
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
                        );
                    }
                }
            }
            Err(c2pa::Error::JumbfNotFound) => {
                return mk(
                    Status::Absent,
                    "No C2PA manifest. Most files have none; this says nothing about origin."
                        .into(),
                    serde_json::Value::Null,
                );
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
                );
            }
            Err(e) => {
                return mk(
                    Status::Inconclusive,
                    format!("A manifest container is present but could not be read: {e}"),
                    serde_json::json!({ "error": e.to_string() }),
                );
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
        let lineage = crate::lineage::walk(&js);
        let ingredient_failures = crate::lineage::ingredient_failures(&js);
        let manifests = js
            .get("manifests")
            .and_then(|m| m.as_object())
            .map(|m| m.len())
            .unwrap_or(0);
        let validation_status = js
            .get("validation_status")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        // The C2PA signals rubric over the whole store, from c2pa's own crJSON export.
        // Details only: a signal classifies what a signer declared and never changes
        // the verdict (see `signals`).
        let signals = match reader.to_crjson_value() {
            Ok(crjson) => crate::signals::details(&crjson),
            Err(e) => serde_json::json!({
                "error": format!("c2pa could not export the store as crJSON: {e}"),
            }),
        };

        // C2PA 2.4 §15.12.4, which c2pa 0.91 does not apply (D-014). Only an Invalid
        // manifest can have a failed hard binding to fall back from.
        let multi_part = if state == c2pa::ValidationState::Invalid {
            multi_asset_fallback(&reader, active_label.as_deref(), &a.bytes)
        } else {
            None
        };

        // Time-stamp codes of the active manifest, from all three buckets of
        // validation_results: c2pa files `timeStamp.validated` / `.trusted` as success
        // and `timeStamp.untrusted` as informational, so validation_status (failures
        // only) never holds them. `None` when c2pa reported no results: then the
        // status is unknown and nothing is claimed about it.
        let time_stamp_codes: Option<Vec<String>> = reader
            .validation_results()
            .and_then(|r| r.active_manifest())
            .map(|s| {
                let mut codes: Vec<String> = s
                    .success()
                    .iter()
                    .chain(s.informational())
                    .chain(s.failure())
                    .map(|v| v.code())
                    .filter(|c| c.starts_with("timeStamp."))
                    .map(str::to_string)
                    .collect();
                codes.sort_unstable();
                codes.dedup();
                codes
            });
        let time_stamp: Option<TimeStampStatus> = time_stamp_codes
            .as_ref()
            .map(|c| TimeStampStatus::from_codes(c.iter().map(String::as_str)));

        // The signer's certificate, leaf first in the chain c2pa read. Its serial must
        // match the one c2pa reports for the signature, so a chain in another order
        // cannot report another certificate's window.
        let c2pa_serial = active
            .get("signature_info")
            .and_then(|s| s.get("cert_serial_number"))
            .and_then(|v| v.as_str());
        let signing_cert: Result<Option<SigningCert>, String> =
            match reader.active_manifest().and_then(|m| m.signature_info()) {
                None => Ok(None),
                Some(si) => {
                    shelf_life::leaf_from_pem(si.cert_chain()).and_then(|c| match c2pa_serial {
                        Some(d) if d != c.serial_decimal => Err(format!(
                            "the first certificate in the chain (serial {}) is not the \
                             signer c2pa reports (serial {d})",
                            c.serial
                        )),
                        _ => Ok(Some(c)),
                    })
                }
            };
        let (signing_cert, signing_cert_error) = match signing_cert {
            Ok(c) => (c, None),
            Err(e) => (None, Some(e)),
        };
        let shelf_note = match (&signing_cert, time_stamp) {
            (Some(c), Some(ts)) => {
                shelf_life::validity_note(c, ts, signed_at.as_deref()).unwrap_or_default()
            }
            _ => String::new(),
        };

        // The rationale names the generator without its build number; the full
        // `name version` stays in details.claim_generator (compared with c2patool, D-003).
        let generator_name = active
            .get("claim_generator")
            .and_then(|v| v.as_str())
            .or_else(|| {
                active
                    .pointer("/claim_generator_info/0/name")
                    .and_then(|v| v.as_str())
            })
            .map(str::to_string);
        let who = match (&generator_name, &issuer) {
            (Some(g), Some(i)) => format!("from {g}, signed by {i}"),
            (Some(g), None) => format!("from {g}, signer unknown"),
            (None, Some(i)) => format!("signed by {i}"),
            (None, None) => "from an unknown claim generator".into(),
        };
        let ai_note = crate::lineage::describe(&source_type, &lineage, ingredient_failures);

        let (status, rationale) = match state {
            c2pa::ValidationState::Trusted => (
                Status::Present,
                format!(
                    "Valid C2PA manifest {who}; signer chains to a trusted anchor in \
                     {lists} and the content hash matches, so the file is unchanged \
                     since signing.{shelf_note}{ai_note}",
                    lists = resolved.manifest_lists_label()
                ),
            ),
            c2pa::ValidationState::Valid => (
                Status::Inconclusive,
                format!(
                    "C2PA manifest {who} is cryptographically valid and the content is \
                     unchanged since signing, but the signer does not chain to any anchor \
                     in {lists} (details.trust lists them).{shelf_note}{ai_note}",
                    lists = resolved.manifest_lists_label()
                ),
            ),
            c2pa::ValidationState::Invalid
                if multi_part
                    .as_ref()
                    .is_some_and(|r| r.outcome == Outcome::Match) =>
            {
                // The whole-file hash failed, but every part still in the file matches
                // its own signed hash and only optional parts are gone: per §15.12.4
                // the content binding holds. Decide trust as for an intact file.
                let r = multi_part.as_ref().expect("guarded by is_some_and above");
                let matched = parts_where(r, |p| *p == PartResult::Match);
                let gone = parts_where(r, |p| *p == PartResult::Absent);
                let removed = if gone.is_empty() {
                    String::new()
                } else {
                    format!(
                        " {} {} no longer in the file, which the signer allowed by marking {} \
                         optional.",
                        cap(and_list(&gone)),
                        if gone.len() == 1 { "is" } else { "are" },
                        if gone.len() == 1 { "it" } else { "them" }
                    )
                };
                let parts_sentence = format!(
                    "The whole-file content hash no longer matches, but the manifest also signs \
                     each part of this multi-part file on its own ({rule}), and {} still {}.{removed} \
                     The content still in the file is unchanged since signing. (c2pa-rs alone \
                     reports this manifest Invalid: it does not apply that rule.)",
                    and_list(&matched),
                    if matched.len() == 1 {
                        "matches"
                    } else {
                        "match"
                    },
                    rule = multi_asset::RULE,
                );
                let untrusted = validation_status.as_array().is_some_and(|a| {
                    a.iter().any(|x| {
                        x.get("code").and_then(|c| c.as_str())
                            == Some("signingCredential.untrusted")
                    })
                });
                if untrusted {
                    (
                        Status::Inconclusive,
                        format!(
                            "C2PA manifest {who} is cryptographically valid, but the signer does \
                             not chain to any anchor in {lists} (details.trust lists them). \
                             {parts_sentence}{shelf_note}{ai_note}",
                            lists = resolved.manifest_lists_label()
                        ),
                    )
                } else {
                    (
                        Status::Present,
                        format!(
                            "Valid C2PA manifest {who}; signer chains to a trusted anchor in \
                             {lists}. {parts_sentence}{shelf_note}{ai_note}",
                            lists = resolved.manifest_lists_label()
                        ),
                    )
                }
            }
            c2pa::ValidationState::Invalid => {
                let ma_note = multi_part
                    .as_ref()
                    .map(multi_asset_note)
                    .unwrap_or_default();
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
                if expired && !broken {
                    // Why nothing placed the signature inside the window comes from the
                    // time-stamp codes, not from a `c2pa.time-stamp` assertion: that
                    // assertion time-stamps other manifests (D-010 addendum).
                    let phrase = shelf_life::expired_phrase(signing_cert.as_ref(), time_stamp);
                    let alternative = if time_stamp == Some(TimeStampStatus::Untrusted) {
                        shelf_life::tsa_alternative(signed_at.as_deref())
                    } else {
                        String::new()
                    };
                    (
                        Status::Inconclusive,
                        format!(
                            "C2PA manifest {who} is present and its content hash still matches, \
                             but the signing certificate {phrase}. The signature cannot be \
                             placed inside the certificate's validity window, so it no longer \
                             validates; it may have validated when the file was made. Whether a \
                             reader accepts it now depends on the date of checking.{alternative} \
                             See validation_status.{ai_note}"
                        ),
                    )
                } else if broken {
                    (
                        Status::Inconclusive,
                        format!(
                            "C2PA manifest {who} is present but does not validate: the signature \
                             fails or the content was modified after signing.{ma_note} See \
                             validation_status.{ai_note}"
                        ),
                    )
                } else {
                    // No hash mismatch and no claim-signature failure: the manifest
                    // fails a conformance rule (a certificate profile, an assertion
                    // the validator cannot decode). Saying "modified after signing"
                    // here would accuse the file of something nothing showed (D-011).
                    let failing: Vec<&str> = codes
                        .iter()
                        .copied()
                        .filter(|c| *c != "signingCredential.untrusted")
                        .collect();
                    (
                        Status::Inconclusive,
                        format!(
                            "C2PA manifest {who} is present and no hash mismatch or claim-signature \
                             failure was reported, but it does not validate: it fails {}. That is \
                             a conformance failure of the manifest or its signing certificate, \
                             not evidence that the content changed after signing. See \
                             validation_status.{ai_note}",
                            if failing.is_empty() {
                                "a check that reported no code".to_string()
                            } else {
                                failing.join(", ")
                            }
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
            "declares_ai": source_type.declares_ai || lineage.declares_ai(),
            "declares_ai_active": source_type.declares_ai,
            "generative_depth": lineage.generative_depth(),
            "lineage_digital_source_type": lineage.parent_codes(),
            "digital_source_type": source_type.codes,
            "digital_source_type_unknown": source_type.unknown,
            "digital_source_type_hits": source_type.hits,
            "lineage": lineage,
            "signals": signals,
            "source_type_note": ai_note.clone(),
            "vocabulary_version": halftone_core::dst::VOCABULARY_VERSION,
            "trust": resolved.details(),
            "signing_cert": signing_cert,
            "time_stamp": time_stamp.map(|ts| serde_json::json!({
                "status": ts.as_str(),
                "codes": time_stamp_codes,
            })),
        });
        if let Some(e) = signing_cert_error {
            details["signing_cert_error"] = serde_json::Value::String(e);
        }
        // Only present when the fallback applied, so other files' details are unchanged.
        if let Some(r) = &multi_part {
            details["multi_asset"] = serde_json::to_value(r).unwrap_or(serde_json::Value::Null);
        }
        // Only present when the manifest came over the network, so an embedded
        // manifest's details are unchanged from earlier versions.
        if let Some((url, at)) = fetched_from {
            details["fetched_from"] = serde_json::Value::String(url);
            details["fetched_at"] = serde_json::Value::String(at);
        }
        mk(status, rationale, details)
    }
}

#[cfg(all(test, feature = "c2pa"))]
mod c2pa_settings_tests {
    use super::trust::{AnchorKind, InternalList, TrustConfig};
    use c2pa::settings::TrustListKind;

    fn vendored() -> super::trust::ResolvedTrust {
        TrustConfig {
            internal: InternalList::Vendored,
            ..Default::default()
        }
        .resolve()
        .unwrap()
    }

    #[test]
    fn anchors_keep_kind_and_uri() {
        let r = vendored();
        let a = super::imp::anchors(&r);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].trust_kind, TrustListKind::Manifest);
        assert_eq!(a[1].trust_kind, TrustListKind::TSA);
        assert_eq!(a[0].trust_uri.as_deref(), Some(r.anchors[0].uri.as_str()));
        assert_eq!(r.anchors[0].kind, AnchorKind::Manifest);
    }

    /// The D-011 regression: anchors must land where c2pa validation reads them.
    #[test]
    fn context_carries_the_anchors() {
        let ctx = super::imp::context(&vendored(), false).unwrap();
        let got = ctx
            .settings()
            .trust
            .anchors
            .as_ref()
            .expect("trust.anchors unset");
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|a| !a.trust_anchors.is_empty()));
    }

    /// D-011: without trust_config, c2pa 0.91 accepts no vendor EKU.
    #[test]
    fn context_carries_the_eku_policy() {
        let ctx = super::imp::context(&vendored(), false).unwrap();
        let cfg = ctx
            .settings()
            .trust
            .trust_config
            .as_deref()
            .expect("trust_config unset");
        assert!(cfg.lines().any(|l| l == "1.3.6.1.4.1.311.76.59.1.9"));
        assert_eq!(cfg, super::trust::VENDORED_EKU_CONFIG);
    }

    #[test]
    fn disabled_trust_sets_no_anchors() {
        let r = TrustConfig {
            internal: InternalList::Disabled,
            ..Default::default()
        }
        .resolve()
        .unwrap();
        let ctx = super::imp::context(&r, false).unwrap();
        assert!(ctx.settings().trust.anchors.is_none());
        // The EKU policy is independent of which lists are trusted.
        assert!(ctx.settings().trust.trust_config.is_some());
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
