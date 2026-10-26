//! The signals evaluator against the reference: every case in
//! `testdata/rubric-signals/` (crJSON in `in/`, the official evaluator's output in
//! `expected/`, both written by `scripts/rubric-golden.py`) must match exactly.

use halftone_rubric::JValue;
use std::path::Path;

#[test]
fn every_golden_case_matches_the_official_evaluator() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/rubric-signals");
    let rubric = halftone_rubric::signals_local().unwrap();
    let mut names: Vec<_> = std::fs::read_dir(dir.join("in"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    names.sort();
    let mut failures = Vec::new();
    for p in &names {
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        let input = JValue::from_json_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let expected =
            std::fs::read_to_string(dir.join("expected").join(format!("{name}.json"))).unwrap();
        let want: serde_json::Value = serde_json::from_str(&expected).unwrap();
        let got = rubric.evaluate_signals(&input).unwrap();
        if got.output.to_serde() != want {
            failures.push(format!(
                "{name}:\n  want {want}\n  got  {}",
                got.output.to_serde()
            ));
        }
        assert!(got.errors.is_empty(), "{name}: {:?}", got.errors);
    }
    assert!(
        names.len() >= 31,
        "fixtures missing: run scripts/rubric-golden.py"
    );
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        names.len(),
        failures.join("\n")
    );
}
