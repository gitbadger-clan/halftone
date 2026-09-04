//! `halftone packs update` — the one networked operation.
//!
//! Flow: obtain `index.json` + `.sig` (network or a local bundle directory) →
//! verify against publisher keys → refuse rollback → for each artifact newer
//! than what is installed: download to `tmp/`, verify sha256 (+ signature for
//! packs), extract/validate, rename into place, record in `installed.json`.
//!
//! Two escape hatches keep this usable before the key ceremony and on
//! air-gapped hosts:
//! - `upstream: true` fetches the C2PA trust lists straight from the C2PA
//!   conformance repository over TLS with no index at all. Nothing is pinned
//!   beyond TLS, and the recorded origin says so.
//! - `from_dir: Some(dir)` reads `index.json`, `index.json.sig` and every
//!   artifact (by URL basename) from a directory instead of the network. Same
//!   verification path, so a bundle carried in on a USB stick is checked exactly
//!   like a download.

use crate::index::{verify_detached, Artifact, Index, PackArtifact, TrustListArtifact};
use crate::manifest::{PackManifest, Tier};
use crate::store::{now_rfc3339, write_atomic, InstalledPack, InstalledTrust, Store};
use ed25519_dalek::VerifyingKey;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Upstream C2PA trust lists. Used by `upstream` mode and as the fallback when
/// an index does not carry `upstream_url`.
pub const C2PA_UPSTREAM: &[(&str, &str)] = &[
    (
        "C2PA-TRUST-LIST.pem",
        "https://raw.githubusercontent.com/c2pa-org/conformance-public/main/trust-list/C2PA-TRUST-LIST.pem",
    ),
    (
        "C2PA-TSA-TRUST-LIST.pem",
        "https://raw.githubusercontent.com/c2pa-org/conformance-public/main/trust-list/C2PA-TSA-TRUST-LIST.pem",
    ),
];
/// Name under which the C2PA trust lists are recorded in `installed.json`.
pub const C2PA_TRUST_NAME: &str = "c2pa-trust-list";

/// Which artifact kinds an update touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Only {
    /// Trust lists and packs.
    All,
    /// Trust lists only.
    Trust,
    /// Packs only.
    Packs,
}

/// Parameters for [`run`].
#[derive(Debug, Clone)]
pub struct UpdateOptions {
    /// Where `index.json` lives; `.sig` is fetched from `<url>.sig`.
    pub index_url: String,
    /// Keys allowed to sign the index and pack archives.
    pub publisher_keys: Vec<VerifyingKey>,
    /// Restrict to trust lists or packs.
    pub only: Only,
    /// Report without writing.
    pub dry_run: bool,
    /// Fetch trust lists directly from the authority; ignores the index.
    pub upstream: bool,
    /// Read index and artifacts from this directory instead of the network.
    pub from_dir: Option<PathBuf>,
    /// Hard cap on any single download.
    pub max_bytes: u64,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self {
            index_url: crate::index::DEFAULT_INDEX_URL.into(),
            publisher_keys: Vec::new(),
            only: Only::All,
            dry_run: false,
            upstream: false,
            from_dir: None,
            max_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

/// One line of the update report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Written to disk.
    Installed {
        /// Artifact name.
        name: String,
        /// Version now installed.
        version: String,
    },
    /// Dry run: would be written.
    WouldInstall {
        /// Artifact name.
        name: String,
        /// Version that would be installed.
        version: String,
    },
    /// Installed version already matches the index.
    UpToDate {
        /// Artifact name.
        name: String,
        /// Installed version.
        version: String,
    },
    /// Not installed, with a reason (license, kind filter).
    Skipped {
        /// Artifact name.
        name: String,
        /// Why it was skipped.
        reason: String,
    },
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Installed { name, version } => write!(f, "installed   {name} {version}"),
            Action::WouldInstall { name, version } => write!(f, "would install {name} {version}"),
            Action::UpToDate { name, version } => write!(f, "up to date  {name} {version}"),
            Action::Skipped { name, reason } => write!(f, "skipped     {name}: {reason}"),
        }
    }
}

/// Where bytes come from. Network or a local bundle directory; both go through
/// the same verification afterwards.
trait Fetch {
    fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, String>;
}

struct DirFetch(PathBuf);

impl Fetch for DirFetch {
    fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
        let name = url.rsplit('/').next().unwrap_or(url);
        let p = self.0.join(name);
        let meta = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        if meta.len() > max {
            return Err(format!(
                "{}: {} bytes exceeds limit",
                p.display(),
                meta.len()
            ));
        }
        std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))
    }
}

#[cfg(feature = "net")]
struct NetFetch {
    agent: ureq::Agent,
}

#[cfg(feature = "net")]
impl NetFetch {
    fn new() -> Self {
        let cfg = ureq::Agent::config_builder()
            .user_agent(format!("halftone/{}", env!("CARGO_PKG_VERSION")))
            .timeout_global(Some(std::time::Duration::from_secs(600)))
            .build();
        Self {
            agent: cfg.new_agent(),
        }
    }
}

#[cfg(feature = "net")]
impl Fetch for NetFetch {
    fn get(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
        if !url.starts_with("https://") {
            return Err(format!("refusing non-HTTPS URL {url}"));
        }
        let mut resp = self
            .agent
            .get(url)
            .call()
            .map_err(|e| format!("{url}: {e}"))?;
        resp.body_mut()
            .with_config()
            .limit(max)
            .read_to_vec()
            .map_err(|e| format!("{url}: {e}"))
    }
}

fn fetcher(o: &UpdateOptions) -> Result<Box<dyn Fetch>, String> {
    if let Some(d) = &o.from_dir {
        return Ok(Box::new(DirFetch(d.clone())));
    }
    #[cfg(feature = "net")]
    {
        Ok(Box::new(NetFetch::new()))
    }
    #[cfg(not(feature = "net"))]
    {
        Err("this build has no network support; use --from <dir>".into())
    }
}

fn sha256_hex(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

/// Minimal PEM sanity check: at least one certificate block.
fn looks_like_pem_bundle(b: &[u8]) -> bool {
    let s = String::from_utf8_lossy(b);
    s.matches("-----BEGIN CERTIFICATE-----").count() >= 1
        && s.matches("-----END CERTIFICATE-----").count() >= 1
}

/// Run an update. Returns the per-artifact report; the caller prints it.
pub fn run(store: &Store, o: &UpdateOptions) -> Result<Vec<Action>, String> {
    store.ensure_layout().map_err(|e| e.to_string())?;
    let fetch = fetcher(o)?;
    let mut installed = store.load_installed()?;
    let mut report = Vec::new();

    if o.upstream {
        if o.only == Only::Packs {
            return Err("--upstream only applies to trust lists".into());
        }
        report.push(install_trust_upstream(store, &*fetch, o, &mut installed)?);
        if !o.dry_run {
            store.save_installed(&installed)?;
        }
        return Ok(report);
    }

    // Index.
    let index_bytes = fetch.get(&o.index_url, 16 * 1024 * 1024)?;
    let sig_bytes = fetch.get(&format!("{}.sig", o.index_url), 4096)?;
    let sig_hex = String::from_utf8_lossy(&sig_bytes).trim().to_string();
    let index = Index::verify_and_parse(
        &index_bytes,
        &sig_hex,
        &o.publisher_keys,
        installed.index_generated_at.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    if version_lt(env!("CARGO_PKG_VERSION"), &index.min_tool_version) {
        return Err(format!(
            "index requires halftone >= {}; this is {}",
            index.min_tool_version,
            env!("CARGO_PKG_VERSION")
        ));
    }

    for a in &index.artifacts {
        let act = match (a, o.only) {
            (Artifact::TrustList(_), Only::Packs) | (Artifact::Pack(_), Only::Trust) => continue,
            (Artifact::TrustList(t), _) => {
                install_trust_list(store, &*fetch, o, t, &mut installed)?
            }
            (Artifact::Pack(p), _) => install_pack(store, &*fetch, o, p, &mut installed)?,
        };
        report.push(act);
    }

    if !o.dry_run {
        write_atomic(&store.index_path(), &index_bytes, &store.tmp_dir())?;
        write_atomic(
            &store.index_sig_path(),
            sig_hex.as_bytes(),
            &store.tmp_dir(),
        )?;
        installed.index_generated_at = Some(index.generated_at.clone());
        store.save_installed(&installed)?;
    }
    Ok(report)
}

fn install_trust_list(
    store: &Store,
    fetch: &dyn Fetch,
    o: &UpdateOptions,
    t: &TrustListArtifact,
    installed: &mut crate::store::Installed,
) -> Result<Action, String> {
    if let Some(cur) = installed.trust.get(&t.name) {
        if cur.version == t.version && cur.origin == "mirror" {
            return Ok(Action::UpToDate {
                name: t.name.clone(),
                version: t.version.clone(),
            });
        }
    }
    if o.dry_run {
        return Ok(Action::WouldInstall {
            name: t.name.clone(),
            version: t.version.clone(),
        });
    }
    let mut files = BTreeMap::new();
    let mut staged: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for f in &t.files {
        let bytes = fetch.get(&f.url, o.max_bytes)?;
        let got = sha256_hex(&bytes);
        if got != f.sha256.to_lowercase() {
            return Err(format!(
                "{}: sha256 mismatch (index {}, got {got})",
                f.path, f.sha256
            ));
        }
        if !looks_like_pem_bundle(&bytes) {
            return Err(format!("{}: does not contain a PEM certificate", f.path));
        }
        files.insert(f.path.clone(), got);
        staged.push((store.trust_dir().join(&f.path), bytes));
    }
    commit_trust(
        store,
        &staged,
        &t.name,
        &t.version,
        files,
        "mirror",
        &o.index_url,
        installed,
    )?;
    Ok(Action::Installed {
        name: t.name.clone(),
        version: t.version.clone(),
    })
}

fn install_trust_upstream(
    store: &Store,
    fetch: &dyn Fetch,
    o: &UpdateOptions,
    installed: &mut crate::store::Installed,
) -> Result<Action, String> {
    let version = now_rfc3339()[..10].to_string();
    if o.dry_run {
        return Ok(Action::WouldInstall {
            name: C2PA_TRUST_NAME.into(),
            version,
        });
    }
    let mut files = BTreeMap::new();
    let mut staged = Vec::new();
    for (name, url) in C2PA_UPSTREAM {
        let bytes = fetch.get(url, o.max_bytes)?;
        if !looks_like_pem_bundle(&bytes) {
            return Err(format!("{url}: response is not a PEM certificate bundle"));
        }
        files.insert((*name).to_string(), sha256_hex(&bytes));
        staged.push((store.trust_dir().join(name), bytes));
    }
    let src = C2PA_UPSTREAM[0]
        .1
        .rsplit_once('/')
        .map(|(d, _)| d)
        .unwrap_or("")
        .to_string();
    commit_trust(
        store,
        &staged,
        C2PA_TRUST_NAME,
        &version,
        files,
        "upstream",
        &src,
        installed,
    )?;
    Ok(Action::Installed {
        name: C2PA_TRUST_NAME.into(),
        version,
    })
}

#[allow(clippy::too_many_arguments)]
fn commit_trust(
    store: &Store,
    staged: &[(PathBuf, Vec<u8>)],
    name: &str,
    version: &str,
    files: BTreeMap<String, String>,
    origin: &str,
    source_url: &str,
    installed: &mut crate::store::Installed,
) -> Result<(), String> {
    for (dst, bytes) in staged {
        write_atomic(dst, bytes, &store.tmp_dir())?;
    }
    let rec = InstalledTrust {
        version: version.to_string(),
        files,
        fetched_at: now_rfc3339(),
        origin: origin.to_string(),
        source_url: source_url.to_string(),
    };
    // meta.json next to the PEMs so halftone-c2pa can report provenance without
    // reading installed.json.
    let meta = serde_json::to_string_pretty(&rec).map_err(|e| e.to_string())?;
    write_atomic(
        &store.trust_dir().join("meta.json"),
        meta.as_bytes(),
        &store.tmp_dir(),
    )?;
    installed.trust.insert(name.to_string(), rec);
    Ok(())
}

fn install_pack(
    store: &Store,
    fetch: &dyn Fetch,
    o: &UpdateOptions,
    p: &PackArtifact,
    installed: &mut crate::store::Installed,
) -> Result<Action, String> {
    if let Some(cur) = installed.packs.get(&p.name) {
        if cur.version == p.version {
            return Ok(Action::UpToDate {
                name: p.name.clone(),
                version: p.version.clone(),
            });
        }
    }
    if p.tier.requires_key() && !store.license_path().exists() {
        return Ok(Action::Skipped {
            name: p.name.clone(),
            reason: format!(
                "tier {} requires a license key ({})",
                p.tier,
                store.license_path().display()
            ),
        });
    }
    if o.dry_run {
        return Ok(Action::WouldInstall {
            name: p.name.clone(),
            version: p.version.clone(),
        });
    }
    if p.size > o.max_bytes {
        return Err(format!("{}: {} bytes exceeds limit", p.name, p.size));
    }
    let archive = fetch.get(&p.url, o.max_bytes)?;
    let got = sha256_hex(&archive);
    if got != p.sha256.to_lowercase() {
        return Err(format!(
            "{}: sha256 mismatch (index {}, got {got})",
            p.name, p.sha256
        ));
    }
    verify_detached(&archive, &p.sig, &o.publisher_keys).map_err(|e| format!("{}: {e}", p.name))?;

    // Extract into a staging dir, validate pack.json, then rename into place.
    let stage = store
        .tmp_dir()
        .join(format!("{}-{}.{}", p.name, p.version, std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    extract_tar_zst(&archive, &stage)?;
    let manifest_path = stage.join("pack.json");
    let manifest: PackManifest = serde_json::from_slice(
        &std::fs::read(&manifest_path).map_err(|e| format!("pack.json missing: {e}"))?,
    )
    .map_err(|e| format!("pack.json: {e}"))?;
    if manifest.name != p.name || manifest.version != p.version {
        return Err(format!(
            "pack.json says {} {} but index says {} {}",
            manifest.name, manifest.version, p.name, p.version
        ));
    }
    if manifest.tier != p.tier {
        return Err(format!(
            "pack.json tier {} disagrees with index {}",
            manifest.tier, p.tier
        ));
    }
    manifest.validate_license()?;
    for f in &manifest.files {
        let path = safe_join(&stage, &f.path)?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", f.path))?;
        if sha256_hex(&bytes) != f.sha256.to_lowercase() {
            return Err(format!("{}: sha256 mismatch inside pack", f.path));
        }
    }
    let dst = store.pack_dir(&p.name, &p.version);
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(&dst);
    std::fs::rename(&stage, &dst)
        .map_err(|e| format!("{} -> {}: {e}", stage.display(), dst.display()))?;

    installed.packs.insert(
        p.name.clone(),
        InstalledPack {
            version: p.version.clone(),
            archive_sha256: got,
            installed_at: now_rfc3339(),
            tier: match p.tier {
                Tier::Eval => "eval".into(),
                Tier::Pro => "pro".into(),
            },
        },
    );
    Ok(Action::Installed {
        name: p.name.clone(),
        version: p.version.clone(),
    })
}

/// Extract a `.tar.zst` archive into `dst`, refusing absolute paths, `..`
/// components, and links.
fn extract_tar_zst(archive: &[u8], dst: &Path) -> Result<(), String> {
    let dec = zstd::stream::read::Decoder::new(archive).map_err(|e| format!("zstd: {e}"))?;
    let mut tar = tar::Archive::new(dec);
    for entry in tar.entries().map_err(|e| format!("tar: {e}"))? {
        let mut entry = entry.map_err(|e| format!("tar: {e}"))?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            return Err(format!("tar: refusing entry type {kind:?}"));
        }
        let rel = entry.path().map_err(|e| format!("tar: {e}"))?.into_owned();
        let out = safe_join(dst, rel.to_str().ok_or("tar: non-UTF-8 path")?)?;
        if kind.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut f =
                std::fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
            std::io::copy(&mut entry, &mut f).map_err(|e| format!("{}: {e}", out.display()))?;
        }
    }
    Ok(())
}

/// Join a relative path onto `base`, rejecting anything that could escape it.
fn safe_join(base: &Path, rel: &str) -> Result<PathBuf, String> {
    use std::path::Component;
    let p = Path::new(rel);
    let mut out = base.to_path_buf();
    for c in p.components() {
        match c {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            _ => return Err(format!("unsafe path in archive: {rel}")),
        }
    }
    Ok(out)
}

/// `a < b` for dotted numeric versions; non-numeric segments compare as 0.
fn version_lt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split(['.', '-', '+'])
            .map(|x| x.parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(a), parse(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x < y;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_rejects_escape() {
        let base = Path::new("/x");
        assert!(safe_join(base, "../y").is_err());
        assert!(safe_join(base, "/abs").is_err());
        assert_eq!(safe_join(base, "./a/b").unwrap(), PathBuf::from("/x/a/b"));
    }

    #[test]
    fn version_compare() {
        assert!(version_lt("0.1.0", "0.2.0"));
        assert!(!version_lt("0.2.0", "0.1.9"));
        assert!(!version_lt("1.0.0", "1.0.0"));
    }

    #[test]
    fn pem_sniff() {
        assert!(looks_like_pem_bundle(
            b"-----BEGIN CERTIFICATE-----\nAA==\n-----END CERTIFICATE-----\n"
        ));
        assert!(!looks_like_pem_bundle(b"<html>404</html>"));
    }

    #[test]
    fn dir_fetch_uses_basename_and_limit() {
        let dir = std::env::temp_dir().join(format!("halftone-fetch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.pem"), b"hello").unwrap();
        let f = DirFetch(dir.clone());
        assert_eq!(f.get("https://example/x/a.pem", 100).unwrap(), b"hello");
        assert!(f.get("https://example/x/a.pem", 2).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
