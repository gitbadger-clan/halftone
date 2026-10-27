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

#[test]
fn the_vendored_conformance_rubrics_are_the_official_files_unmodified() {
    use halftone_rubric::{
        CONFORMANCE_0_2_SPEC_2_4_SHA256, CONFORMANCE_0_2_SPEC_2_4_YAML, INTEGRITY_SHA256,
        INTEGRITY_YAML,
    };
    for (yaml, sha) in [
        (INTEGRITY_YAML, INTEGRITY_SHA256),
        (
            CONFORMANCE_0_2_SPEC_2_4_YAML,
            CONFORMANCE_0_2_SPEC_2_4_SHA256,
        ),
    ] {
        let hex: String = Sha256::digest(yaml.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hex, sha);
    }
}

#[test]
fn yaml_reads_the_conformance_rubrics_exactly_as_pyyaml_does() {
    use halftone_rubric::{CONFORMANCE_0_2_SPEC_2_4_YAML, INTEGRITY_YAML};
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/rubric-conformance/rubrics");
    for (yaml, parsed) in [
        (INTEGRITY_YAML, "integrity.parsed.json"),
        (
            CONFORMANCE_0_2_SPEC_2_4_YAML,
            "conformance0.2-spec2.4.parsed.json",
        ),
    ] {
        let ours = yaml_documents(yaml).unwrap();
        let theirs =
            JValue::from_json_str(&std::fs::read_to_string(dir.join(parsed)).unwrap()).unwrap();
        assert_eq!(
            JValue::Array(ours).to_serde(),
            theirs.to_serde(),
            "{parsed}"
        );
    }
}
