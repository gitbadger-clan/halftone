//! Which certificates the manifest layer trusts, and where they came from.
//!
//! Two independent inputs. Each bundle becomes one entry of the `c2pa` crate's
//! `trust.anchors` (0.91+), tagged with what it is trusted for ([`AnchorKind`]):
//!
//! | Input | Bundles → kind | Who controls it |
//! |---|---|---|
//! | **internal** list — the official C2PA Trust List and TSA list | `C2PA-TRUST-LIST.pem` → `Manifest`, `C2PA-TSA-TRUST-LIST.pem` → `Tsa` | the C2PA; refreshed by `halftone packs update`, vendored snapshot as fallback |
//! | **custom** anchors — an operator's own CAs | each `--trust-anchors` bundle → `Manifest` | the operator (repeatable) |
//!
//! The internal list can also be pointed at a file (an organisation that curates
//! its own policy; treated as a `Manifest` list) or disabled (custom anchors only,
//! for closed ecosystems). Whatever is chosen, [`ResolvedTrust::details`] records the
//! origin, kind and hash of every bundle so a `Present` verdict states which list it
//! was judged against.
//!
//! The kinds matter since c2pa 0.91: a signing certificate is only checked against
//! `Manifest` anchors and a time-stamp certificate only against `Tsa` anchors. Before
//! 0.91 both lists went into one bundle (DIFFERENTIAL.md D-011).
//!
//! The extended key usages a signing certificate may carry are a separate policy,
//! passed as `trust.trust_config` from the vendored `trust/C2PA-EKU-CONFIG.cfg`
//! and recorded in `details.trust.eku_config`. It applies whether or not any trust
//! list is enabled: c2pa rejects a signer with no accepted EKU as
//! `signingCredential.invalid`, trusted or not (D-011).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Vendored snapshot of the official lists. Refresh with
/// `scripts/refresh-trust-snapshot.fish`; `trust/SNAPSHOT` records the source
/// commit and date.
pub const VENDORED_TRUST_LIST: &str = include_str!("../trust/C2PA-TRUST-LIST.pem");
/// Vendored C2PA time-stamp-authority trust list.
pub const VENDORED_TSA_TRUST_LIST: &str = include_str!("../trust/C2PA-TSA-TRUST-LIST.pem");
/// Extended key usages accepted on signing certificates, one OID per line.
pub const VENDORED_EKU_CONFIG: &str = include_str!("../trust/C2PA-EKU-CONFIG.cfg");
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

/// What a bundle's certificates are trusted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnchorKind {
    /// Roots for manifest signing certificates.
    #[default]
    Manifest,
    /// Roots for RFC 3161 time-stamp authority certificates.
    Tsa,
}

/// One bundle as the `c2pa` settings take it.
#[derive(Debug, Clone)]
pub struct Anchor {
    /// What the certificates are trusted for.
    pub kind: AnchorKind,
    /// Stable identifier for the list, reported by c2pa as the trust list URI.
    pub uri: String,
    /// The PEM bundle.
    pub pem: String,
}

/// One PEM bundle that went into the policy, with provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleInfo {
    /// What the bundle is trusted for.
    #[serde(default)]
    pub kind: AnchorKind,
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

/// Provenance of the EKU policy, for the verdict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EkuInfo {
    /// `vendored` (the only source today).
    pub origin: String,
    /// File name and snapshot id.
    pub source: String,
    /// SHA-256 of the file as loaded.
    pub sha256: String,
    /// The OIDs c2pa will accept, in file order.
    pub oids: Vec<String>,
}

/// Everything the `c2pa` settings need, plus what to put in the verdict.
#[derive(Debug, Clone)]
pub struct ResolvedTrust {
    /// One entry per bundle, internal first, for `trust.anchors`.
    pub anchors: Vec<Anchor>,
    /// Text for `trust.trust_config`: accepted signing-certificate EKUs.
    pub eku_config: String,
    /// Provenance of `eku_config`.
    pub eku: EkuInfo,
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
            "eku_config": self.eku,
        })
    }

    /// True when no signing certificate can be trusted: there is no `Manifest`
    /// anchor (a TSA list alone trusts time-stamps, not signers).
    pub fn is_empty(&self) -> bool {
        !self.anchors.iter().any(|a| a.kind == AnchorKind::Manifest)
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
        let mut anchors = Vec::new();

        match &self.internal {
            InternalList::Disabled => {}
            InternalList::Vendored => push_vendored(&mut internal, &mut anchors),
            InternalList::File(p) => {
                let pem = read_pem(p)?;
                let b = info(
                    "file",
                    AnchorKind::Manifest,
                    p.display().to_string(),
                    &pem,
                    None,
                );
                anchors.push(anchor(&b, file_name(p), pem));
                internal.push(b);
            }
            InternalList::Auto => {
                let dir = self.trust_dir()?;
                if !push_installed(&dir, &mut internal, &mut anchors)? {
                    push_vendored(&mut internal, &mut anchors);
                }
            }
        }

        let mut custom = Vec::new();
        for (i, p) in self.custom_anchors.iter().enumerate() {
            let pem = read_pem(p)?;
            let b = info(
                "custom",
                AnchorKind::Manifest,
                p.display().to_string(),
                &pem,
                None,
            );
            // Indexed: two bundles may share a file name.
            anchors.push(anchor(&b, format!("{i}:{}", file_name(p)), pem));
            custom.push(b);
        }

        Ok(ResolvedTrust {
            anchors,
            eku_config: VENDORED_EKU_CONFIG.to_string(),
            eku: eku_info(),
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

/// The two official bundles and what each is trusted for.
const OFFICIAL: [(&str, AnchorKind); 2] = [
    ("C2PA-TRUST-LIST.pem", AnchorKind::Manifest),
    ("C2PA-TSA-TRUST-LIST.pem", AnchorKind::Tsa),
];

fn push_vendored(out: &mut Vec<BundleInfo>, anchors: &mut Vec<Anchor>) {
    let snap = VENDORED_SNAPSHOT
        .lines()
        .next()
        .unwrap_or("unknown")
        .to_string();
    for ((name, kind), body) in OFFICIAL
        .into_iter()
        .zip([VENDORED_TRUST_LIST, VENDORED_TSA_TRUST_LIST])
    {
        let b = info("vendored", kind, format!("{name}@{snap}"), body, None);
        anchors.push(anchor(&b, name.to_string(), body.to_string()));
        out.push(b);
    }
}

/// Load `<dir>/*.pem` listed in `meta.json`. Returns `Ok(false)` if nothing is
/// installed, so the caller can fall back to the vendored snapshot.
fn push_installed(
    dir: &Path,
    out: &mut Vec<BundleInfo>,
    anchors: &mut Vec<Anchor>,
) -> Result<bool, String> {
    let meta_path = dir.join("meta.json");
    if !meta_path.exists() {
        return Ok(false);
    }
    let meta: InstalledMeta = serde_json::from_slice(
        &std::fs::read(&meta_path).map_err(|e| format!("{}: {e}", meta_path.display()))?,
    )
    .map_err(|e| format!("{}: {e}", meta_path.display()))?;
    let mut any = false;
    for (name, kind) in OFFICIAL {
        let p = dir.join(name);
        if !p.exists() {
            continue;
        }
        let body = read_pem(&p)?;
        let mut b = info(
            "installed",
            kind,
            p.display().to_string(),
            &body,
            Some(meta.fetched_at.clone()),
        );
        b.version = Some(format!("{} ({})", meta.version, meta.origin));
        anchors.push(anchor(&b, name.to_string(), body));
        out.push(b);
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

/// Lines c2pa will accept as OIDs; it ignores everything else in the file.
fn eku_oids(cfg: &str) -> Vec<String> {
    cfg.lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && l.split('.')
                    .all(|a| !a.is_empty() && a.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(str::to_string)
        .collect()
}

fn eku_info() -> EkuInfo {
    let snap = VENDORED_SNAPSHOT.lines().next().unwrap_or("unknown");
    EkuInfo {
        origin: "vendored".into(),
        source: format!("C2PA-EKU-CONFIG.cfg@{snap}"),
        sha256: hex::encode(Sha256::digest(VENDORED_EKU_CONFIG.as_bytes())),
        oids: eku_oids(VENDORED_EKU_CONFIG),
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// `halftone:<origin>:<name>` — stable across machines (no home paths), unique per
/// bundle, and enough to match a c2pa trust-list URI back to `details.trust`.
fn anchor(b: &BundleInfo, name: String, pem: String) -> Anchor {
    Anchor {
        kind: b.kind,
        uri: format!("halftone:{}:{name}", b.origin),
        pem,
    }
}

fn info(
    origin: &str,
    kind: AnchorKind,
    source: String,
    pem: &str,
    fetched_at: Option<String>,
) -> BundleInfo {
    BundleInfo {
        kind,
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
    fn vendored_lists_become_one_manifest_and_one_tsa_anchor() {
        let r = TrustConfig {
            internal: InternalList::Vendored,
            ..Default::default()
        }
        .resolve()
        .unwrap();
        let kinds: Vec<AnchorKind> = r.anchors.iter().map(|a| a.kind).collect();
        assert_eq!(kinds, [AnchorKind::Manifest, AnchorKind::Tsa]);
        assert_eq!(r.anchors[0].pem, VENDORED_TRUST_LIST);
        assert_eq!(r.anchors[1].pem, VENDORED_TSA_TRUST_LIST);
        assert_eq!(r.anchors[0].uri, "halftone:vendored:C2PA-TRUST-LIST.pem");
        assert_eq!(
            r.anchors[1].uri,
            "halftone:vendored:C2PA-TSA-TRUST-LIST.pem"
        );
    }

    #[test]
    fn custom_bundles_are_manifest_anchors_with_distinct_uris() {
        let dir = std::env::temp_dir().join(format!("halftone-custom-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        let (pa, pb) = (dir.join("a/ca.pem"), dir.join("b/ca.pem"));
        std::fs::write(&pa, VENDORED_TRUST_LIST).unwrap();
        std::fs::write(&pb, VENDORED_TRUST_LIST).unwrap();
        let r = TrustConfig {
            internal: InternalList::Disabled,
            custom_anchors: vec![pa, pb],
            ..Default::default()
        }
        .resolve()
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(r.anchors.iter().all(|a| a.kind == AnchorKind::Manifest));
        assert_eq!(r.anchors[0].uri, "halftone:custom:0:ca.pem");
        assert_eq!(r.anchors[1].uri, "halftone:custom:1:ca.pem");
        assert!(!r.is_empty());
    }

    #[test]
    fn a_tsa_list_alone_trusts_no_signer() {
        let r = ResolvedTrust {
            anchors: vec![Anchor {
                kind: AnchorKind::Tsa,
                uri: "halftone:test:tsa".into(),
                pem: VENDORED_TSA_TRUST_LIST.into(),
            }],
            eku_config: VENDORED_EKU_CONFIG.into(),
            eku: eku_info(),
            internal: vec![],
            custom: vec![],
        };
        assert!(r.is_empty());
    }

    #[test]
    fn eku_config_is_the_six_c2pa_default_oids() {
        // c2pa-rs valid_eku_oids.cfg, identical in 0.90.20 and 0.91.0 (D-011).
        assert_eq!(
            eku_oids(VENDORED_EKU_CONFIG),
            [
                "1.3.6.1.5.5.7.3.4",
                "1.3.6.1.5.5.7.3.36",
                "1.3.6.1.5.5.7.3.8",
                "1.3.6.1.5.5.7.3.9",
                "1.3.6.1.4.1.311.76.59.1.9",
                "1.3.6.1.4.1.62558.2.1",
            ]
        );
    }

    #[test]
    fn eku_oids_ignores_comments_and_junk() {
        assert_eq!(
            eku_oids("// c\n\n 1.2.3 \n1..2\nx.1\n1.2.\n4.5"),
            ["1.2.3", "4.5"]
        );
    }

    #[test]
    fn eku_policy_applies_with_trust_lists_disabled() {
        let r = TrustConfig {
            internal: InternalList::Disabled,
            ..Default::default()
        }
        .resolve()
        .unwrap();
        assert_eq!(r.eku_config, VENDORED_EKU_CONFIG);
        assert_eq!(
            r.details()["eku_config"]["oids"].as_array().unwrap().len(),
            6
        );
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
        assert_eq!(d["internal"][0]["kind"], "manifest");
        assert_eq!(d["internal"][1]["kind"], "tsa");
        assert!(d["anchors_total"].as_u64().unwrap() > 0);
    }
}
