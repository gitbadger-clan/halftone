//! D-011: the legacy key is migrated from TOML but not from `with_value`.
#![allow(deprecated)]
use c2pa::settings::Settings;

const PEM: &str = include_str!("../../../crates/halftone-c2pa/trust/C2PA-TRUST-LIST.pem");

/// Control: the same legacy key, loaded from TOML, reaches `trust.anchors`.
#[test]
fn legacy_key_via_toml_reaches_anchors() {
    let toml = format!("[trust]\ntrust_anchors = \"\"\"\n{PEM}\"\"\"\n");
    let s = Settings::new().with_toml(&toml).expect("toml accepted");
    assert!(
        s.trust.anchors.is_some(),
        "TOML path did not migrate the legacy key"
    );
}

/// The bug: `with_value` accepts the legacy key without error, and nothing reaches
/// `trust.anchors`, so validation runs with zero anchors.
#[test]
fn legacy_key_via_with_value_reaches_anchors() {
    let s = Settings::new()
        .with_value("trust.trust_anchors", PEM)
        .expect("with_value accepted the legacy key");
    assert!(
        s.trust.anchors.is_some(),
        "trust.trust_anchors was accepted by with_value, but trust.anchors is empty"
    );
}
