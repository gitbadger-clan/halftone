//! The vendored rubric is the published file, and the YAML library reads it
//! exactly as PyYAML (which the reference evaluator uses) does.

use halftone_rubric::rubric::yaml_documents;
use halftone_rubric::{JValue, SIGNALS_LOCAL_SHA256, SIGNALS_LOCAL_YAML};
use sha2::{Digest, Sha256};
use std::path::Path;

#[test]
fn the_vendored_rubric_is_the_official_file_unmodified() {
    let hex: String = Sha256::digest(SIGNALS_LOCAL_YAML.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hex, SIGNALS_LOCAL_SHA256);
}

#[test]
fn yaml_reads_the_rubric_exactly_as_pyyaml_does() {
    let ours = yaml_documents(SIGNALS_LOCAL_YAML).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/rubric-signals/rubric-parsed.json");
    let theirs = JValue::from_json_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(JValue::Array(ours).to_serde(), theirs.to_serde());
}
