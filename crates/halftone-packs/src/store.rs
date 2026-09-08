//! On-disk layout for everything `halftone packs update` installs.
//!
//! ```text
//! <home>/
//!   index.json            last verified index (rollback protection)
//!   index.json.sig
//!   installed.json        what is installed, with hashes and timestamps
//!   packs/<name>/<version>/…      extracted pack contents
//!   trust/c2pa/C2PA-TRUST-LIST.pem
//!   trust/c2pa/C2PA-TSA-TRUST-LIST.pem
//!   trust/c2pa/meta.json          origin, sha256, fetched_at
//!   tmp/                          staging; renamed into place atomically
//! ```
//!
//! `<home>` is `$HALFTONE_HOME` if set, otherwise the platform data directory.
//! Readers (`halftone-c2pa`, `halftone-blind`) only ever read from here; the
//! single writer is the update command.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Resolved data directory.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// `$HALFTONE_HOME`, else the platform default.
    pub fn resolve() -> Result<Self, String> {
        if let Some(p) = std::env::var_os("HALFTONE_HOME") {
            return Ok(Self {
                root: PathBuf::from(p),
            });
        }
        let root =
            platform_data_dir().ok_or("cannot determine a data directory; set HALFTONE_HOME")?;
        Ok(Self {
            root: root.join("halftone"),
        })
    }

    /// Use an explicit directory (tests, `--home`).
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The data directory itself.
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// `packs/`.
    pub fn packs_dir(&self) -> PathBuf {
        self.root.join("packs")
    }
    /// `packs/<name>/<version>/`.
    pub fn pack_dir(&self, name: &str, version: &str) -> PathBuf {
        self.packs_dir().join(name).join(version)
    }
    /// `trust/c2pa/`, where installed C2PA trust lists live.
    pub fn trust_dir(&self) -> PathBuf {
        self.root.join("trust").join("c2pa")
    }
    /// Staging directory for atomic writes.
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }
    /// Last verified `index.json`.
    pub fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }
    /// Signature of the last verified index.
    pub fn index_sig_path(&self) -> PathBuf {
        self.root.join("index.json.sig")
    }
    /// `installed.json`.
    pub fn installed_path(&self) -> PathBuf {
        self.root.join("installed.json")
    }
    /// Offline license key, if any.
    pub fn license_path(&self) -> PathBuf {
        self.root.join("license.json")
    }

    /// Create the directory skeleton.
    pub fn ensure_layout(&self) -> std::io::Result<()> {
        for d in [self.packs_dir(), self.trust_dir(), self.tmp_dir()] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }

    /// Read `installed.json`; absent means nothing installed.
    pub fn load_installed(&self) -> Result<Installed, String> {
        let p = self.installed_path();
        if !p.exists() {
            return Ok(Installed::default());
        }
        let s = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        serde_json::from_str(&s).map_err(|e| format!("{}: {e}", p.display()))
    }

    /// Write `installed.json` atomically.
    pub fn save_installed(&self, inst: &Installed) -> Result<(), String> {
        let json = serde_json::to_string_pretty(inst).map_err(|e| e.to_string())?;
        write_atomic(&self.installed_path(), json.as_bytes(), &self.tmp_dir())
    }
}

/// `installed.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Installed {
    /// Pack name → installed record.
    #[serde(default)]
    pub packs: BTreeMap<String, InstalledPack>,
    /// Trust-list bundles by name (`c2pa-trust-list`).
    #[serde(default)]
    pub trust: BTreeMap<String, InstalledTrust>,
    /// `generated_at` of the last verified index; a fetched index older than
    /// this is rejected.
    #[serde(default)]
    pub index_generated_at: Option<String>,
}

/// One installed pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPack {
    /// Installed version.
    pub version: String,
    /// SHA-256 of the archive as downloaded.
    pub archive_sha256: String,
    /// RFC 3339 install time.
    pub installed_at: String,
    /// `eval` or `pro`, copied from the index at install time.
    pub tier: String,
}

/// One installed trust-list bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledTrust {
    /// Bundle version from the index, or the fetch date for `upstream`.
    pub version: String,
    /// File name → sha256.
    pub files: BTreeMap<String, String>,
    /// RFC 3339 fetch time.
    pub fetched_at: String,
    /// Where the bytes came from: the signed index (`mirror`) or `upstream`.
    pub origin: String,
    /// Index URL or upstream directory the files came from.
    pub source_url: String,
}

/// Write via a temp file in `tmp_dir` and rename, so readers never see a
/// half-written file.
pub fn write_atomic(dst: &Path, bytes: &[u8], tmp_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(tmp_dir).map_err(|e| e.to_string())?;
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = tmp_dir.join(format!(
        ".{}.{}",
        dst.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, dst).map_err(|e| format!("{} -> {}: {e}", tmp.display(), dst.display()))
}

/// RFC 3339 UTC timestamp, seconds precision, without pulling in a time crate.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Civil-from-days (Howard Hinnant), valid for the whole i64 range we care about.
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn platform_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_shape() {
        let s = now_rfc3339();
        assert_eq!(s.len(), 20, "{s}");
        assert!(s.ends_with('Z'));
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], "T");
    }

    #[test]
    fn installed_roundtrip() {
        let dir = std::env::temp_dir().join(format!("halftone-store-{}", std::process::id()));
        let st = Store::at(&dir);
        st.ensure_layout().unwrap();
        let inst = Installed {
            index_generated_at: Some("2026-09-04T00:00:00Z".into()),
            ..Default::default()
        };
        st.save_installed(&inst).unwrap();
        let back = st.load_installed().unwrap();
        assert_eq!(
            back.index_generated_at.as_deref(),
            Some("2026-09-04T00:00:00Z")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
