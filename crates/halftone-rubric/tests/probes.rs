//! The json-formula interpreter against json-formula-py, expression by expression:
//! each probe in `testdata/rubric-signals/formula-probes.json` holds the reference's
//! result, or marks that it failed; ours must agree.

use halftone_rubric::JValue;
use halftone_rubric::formula::{Engine, Globals, parse};
use std::path::Path;

#[test]
fn every_probe_matches_the_reference() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/rubric-signals/formula-probes.json");
    let JValue::Array(probes) =
        JValue::from_json_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    else {
        panic!("probes file is not an array")
    };
    let mut failures = Vec::new();
    for p in &probes {
        let expr = p.get("expr").and_then(JValue::as_str).unwrap();
        let data = p.get("data").cloned().unwrap_or_default();
        let ours =
            parse(expr, &[]).and_then(|ast| Engine::new().search(&ast, &data, &mut Globals::new()));
        let agrees = match (p.get("result"), &ours) {
            (Some(want), Ok(got)) => want.to_serde() == got.to_serde(),
            (None, Err(_)) => true,
            _ => false,
        };
        if !agrees {
            let want = p
                .get("result")
                .map_or("an error".to_string(), |v| v.to_serde().to_string());
            failures.push(format!(
                "{expr:40} reference {want} / ours {:?}",
                ours.map(|v| v.to_serde())
            ));
        }
    }
    assert!(
        probes.len() >= 100,
        "probes missing: run scripts/rubric-golden.py"
    );
    assert!(
        failures.is_empty(),
        "{} of {} probes differ:\n{}",
        failures.len(),
        probes.len(),
        failures.join("\n")
    );
}
