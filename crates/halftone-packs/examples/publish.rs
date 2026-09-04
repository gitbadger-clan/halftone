//! Publisher-side helper. Not shipped in the CLI: it needs the signing seed.
//!
//! ```text
//! cargo run -p halftone-packs --example publish -- keygen  seed.bin
//! cargo run -p halftone-packs --example publish -- pubkey  seed.bin
//! cargo run -p halftone-packs --example publish -- index   seed.bin <site-dir> <base-url>
//! ```
//!
//! `index` walks `<site-dir>/trust/*.pem` and `<site-dir>/packs/*.tar.zst`,
//! computes hashes, signs pack archives, writes `<site-dir>/index.json` and
//! `index.json.sig`. Upload `<site-dir>` as-is to the static host.

use ed25519_dalek::{Signer, SigningKey};
use halftone_packs::index::{
    Artifact, Index, PackArtifact, TrustFile, TrustListArtifact, INDEX_SCHEMA,
};
use halftone_packs::manifest::PackManifest;
use halftone_packs::store::now_rfc3339;
use sha2::{Digest, Sha256};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.as_slice() {
        [c, seed] if c == "keygen" => keygen(Path::new(seed)),
        [c, seed] if c == "pubkey" => {
            load(Path::new(seed)).map(|k| println!("{}", hex::encode(k.verifying_key().to_bytes())))
        }
        [c, seed, site, base] if c == "index" => index(Path::new(seed), Path::new(site), base),
        _ => Err("usage: publish keygen|pubkey <seed> | index <seed> <site-dir> <base-url>".into()),
    };
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn keygen(seed: &Path) -> Result<(), String> {
    if seed.exists() {
        return Err(format!(
            "{} exists; refusing to overwrite a signing key",
            seed.display()
        ));
    }
    // 32 random bytes from the OS; no rand dependency needed.
    let bytes = std::fs::read("/dev/urandom").ok().filter(|b| b.len() >= 32);
    let bytes = match bytes {
        Some(b) => b[..32].to_vec(),
        None => return Err("could not read /dev/urandom; generate 32 bytes another way".into()),
    };
    std::fs::write(seed, &bytes).map_err(|e| e.to_string())?;
    let key = SigningKey::from_bytes(bytes.as_slice().try_into().unwrap());
    println!("seed written to {}", seed.display());
    println!(
        "public key (hex): {}",
        hex::encode(key.verifying_key().to_bytes())
    );
    Ok(())
}

fn load(seed: &Path) -> Result<SigningKey, String> {
    let b = std::fs::read(seed).map_err(|e| format!("{}: {e}", seed.display()))?;
    let arr: [u8; 32] = b
        .as_slice()
        .try_into()
        .map_err(|_| "seed must be 32 bytes")?;
    Ok(SigningKey::from_bytes(&arr))
}

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn index(seed: &Path, site: &Path, base: &str) -> Result<(), String> {
    let key = load(seed)?;
    let base = base.trim_end_matches('/');
    let mut artifacts = Vec::new();

    // Trust lists: everything in trust/ becomes one bundle.
    let trust_dir = site.join("trust");
    if trust_dir.is_dir() {
        let mut files = Vec::new();
        let mut names: Vec<_> = std::fs::read_dir(&trust_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "pem"))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            let bytes = std::fs::read(trust_dir.join(&name)).map_err(|e| e.to_string())?;
            files.push(TrustFile {
                url: format!("{base}/trust/{name}"),
                upstream_url: Some(format!(
                    "https://raw.githubusercontent.com/c2pa-org/conformance-public/main/trust-list/{name}"
                )),
                sha256: sha(&bytes),
                path: name,
            });
        }
        if !files.is_empty() {
            artifacts.push(Artifact::TrustList(TrustListArtifact {
                name: "c2pa-trust-list".into(),
                version: now_rfc3339()[..10].to_string(),
                files,
                upstream: "https://github.com/c2pa-org/conformance-public/tree/main/trust-list"
                    .into(),
            }));
        }
    }

    // Packs: <name>-<version>.tar.zst with pack.json inside; we read the manifest
    // from a sibling <name>-<version>.pack.json to avoid decompressing here.
    let packs_dir = site.join("packs");
    if packs_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&packs_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with(".tar.zst"))
            .collect();
        entries.sort();
        for archive in entries {
            let stem = archive
                .file_name()
                .unwrap()
                .to_string_lossy()
                .trim_end_matches(".tar.zst")
                .to_string();
            let manifest_path = packs_dir.join(format!("{stem}.pack.json"));
            let manifest: PackManifest = serde_json::from_slice(
                &std::fs::read(&manifest_path)
                    .map_err(|e| format!("{}: {e}", manifest_path.display()))?,
            )
            .map_err(|e| e.to_string())?;
            manifest.validate_license()?;
            let bytes = std::fs::read(&archive).map_err(|e| e.to_string())?;
            artifacts.push(Artifact::Pack(PackArtifact {
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                tier: manifest.tier,
                url: format!("{base}/packs/{stem}.tar.zst"),
                sha256: sha(&bytes),
                size: bytes.len() as u64,
                sig: hex::encode(key.sign(&bytes).to_bytes()),
            }));
        }
    }

    let idx = Index {
        schema: INDEX_SCHEMA,
        generated_at: now_rfc3339(),
        min_tool_version: "0.1.0".into(),
        artifacts,
    };
    let json = serde_json::to_string_pretty(&idx).map_err(|e| e.to_string())?;
    let sig = hex::encode(key.sign(json.as_bytes()).to_bytes());
    std::fs::write(site.join("index.json"), &json).map_err(|e| e.to_string())?;
    std::fs::write(site.join("index.json.sig"), sig).map_err(|e| e.to_string())?;
    println!(
        "wrote index.json ({} artifacts) and index.json.sig",
        idx.artifacts.len()
    );
    println!(
        "publisher key: {}",
        hex::encode(key.verifying_key().to_bytes())
    );
    Ok(())
}
