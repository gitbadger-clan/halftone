//! Which certificates the manifest layer trusts, and where they came from.
//!
//! Two independent inputs, mapped onto the `c2pa` crate's two settings:
//!
//! | Input | Setting | Who controls it |
//! |---|---|---|
//! | **internal** list — the official C2PA Trust List (+ TSA list) | `trust.trust_anchors` | the C2PA; refreshed by `halftone packs update`, vendored snapshot as fallback |
//! | **custom** anchors — an operator's own CAs | `trust.user_anchors` | the operator (`--trust-anchors`, repeatable) |
//!
//! The internal list can also be pointed at a file (an organisation that curates
//! its own policy) or disabled (custom anchors only, for closed ecosystems).
//! Whatever is chosen, [`ResolvedTrust::details`] records the origin and hash of
//! every bundle so a `Present` verdict states which list it was judged against.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Vendored snapshot of the official lists. Refresh with
/// `scripts/refresh-trust-snapshot.fish`; `trust/SNAPSHOT` records the source
/// commit and date.
pub const VENDORED_TRUST_LIST: &str = include_str!("../trust/C2PA-TRUST-LIST.pem");
/// Vendored C2PA time-stamp-authority trust list.
pub const VENDORED_TSA_TRUST_LIST: &str = include_str!("../trust/C2PA-TSA-TRUST-LIST.pem");
/// First line names the upstream commit and fetch date of the vendored lists.
pub const VENDORED_SNAPSHOT: &str = include_str!("../trust/SNAPSHOT");

/// Source of the internal (official) list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InternalList {
    /// Installed copy from `packs update` if present, else the vendored snapshot.
    #[default]
    Auto,
    /// Only the snapshot compiled into this binary.
    Vendored,
    /// A PEM bundle the operator maintains instead of the official list.
    File(PathBuf),
    /// No internal list; only custom anchors are trusted.
    Disabled,
}

/// Trust configuration for [`crate::C2paSource`].
#[derive(Debug, Clone, Default)]
pub struct TrustConfig {
    /// Where the official list comes from.
    pub internal: InternalList,
    /// Extra PEM bundles added as user anchors.
    pub custom_anchors: Vec<PathBuf>,
    /// Data directory holding `trust/c2pa/`; `None` → resolve via `halftone-packs`.
    pub home: Option<PathBuf>,
}

/// One PEM bundle that went into the policy, with provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleInfo {
    /// `vendored`, `installed`, `file`, `custom`.
    pub origin: String,
    /// Path for installed/file/custom; snapshot id for vendored.
    pub source: String,
    /// SHA-256 of the bundle bytes as loaded.
    pub sha256: String,
    /// Number of `BEGIN CERTIFICATE` blocks.
    pub certificates: usize,
    /// From `meta.json` for installed lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<String>,
    /// Bundle version, e.g. `2026-09-01 (mirror)`, for installed lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Everything the `c2pa` settings need, plus what to put in the verdict.
#[derive(Debug, Clone)]
pub struct ResolvedTrust {
    /// Concatenated PEM for `trust.trust_anchors`; empty when disabled.
    pub internal_pem: String,
    /// Concatenated PEM for `trust.user_anchors`; empty when none.
    pub custom_pem: String,
    /// Provenance of each internal bundle.
    pub internal: Vec<BundleInfo>,
    /// Provenance of each custom bundle.
    pub custom: Vec<BundleInfo>,
}

impl ResolvedTrust {
    /// Block for `Evidence.details.trust`.
    pub fn details(&self) -> serde_json::Value {
        serde_json::json!({
            "internal": self.internal,
            "custom": self.custom,
            "anchors_total": self.internal.iter().chain(&self.custom).map(|b| b.certificates).sum::<usize>(),
        })
    }

    /// True when nothing at all is trusted — every signer will be untrusted.
    pub fn is_empty(&self) -> bool {
        self.internal_pem.trim().is_empty() && self.custom_pem.trim().is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledMeta {
    version: String,
    fetched_at: String,
    origin: String,
    source_url: String,
}

impl TrustConfig {
    /// Load every configured bundle from disk (or the binary) and build the PEM strings.
    pub fn resolve(&self) -> Result<ResolvedTrust, String> {
        let mut internal = Vec::new();
        let mut internal_pem = String::new();

        match &self.internal {
            InternalList::Disabled => {}
            InternalList::Vendored => push_vendored(&mut internal, &mut internal_pem),
            InternalList::File(p) => {
                let pem = read_pem(p)?;
                internal.push(info("file", p.display().to_string(), &pem, None));
                internal_pem.push_str(&pem);
            }
            InternalList::Auto => {
                let dir = self.trust_dir()?;
                if !push_installed(&dir, &mut internal, &mut internal_pem)? {
                    push_vendored(&mut internal, &mut internal_pem);
                }
            }
        }

        let mut custom = Vec::new();
        let mut custom_pem = String::new();
        for p in &self.custom_anchors {
            let pem = read_pem(p)?;
            custom.push(info("custom", p.display().to_string(), &pem, None));
            custom_pem.push_str(&pem);
            custom_pem.push('\n');
        }

        Ok(ResolvedTrust {
            internal_pem,
            custom_pem,
            internal,
            custom,
        })
    }

    fn trust_dir(&self) -> Result<PathBuf, String> {
        match &self.home {
            Some(h) => Ok(h.join("trust").join("c2pa")),
            None => halftone_packs::store::Store::resolve().map(|s| s.trust_dir()),
        }
    }
}

fn push_vendored(out: &mut Vec<BundleInfo>, pem: &mut String) {
    let snap = VENDORED_SNAPSHOT
        .lines()
        .next()
        .unwrap_or("unknown")
        .to_string();
    for (name, body) in [
        ("C2PA-TRUST-LIST.pem", VENDORED_TRUST_LIST),
        ("C2PA-TSA-TRUST-LIST.pem", VENDORED_TSA_TRUST_LIST),
    ] {
        out.push(info("vendored", format!("{name}@{snap}"), body, None));
        pem.push_str(body);
        pem.push('\n');
    }
}

/// Load `<dir>/*.pem` listed in `meta.json`. Returns `Ok(false)` if nothing is
/// installed, so the caller can fall back to the vendored snapshot.
fn push_installed(dir: &Path, out: &mut Vec<BundleInfo>, pem: &mut String) -> Result<bool, String> {
    let meta_path = dir.join("meta.json");
    if !meta_path.exists() {
        return Ok(false);
    }
    let meta: InstalledMeta = serde_json::from_slice(
        &std::fs::read(&meta_path).map_err(|e| format!("{}: {e}", meta_path.display()))?,
    )
    .map_err(|e| format!("{}: {e}", meta_path.display()))?;
    let mut any = false;
    for name in ["C2PA-TRUST-LIST.pem", "C2PA-TSA-TRUST-LIST.pem"] {
        let p = dir.join(name);
        if !p.exists() {
            continue;
        }
        let body = read_pem(&p)?;
        let mut b = info(
            "installed",
            p.display().to_string(),
            &body,
            Some(meta.fetched_at.clone()),
        );
        b.version = Some(format!("{} ({})", meta.version, meta.origin));
        out.push(b);
        pem.push_str(&body);
        pem.push('\n');
        any = true;
    }
    Ok(any)
}

fn read_pem(p: &Path) -> Result<String, String> {
    let s = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if count_certs(&s) == 0 {
        return Err(format!("{}: no PEM certificates found", p.display()));
    }
    Ok(s)
}

fn count_certs(pem: &str) -> usize {
    pem.matches("-----BEGIN CERTIFICATE-----").count()
}

fn info(origin: &str, source: String, pem: &str, fetched_at: Option<String>) -> BundleInfo {
    BundleInfo {
        origin: origin.into(),
        source,
        sha256: hex::encode(Sha256::digest(pem.as_bytes())),
        certificates: count_certs(pem),
        fetched_at,
        version: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendored_snapshot_is_non_empty() {
        assert!(
            count_certs(VENDORED_TRUST_LIST) > 0,
            "trust/C2PA-TRUST-LIST.pem is empty"
        );
        assert!(count_certs(VENDORED_TSA_TRUST_LIST) > 0);
        assert!(!VENDORED_SNAPSHOT.trim().is_empty());
    }

    #[test]
    fn disabled_with_no_custom_is_empty() {
        let r = TrustConfig {
            internal: InternalList::Disabled,
            ..Default::default()
        }
        .resolve()
        .unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn auto_falls_back_to_vendored_when_nothing_installed() {
        let empty = std::env::temp_dir().join(format!("halftone-trust-{}", std::process::id()));
        let r = TrustConfig {
            internal: InternalList::Auto,
            home: Some(empty),
            ..Default::default()
        }
        .resolve()
        .unwrap();
        assert!(r.internal.iter().all(|b| b.origin == "vendored"));
        assert!(!r.is_empty());
    }

    #[test]
    fn details_reports_every_bundle() {
        let r = TrustConfig {
            internal: InternalList::Vendored,
            ..Default::default()
        }
        .resolve()
        .unwrap();
        let d = r.details();
        assert_eq!(d["internal"].as_array().unwrap().len(), 2);
        assert!(d["anchors_total"].as_u64().unwrap() > 0);
    }
}
