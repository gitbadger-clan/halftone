//! End-to-end: a signed index in a directory installs a trust list, rejects a
//! tampered file, refuses rollback, and reports up-to-date on rerun.

use ed25519_dalek::{Signer, SigningKey};
use halftone_packs::store::Store;
use halftone_packs::update::{run, Action, Only, UpdateOptions};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIU\n-----END CERTIFICATE-----\n";

struct Bundle {
    dir: PathBuf,
    home: PathBuf,
    key: SigningKey,
}

impl Drop for Bundle {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn write_index(dir: &Path, key: &SigningKey, generated_at: &str, version: &str, pem: &str) {
    std::fs::write(dir.join("C2PA-TRUST-LIST.pem"), pem).unwrap();
    let index = serde_json::json!({
        "schema": 1,
        "generated_at": generated_at,
        "min_tool_version": "0.1.0",
        "artifacts": [{
            "kind": "trust-list",
            "name": "c2pa-trust-list",
            "version": version,
            "upstream": "https://example.invalid/trust-list",
            "files": [{
                "path": "C2PA-TRUST-LIST.pem",
                "url": "https://example.invalid/trust/C2PA-TRUST-LIST.pem",
                "sha256": hex::encode(Sha256::digest(pem.as_bytes())),
            }]
        }]
    })
    .to_string();
    let sig = hex::encode(key.sign(index.as_bytes()).to_bytes());
    std::fs::write(dir.join("index.json"), &index).unwrap();
    std::fs::write(dir.join("index.json.sig"), sig).unwrap();
}

fn bundle(tag: &str) -> Bundle {
    let base = std::env::temp_dir().join(format!("halftone-fromdir-{tag}-{}", std::process::id()));
    let dir = base.join("bundle");
    let home = base.join("home");
    std::fs::create_dir_all(&dir).unwrap();
    let key = SigningKey::from_bytes(&[42u8; 32]);
    write_index(&dir, &key, "2026-09-04T00:00:00Z", "2026-09-01", PEM);
    Bundle { dir, home, key }
}

fn opts(b: &Bundle) -> UpdateOptions {
    UpdateOptions {
        publisher_keys: vec![b.key.verifying_key()],
        only: Only::Trust,
        from_dir: Some(b.dir.clone()),
        ..Default::default()
    }
}

#[test]
fn installs_then_reports_up_to_date() {
    let b = bundle("install");
    let store = Store::at(&b.home);

    let report = run(&store, &opts(&b)).unwrap();
    assert!(
        matches!(&report[0], Action::Installed { name, .. } if name == "c2pa-trust-list"),
        "{report:?}"
    );
    assert_eq!(
        std::fs::read_to_string(store.trust_dir().join("C2PA-TRUST-LIST.pem")).unwrap(),
        PEM
    );
    assert!(store.trust_dir().join("meta.json").exists());
    let inst = store.load_installed().unwrap();
    assert_eq!(
        inst.index_generated_at.as_deref(),
        Some("2026-09-04T00:00:00Z")
    );
    assert_eq!(inst.trust["c2pa-trust-list"].origin, "mirror");

    let report = run(&store, &opts(&b)).unwrap();
    assert!(matches!(&report[0], Action::UpToDate { .. }), "{report:?}");
}

#[test]
fn dry_run_writes_nothing() {
    let b = bundle("dry");
    let store = Store::at(&b.home);
    let report = run(
        &store,
        &UpdateOptions {
            dry_run: true,
            ..opts(&b)
        },
    )
    .unwrap();
    assert!(
        matches!(&report[0], Action::WouldInstall { .. }),
        "{report:?}"
    );
    assert!(!store.trust_dir().join("C2PA-TRUST-LIST.pem").exists());
    assert!(!store.installed_path().exists());
}

#[test]
fn tampered_file_is_rejected() {
    let b = bundle("tamper");
    std::fs::write(
        b.dir.join("C2PA-TRUST-LIST.pem"),
        PEM.replace("MIIB", "MIIC"),
    )
    .unwrap();
    let err = run(&Store::at(&b.home), &opts(&b)).unwrap_err();
    assert!(err.contains("sha256 mismatch"), "{err}");
}

#[test]
fn wrong_publisher_key_is_rejected() {
    let b = bundle("key");
    let o = UpdateOptions {
        publisher_keys: vec![SigningKey::from_bytes(&[1u8; 32]).verifying_key()],
        ..opts(&b)
    };
    let err = run(&Store::at(&b.home), &o).unwrap_err();
    assert!(err.contains("signature"), "{err}");
}

#[test]
fn rollback_is_refused() {
    let b = bundle("rollback");
    let store = Store::at(&b.home);
    run(&store, &opts(&b)).unwrap();
    write_index(&b.dir, &b.key, "2026-09-03T00:00:00Z", "2026-08-01", PEM);
    let err = run(&store, &opts(&b)).unwrap_err();
    assert!(err.contains("rollback"), "{err}");
}
