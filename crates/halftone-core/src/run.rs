//! What a run was given: the conditions its verdicts hold under.
//!
//! A verdict depends on more than the file. The manifest layer judges a signer
//! against the certificate lists that were loaded, a fetched manifest depends on a
//! server and the time of asking, and `reveal` decides whether the output repeats
//! what a file discloses. [`RunInfo`] states those conditions once per inspection, so
//! a report can be read, and the check repeated, without knowing how the tool was
//! called.
//!
//! Three rules for what goes in here:
//! - It is the tool's own statement about itself and its inputs. Nothing signs or
//!   verifies it; a hash identifies a list, it does not vouch for it.
//! - Files are named without their directory. A path would put a user name from the
//!   operator's machine into a document meant to be filed or shared.
//! - Something that could not be established is named in [`RunInfo::gaps`], not left
//!   out silently.
//!
//! The record is taken once, when the registry is built. An evidence entry's own
//! `details` (the manifest layer's `details.trust`) stays the record of what that
//! one source loaded for that one file.

use serde::{Deserialize, Serialize};

use crate::Layer;

/// The conditions of one run. Part of the verdict schema (1.5.0).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunInfo {
    /// The options that change what is reported.
    pub options: RunOptions,
    /// The certificate lists and the key-usage policy the manifest layer judges
    /// signers against, internal lists first. Empty when none was loaded: the
    /// manifest layer did not run, the tool was built without it, or the internal
    /// list was disabled and no anchors were given.
    pub trust: Vec<TrustInput>,
    /// Model packs installed when the run started, by name. Installed is not used:
    /// which source ran with which pack is that source's `source.version` and
    /// `calibration`. Empty when none is installed.
    pub packs: Vec<PackRef>,
    /// What could not be established for this record, in plain words (a trust list
    /// that could not be loaded, a pack store that could not be read). Empty when
    /// the record is complete.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<String>,
}

/// The options of a run that change what is reported.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOptions {
    /// The layers that were asked for, in execution order. A layer missing here did
    /// not run; that is not a finding about the file.
    pub layers: Vec<Layer>,
    /// Where the internal C2PA trust list was to come from: `auto` (the installed
    /// copy, else the one built in), `vendored` (the one built in), `none`, or
    /// `file` (a list the operator supplied in its place). Which list that turned
    /// out to be is in [`RunInfo::trust`].
    pub trust_list: String,
    /// Whether a manifest that a file only refers to could be fetched. When true, a
    /// result may depend on a server and on the time of checking; the evidence that
    /// used a fetched manifest says so itself (`details.fetched_from`).
    pub fetch_remote_manifests: bool,
    /// Whether identifier values and free text (of rights statements, and the Other
    /// Constraints text of a data-mining reservation) are included. When false they
    /// are masked everywhere in the output; when true the output repeats what the
    /// file discloses.
    pub reveal: bool,
    /// An extra writer-fingerprint database merged over the built-in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprints: Option<FileRef>,
}

/// A file a run was given, identified by name and content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    /// File name, without its directory.
    pub name: String,
    /// SHA-256 of the file's bytes, hex.
    pub sha256: String,
}

/// One certificate list, or the key-usage policy, as loaded for a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustInput {
    /// What it is used for: `manifest` (roots for signing certificates), `tsa`
    /// (roots for time-stamp authorities) or `eku_policy` (the extended key usages
    /// a signing certificate may carry).
    pub kind: String,
    /// Where this copy came from: `vendored` (built into the binary), `installed`
    /// (fetched by `ht packs update`), `file` (given in place of the official list)
    /// or `custom` (anchors the operator added).
    pub origin: String,
    /// File name, without its directory.
    pub name: String,
    /// The list's own version: the upstream commit and date of a vendored snapshot,
    /// or the bundle version of an installed list. Absent for an operator's file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// When an installed list was fetched (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<String>,
    /// SHA-256 of the bytes as loaded, hex.
    pub sha256: String,
    /// Number of certificates in a list. Absent for the key-usage policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificates: Option<usize>,
}

/// An installed model pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackRef {
    /// Pack name.
    pub name: String,
    /// Installed version.
    pub version: String,
    /// `eval` or `pro`.
    pub tier: String,
    /// SHA-256 of the pack's archive as it was downloaded, hex.
    pub archive_sha256: String,
}

/// The name of a file without its directory, for a record that must not carry a
/// path. A path with no file name (`/`, `..`) gives `"(unnamed)"`.
pub fn file_name_only(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || "(unnamed)".to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RunInfo {
        RunInfo {
            options: RunOptions {
                layers: vec![Layer::Manifest, Layer::Container],
                trust_list: "auto".into(),
                fetch_remote_manifests: false,
                reveal: false,
                fingerprints: None,
            },
            trust: vec![TrustInput {
                kind: "manifest".into(),
                origin: "vendored".into(),
                name: "C2PA-TRUST-LIST.pem".into(),
                version: Some(
                    "c2pa-org/conformance-public@d6dec63476e2 2026-10-26T17:11:49Z".into(),
                ),
                fetched_at: None,
                sha256: "ab".repeat(32),
                certificates: Some(32),
            }],
            packs: vec![],
            gaps: vec![],
        }
    }

    #[test]
    fn the_record_states_its_empty_lists_and_leaves_out_what_does_not_apply() {
        let js = serde_json::to_value(sample()).unwrap();
        // "No pack installed" is a statement, so the empty list is written.
        assert_eq!(js["packs"], serde_json::json!([]));
        assert_eq!(
            js["options"]["layers"],
            serde_json::json!(["manifest", "container"])
        );
        assert_eq!(js["options"]["reveal"], false);
        // Absent rather than null: no extra database, no fetch time, nothing missing.
        assert!(js["options"].get("fingerprints").is_none());
        assert!(js["trust"][0].get("fetched_at").is_none());
        assert!(js.get("gaps").is_none());
        let back: RunInfo = serde_json::from_value(js).unwrap();
        assert_eq!(back, sample());
    }

    #[test]
    fn a_file_is_named_without_its_directory() {
        let p = std::path::Path::new("/Users/someone/keys/ca.pem");
        assert_eq!(file_name_only(p), "ca.pem");
        assert_eq!(file_name_only(std::path::Path::new("ca.pem")), "ca.pem");
        assert_eq!(file_name_only(std::path::Path::new("/")), "(unnamed)");
    }
}
