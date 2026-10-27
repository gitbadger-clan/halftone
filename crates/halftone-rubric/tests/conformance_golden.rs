//! The conformance evaluator against the reference: every case in
//! `testdata/rubric-conformance/` (crJSON in `in/`, the official evaluator's output
//! in `expected/`, written by `scripts/rubric-conformance-golden.py`) under the
//! integrity rubric, the conformance-0.2-spec-2.4 rubric and the synthetic
//! mechanics rubric. A `.crash` file means the reference rejects the document
//! outright, and so must this evaluator. An error row's text after `Error:` is
//! Python's exception message, which is not compared.

use halftone_rubric::{JValue, Rubric};
use std::path::Path;

/// Error-row texts reduced to their prefix on both sides.
fn normalize(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            if let Some(serde_json::Value::String(t)) = m.get_mut("reportText") {
                if t.starts_with("Error: ") {
                    *t = "Error:".into();
                }
            }
            m.values_mut().for_each(normalize);
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(normalize),
        _ => {}
    }
}

#[test]
fn every_case_matches_the_official_conformance_evaluator() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/rubric-conformance");
    let mechanics_yaml = std::fs::read_to_string(dir.join("rubrics/mechanics.yml")).unwrap();
    let rubrics: Vec<(&str, Rubric)> = vec![
        ("integrity", halftone_rubric::integrity().unwrap()),
        (
            "conformance0.2-spec2.4",
            halftone_rubric::conformance_0_2_spec_2_4().unwrap(),
        ),
        ("mechanics", Rubric::from_yaml(&mechanics_yaml).unwrap()),
    ];
    let mut names: Vec<_> = std::fs::read_dir(dir.join("in"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    names.sort();
    let (mut compared, mut crashes, mut failures) = (0, 0, Vec::new());
    for p in &names {
        let case = p.file_stem().unwrap().to_string_lossy().to_string();
        let input = JValue::from_json_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        for (rname, rubric) in &rubrics {
            let stem = dir.join("expected").join(format!("{case}__{rname}"));
            let ours = rubric.evaluate_conformance(&input);
            if Path::new(&format!("{}.crash", stem.display())).exists() {
                crashes += 1;
                if ours.is_ok() {
                    failures.push(format!(
                        "{case} / {rname}: the reference rejects this document; ours did not"
                    ));
                }
                continue;
            }
            let mut want: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(format!("{}.json", stem.display())).unwrap(),
            )
            .unwrap();
            let mut got = match ours {
                Ok(r) => r.output.to_serde(),
                Err(e) => {
                    failures.push(format!("{case} / {rname}: ours failed: {e}"));
                    continue;
                }
            };
            normalize(&mut want);
            normalize(&mut got);
            compared += 1;
            if got != want {
                failures.push(format!("{case} / {rname}:\n  want {want}\n  got  {got}"));
            }
        }
    }
    assert!(
        compared >= 45 && crashes >= 3,
        "fixtures missing: run scripts/rubric-conformance-golden.py"
    );
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        compared + crashes,
        failures.join("\n\n")
    );
}
