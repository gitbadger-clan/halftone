//! `index.json`: the signed catalogue that `halftone packs update` reads.
//!
//! One index lists two kinds of artifact. **Packs** are `.tar.zst` archives with
//! a `pack.json` inside (models, fingerprint DBs). **Trust lists** are plain
//! files (PEM bundles) mirrored from an upstream authority; the index pins their
//! hashes so a mirror cannot substitute a different list.
//!
//! The index is signed as raw bytes with Ed25519 (detached `index.json.sig`,
//! hex). Which publisher keys are accepted is decided by the caller: the
//! official keys are compiled in, and `--publisher-key` adds more for third-party
//! pack sources. Signature over bytes, not over parsed JSON, so there is no
//! canonicalisation step to get wrong.

use crate::manifest::Tier;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

/// Current index schema. Bump on incompatible change; readers refuse unknown.
pub const INDEX_SCHEMA: u32 = 1;

/// Official publisher verifying keys (hex, 32 bytes each). Empty until the key
/// ceremony; until then `packs update` needs `--publisher-key` or `--upstream`.
pub const OFFICIAL_PUBLISHER_KEYS_HEX: &[&str] = &[];

/// Default index location.
pub const DEFAULT_INDEX_URL: &str = "https://packs.halftone.gitbadger.com/index.json";

/// Parsed `index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    /// Must equal [`INDEX_SCHEMA`].
    pub schema: u32,
    /// RFC 3339. Monotonic per publisher; used for rollback protection.
    pub generated_at: String,
    /// Oldest `halftone` that can use this index.
    pub min_tool_version: String,
    /// Everything the publisher offers.
    pub artifacts: Vec<Artifact>,
}

/// One downloadable thing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Artifact {
    /// Signed `.tar.zst` pack.
    Pack(PackArtifact),
    /// Mirrored plain files with pinned hashes.
    TrustList(TrustListArtifact),
}

impl Artifact {
    /// Artifact name.
    pub fn name(&self) -> &str {
        match self {
            Artifact::Pack(p) => &p.name,
            Artifact::TrustList(t) => &t.name,
        }
    }
    /// Artifact version string.
    pub fn version(&self) -> &str {
        match self {
            Artifact::Pack(p) => &p.version,
            Artifact::TrustList(t) => &t.version,
        }
    }
}

/// A `.tar.zst` pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackArtifact {
    /// Pack name (matches `pack.json`).
    pub name: String,
    /// Pack version (matches `pack.json`).
    pub version: String,
    /// Distribution tier; `pro` needs a license before install.
    pub tier: Tier,
    /// Download URL.
    pub url: String,
    /// SHA-256 hex of the archive.
    pub sha256: String,
    /// Archive size in bytes; used as a pre-download sanity limit.
    pub size: u64,
    /// Detached Ed25519 signature (hex) over the archive bytes, by a publisher key.
    pub sig: String,
}

/// A bundle of plain files mirrored from an upstream authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustListArtifact {
    /// e.g. `c2pa-trust-list`.
    pub name: String,
    /// Publisher's version string, e.g. the upstream commit date `2026-09-01`.
    pub version: String,
    /// Files in the bundle.
    pub files: Vec<TrustFile>,
    /// Human-readable pointer to the authority the mirror copies from.
    pub upstream: String,
}

/// One file of a trust-list bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustFile {
    /// File name as installed (no directories).
    pub path: String,
    /// Mirror URL.
    pub url: String,
    /// Direct upstream URL (`--upstream` fetches this instead, TLS-only, no pin).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_url: Option<String>,
    /// SHA-256 hex; the mirror copy must match.
    pub sha256: String,
}

/// Errors from parsing or verifying an index.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    /// `index.json` did not parse.
    #[error("index is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// Index written for a newer or older reader.
    #[error("unsupported index schema {0} (this build reads {INDEX_SCHEMA})")]
    Schema(u32),
    /// No publisher key to verify against.
    #[error("no publisher keys configured; pass --publisher-key or use --upstream")]
    NoKeys,
    /// A key or signature was not valid hex / not the right length.
    #[error("bad publisher key or signature encoding: {0}")]
    Encoding(String),
    /// Signature valid hex but verifies under none of the keys.
    #[error("index signature does not verify against any configured publisher key")]
    BadSignature,
    /// Fetched index predates the one already installed.
    #[error("index generated_at {found} is older than installed {installed}; refusing rollback")]
    Rollback {
        /// `generated_at` of the fetched index.
        found: String,
        /// `generated_at` recorded in `installed.json`.
        installed: String,
    },
    /// A trust-list file name contained a directory component.
    #[error("trust file `{0}` is not a plain file name")]
    BadPath(String),
}

/// Parse hex-encoded verifying keys.
pub fn parse_keys(hex_keys: &[impl AsRef<str>]) -> Result<Vec<VerifyingKey>, IndexError> {
    hex_keys
        .iter()
        .map(|h| {
            let bytes =
                hex::decode(h.as_ref().trim()).map_err(|e| IndexError::Encoding(e.to_string()))?;
            let arr: [u8; 32] = bytes
                .try_into()
                .map_err(|_| IndexError::Encoding("publisher key must be 32 bytes".into()))?;
            VerifyingKey::from_bytes(&arr).map_err(|e| IndexError::Encoding(e.to_string()))
        })
        .collect()
}

/// Verify a detached hex signature over `bytes` with any of `keys`.
pub fn verify_detached(
    bytes: &[u8],
    sig_hex: &str,
    keys: &[VerifyingKey],
) -> Result<(), IndexError> {
    if keys.is_empty() {
        return Err(IndexError::NoKeys);
    }
    let sig_bytes = hex::decode(sig_hex.trim()).map_err(|e| IndexError::Encoding(e.to_string()))?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|e| IndexError::Encoding(e.to_string()))?;
    if keys.iter().any(|k| k.verify(bytes, &sig).is_ok()) {
        Ok(())
    } else {
        Err(IndexError::BadSignature)
    }
}

impl Index {
    /// Verify the signature, then parse and sanity-check.
    ///
    /// `installed_generated_at` enables rollback protection: an index no newer than
    /// the last one we accepted is refused. Equal is allowed (re-running update).
    pub fn verify_and_parse(
        bytes: &[u8],
        sig_hex: &str,
        keys: &[VerifyingKey],
        installed_generated_at: Option<&str>,
    ) -> Result<Self, IndexError> {
        verify_detached(bytes, sig_hex, keys)?;
        Self::parse_unverified(bytes, installed_generated_at)
    }

    /// Parse without a signature check. Only for `--from <dir>` bundles that were
    /// verified when exported, or tests. Still enforces schema and rollback.
    pub fn parse_unverified(
        bytes: &[u8],
        installed_generated_at: Option<&str>,
    ) -> Result<Self, IndexError> {
        let idx: Index = serde_json::from_slice(bytes)?;
        if idx.schema != INDEX_SCHEMA {
            return Err(IndexError::Schema(idx.schema));
        }
        if let Some(inst) = installed_generated_at {
            // Same format, same zone → lexicographic order is chronological.
            if idx.generated_at.as_str() < inst {
                return Err(IndexError::Rollback {
                    found: idx.generated_at.clone(),
                    installed: inst.to_string(),
                });
            }
        }
        for a in &idx.artifacts {
            if let Artifact::TrustList(t) = a {
                for f in &t.files {
                    if f.path.contains(['/', '\\']) || f.path.starts_with('.') || f.path.is_empty()
                    {
                        return Err(IndexError::BadPath(f.path.clone()));
                    }
                }
            }
        }
        Ok(idx)
    }

    /// Pack artifacts only.
    pub fn packs(&self) -> impl Iterator<Item = &PackArtifact> {
        self.artifacts.iter().filter_map(|a| match a {
            Artifact::Pack(p) => Some(p),
            _ => None,
        })
    }
    /// Trust-list artifacts only.
    pub fn trust_lists(&self) -> impl Iterator<Item = &TrustListArtifact> {
        self.artifacts.iter().filter_map(|a| match a {
            Artifact::TrustList(t) => Some(t),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn sample() -> String {
        serde_json::json!({
            "schema": 1,
            "generated_at": "2026-09-04T00:00:00Z",
            "min_tool_version": "0.1.0",
            "artifacts": [{
                "kind": "trust-list",
                "name": "c2pa-trust-list",
                "version": "2026-09-01",
                "upstream": "https://github.com/c2pa-org/conformance-public/tree/main/trust-list",
                "files": [{
                    "path": "C2PA-TRUST-LIST.pem",
                    "url": "https://packs.example/trust/C2PA-TRUST-LIST.pem",
                    "sha256": "00"
                }]
            }]
        })
        .to_string()
    }

    #[test]
    fn signed_index_roundtrip() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let vk = sk.verifying_key();
        let bytes = sample();
        let sig = hex::encode(sk.sign(bytes.as_bytes()).to_bytes());
        let idx = Index::verify_and_parse(bytes.as_bytes(), &sig, &[vk], None).unwrap();
        assert_eq!(idx.trust_lists().count(), 1);
        assert_eq!(idx.packs().count(), 0);
    }

    #[test]
    fn wrong_key_rejected() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let other = SigningKey::from_bytes(&[8u8; 32]).verifying_key();
        let bytes = sample();
        let sig = hex::encode(sk.sign(bytes.as_bytes()).to_bytes());
        assert!(matches!(
            Index::verify_and_parse(bytes.as_bytes(), &sig, &[other], None),
            Err(IndexError::BadSignature)
        ));
    }

    #[test]
    fn rollback_refused() {
        let r = Index::parse_unverified(sample().as_bytes(), Some("2026-09-05T00:00:00Z"));
        assert!(matches!(r, Err(IndexError::Rollback { .. })));
        assert!(Index::parse_unverified(sample().as_bytes(), Some("2026-09-04T00:00:00Z")).is_ok());
    }

    #[test]
    fn trust_file_path_must_be_plain() {
        let bad = sample().replace("C2PA-TRUST-LIST.pem", "../etc/passwd");
        assert!(matches!(
            Index::parse_unverified(bad.as_bytes(), None),
            Err(IndexError::BadPath(_))
        ));
    }
}
