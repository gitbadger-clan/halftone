//! Behavioral repro for D-011, compiles against both c2pa 0.90.22 and 0.91.0.
//!
//! `tests/with_value.rs` asserts on `trust.anchors`, which only exists from 0.91.
//! These tests assert on what a user sees instead: the validation state of a file
//! whose signer chains to the PEM bundle.
//!
//!   D011_FILE  a C2PA-signed asset that is `Trusted` under D011_PEM
//!   D011_PEM   the PEM bundle of trust anchors
//!
//!   env D011_FILE=... D011_PEM=... cargo test --test behavior

use c2pa::{Context, Reader, ValidationState, settings::Settings};
use std::fs::File;

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name} (see the module docs)"))
}

fn pem() -> String {
    std::fs::read_to_string(var("D011_PEM")).expect("read D011_PEM")
}

fn state(settings: Settings) -> ValidationState {
    let path = var("D011_FILE");
    let format = match path
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    };
    let context = Context::new().with_settings(settings).expect("context");
    Reader::from_context(context)
        .with_stream(format, File::open(&path).expect("open D011_FILE"))
        .expect("read manifest")
        .validation_state()
}

/// Control: without anchors the file must NOT be trusted, or the other two tests prove nothing.
#[test]
fn control_no_anchors_is_not_trusted() {
    assert_ne!(state(Settings::default()), ValidationState::Trusted);
}

/// Control: the legacy key through TOML (migrated on 0.91) must still trust the signer.
#[test]
fn control_legacy_key_via_toml_is_trusted() {
    let toml = format!("[trust]\ntrust_anchors = '''\n{}'''\n", pem());
    let settings = Settings::default().with_toml(&toml).expect("with_toml");
    assert_eq!(state(settings), ValidationState::Trusted);
}

/// The case under test: the same key and PEM through `with_value`.
#[test]
fn legacy_key_via_with_value_is_trusted() {
    let settings = Settings::default()
        .with_value("trust.trust_anchors", pem())
        .expect("with_value returns Ok on both versions");
    assert_eq!(state(settings), ValidationState::Trusted);
}
