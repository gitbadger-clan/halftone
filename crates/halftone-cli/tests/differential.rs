//! Differential test: Halftone's `marking_metadata` and `c2pa` sources against
//! ExifTool and c2patool ground truth collected by `scripts/differential.py`.
//!
//! Corpus location: `$HALFTONE_DIFFERENTIAL_CORPUS`, else `<repo>/corpus/differential`.
//! The directory must contain `expectations.json`; if it does not exist the test
//! prints a notice and passes, so CI without the corpus stays green. Set
//! `HALFTONE_DIFFERENTIAL_BATCH=<file>` to compare a previously produced
//! `ht inspect --batch --json` document instead of running the binary.
//!
//! Files are matched by SHA-256, not path. Every disagreement is printed as one row
//! of a table before the assertion fails, so a run is also the differential log.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[derive(Debug)]
struct Disagreement {
    file: String,
    field: &'static str,
    expected: String,
    got: String,
}

/// Corpora to compare. `$HALFTONE_DIFFERENTIAL_CORPUS` names one directory;
/// otherwise every immediate subdirectory of `<repo>/corpus/differential` (one per
/// stratum) that has an `expectations.json`, plus the root itself if it has one.
fn corpora() -> Vec<PathBuf> {
    if let Some(d) = std::env::var_os("HALFTONE_DIFFERENTIAL_CORPUS") {
        let d = PathBuf::from(d);
        return if d.join("expectations.json").is_file() {
            vec![d]
        } else {
            Vec::new()
        };
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("corpus")
        .join("differential");
    let mut out = Vec::new();
    if root.join("expectations.json").is_file() {
        out.push(root.clone());
    }
    if let Ok(rd) = std::fs::read_dir(&root) {
        let mut subs: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && p.join("expectations.json").is_file())
            .collect();
        subs.sort();
        out.extend(subs);
    }
    out
}

/// Run `ht inspect --batch --json` over the files in chunks and return every
/// (summary row, inspection) pair.
fn run_ht(files: &[PathBuf]) -> Vec<(Value, Value)> {
    if let Some(p) = std::env::var_os("HALFTONE_DIFFERENTIAL_BATCH") {
        let text = std::fs::read_to_string(p).expect("read HALFTONE_DIFFERENTIAL_BATCH");
        return pairs(serde_json::from_str(&text).expect("batch JSON"));
    }
    let mut out = Vec::new();
    for chunk in files.chunks(40) {
        let res = Command::new(env!("CARGO_BIN_EXE_ht"))
            .arg("inspect")
            .arg("--batch")
            .arg("--json")
            .args(["--only", "manifest,container"])
            .args(chunk)
            .output()
            .expect("run ht");
        assert!(
            matches!(res.status.code(), Some(0 | 2)),
            "ht exited with {:?}\n{}",
            res.status.code(),
            String::from_utf8_lossy(&res.stderr)
        );
        let text = String::from_utf8(res.stdout).unwrap();
        let batch: Value = serde_json::from_str(text.trim()).expect("batch JSON");
        out.extend(pairs(batch));
    }
    out
}

fn pairs(batch: Value) -> Vec<(Value, Value)> {
    let summary = batch["summary"].as_array().cloned().unwrap_or_default();
    let insps = batch["inspections"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        summary.len(),
        insps.len(),
        "summary/inspections length mismatch"
    );
    summary.into_iter().zip(insps).collect()
}

fn str_set(v: &Value) -> Vec<String> {
    let mut s: Vec<String> = v
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    s.sort();
    s.dedup();
    s
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

fn show<T: std::fmt::Debug>(t: T) -> String {
    format!("{t:?}")
}

fn evidence<'a>(insp: &'a Value, source: &str) -> Option<&'a Value> {
    insp["evidence"]
        .as_array()?
        .iter()
        .find(|e| e["source"]["name"] == source)
}

/// Compare one file. `trust_exact` is whether c2patool ran with the same anchors as ht.
fn compare_file(
    name: &str,
    exp: &Value,
    row: &Value,
    insp: &Value,
    trust_exact: bool,
    out: &mut Vec<Disagreement>,
) {
    let mut push = |field: &'static str, expected: String, got: String| {
        if expected != got {
            out.push(Disagreement {
                file: name.to_string(),
                field,
                expected,
                got,
            });
        }
    };

    // ---- container: marking_metadata vs exiftool -------------------------------
    let ex = &exp["exiftool"];
    if ex.get("error").is_none() {
        let mime = ex["mime"].as_str().unwrap_or("");
        let marking_ev = evidence(insp, "marking_metadata");
        let applicable = matches!(mime, "image/jpeg" | "image/png" | "image/webp");
        let status = marking_ev
            .map(|e| e["status"].as_str().unwrap_or("").to_string())
            .unwrap_or_else(|| "missing".into());
        if !applicable {
            push("marking.applicable", "not_applicable".into(), status);
        } else {
            push(
                "marking.ran",
                "ran".into(),
                if status == "not_applicable" || status == "missing" {
                    status
                } else {
                    "ran".into()
                },
            );
            let xmp_packets = row["marking"]["xmp_packets"].as_u64();
            push(
                "xmp_present",
                show(ex["xmp_present"].as_bool().unwrap_or(false)),
                show(xmp_packets.map(|n| n > 0).unwrap_or(false)),
            );
            push(
                "xmp.digital_source_type",
                show(str_set(&ex["digital_source_type"])),
                show(str_set(&row["marking"]["digital_source_type"])),
            );
            let iim = marking_ev.map(|e| &e["details"]["iim"]);
            push(
                "iim.originating_program",
                show(opt_str(&ex["iim_originating_program"])),
                show(iim.and_then(|i| opt_str(&i["originating_program"]))),
            );
            push(
                "iim.program_version",
                show(opt_str(&ex["iim_program_version"])),
                show(iim.and_then(|i| opt_str(&i["program_version"]))),
            );
        }
    }

    // ---- intended verdicts (synthetic strata) -----------------------------------
    // cases.json, merged by the collector, states what marking_metadata must say.
    let want = &exp["halftone"];
    if want.is_object() {
        let status = evidence(insp, "marking_metadata")
            .map(|e| e["status"].as_str().unwrap_or("").to_string())
            .unwrap_or_else(|| "missing".into());
        push(
            "intended.marking.status",
            want["status"].as_str().unwrap_or("").to_string(),
            status,
        );
        push(
            "intended.marking.digital_source_type",
            show(str_set(&want["digital_source_type"])),
            show(str_set(&row["marking"]["digital_source_type"])),
        );
    }

    // ---- manifest: c2pa vs c2patool ---------------------------------------------
    let ct = &exp["c2patool"];
    let c2pa_ev = evidence(insp, "c2pa");
    let layer_enabled = c2pa_ev
        .map(|e| {
            !e["rationale"]
                .as_str()
                .unwrap_or("")
                .contains("built without")
        })
        .unwrap_or(false);
    if ct.is_object() && ct.get("remote_manifest").is_some() && layer_enabled {
        // Only a reference to a remote manifest: neither tool fetches it. Halftone
        // must say Inconclusive (a container is there, unverifiable here), never
        // Present or Absent.
        let status = c2pa_ev
            .map(|e| e["status"].as_str().unwrap_or("").to_string())
            .unwrap_or_default();
        push("c2pa.remote_manifest", "inconclusive".into(), status);
    }
    if ct.is_object() && ct.get("error").is_some() && layer_enabled {
        // c2patool refused the file (remote manifest, unknown algorithm, prerelease
        // claim…). Halftone may say whatever it likes except Present.
        let status = c2pa_ev
            .map(|e| e["status"].as_str().unwrap_or("").to_string())
            .unwrap_or_default();
        push(
            "c2pa.reader_error_not_present",
            "not present".into(),
            if status == "present" {
                status
            } else {
                "not present".into()
            },
        );
    }
    if ct.is_object() && ct.get("error").is_none() && layer_enabled {
        let exp_present = ct["present"].as_bool();
        let state = opt_str(&row["manifest"]["validation_state"]);
        push(
            "c2pa.present",
            show(exp_present.unwrap_or(false)),
            show(state.is_some()),
        );
        if exp_present == Some(true) && state.is_some() {
            let exp_state = opt_str(&ct["validation_state"]);
            let accept = match (exp_state.as_deref(), state.as_deref()) {
                (Some("Valid"), Some("Trusted")) if !trust_exact => true,
                (a, b) => a == b,
            };
            if !accept {
                push("c2pa.validation_state", show(exp_state), show(state));
            }
            push(
                "c2pa.issuer",
                show(opt_str(&ct["issuer"])),
                show(opt_str(&row["manifest"]["issuer"])),
            );
            push(
                "c2pa.claim_generator",
                show(opt_str(&ct["claim_generator"])),
                show(opt_str(&row["manifest"]["claim_generator"])),
            );
            push(
                "c2pa.digital_source_type",
                show(str_set(&ct["digital_source_type"])),
                show(str_set(&row["manifest"]["digital_source_type"])),
            );
        }
    }
}

fn render(rows: &[Disagreement]) -> String {
    let mut s = String::from("| file | field | expected | halftone |\n|---|---|---|---|\n");
    for d in rows {
        s.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            d.file, d.field, d.expected, d.got
        ));
    }
    s
}

#[test]
#[ignore = "needs corpus/differential on disk; run by the differential workflow or the nightly runner"]
fn halftone_agrees_with_exiftool_and_c2patool() {
    let dirs = corpora();
    if dirs.is_empty() {
        eprintln!("differential: no corpus with expectations.json; skipped");
        return;
    }
    let mut all: Vec<Disagreement> = Vec::new();
    let mut total_files = 0;
    let mut total_compared = 0;
    for dir in &dirs {
        let exp: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("expectations.json")).unwrap())
                .expect("expectations.json");
        let trust_exact = !exp["trust_anchors"].is_null();
        let files_obj = exp["files"].as_object().expect("files object");
        let files: Vec<PathBuf> = files_obj.keys().map(|k| dir.join(k)).collect();
        if files.is_empty() {
            continue;
        }
        let label = dir
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();

        let results = run_ht(&files);
        let by_sha: HashMap<String, (Value, Value)> = results
            .into_iter()
            .filter_map(|(row, insp)| Some((row["sha256"].as_str()?.to_string(), (row, insp))))
            .collect();

        let mut dis = Vec::new();
        let mut compared = 0;
        for (name, e) in files_obj {
            let sha = e["sha256"].as_str().unwrap_or("");
            let name = format!("{label}/{name}");
            match by_sha.get(sha) {
                Some((row, insp)) => {
                    compared += 1;
                    compare_file(&name, e, row, insp, trust_exact, &mut dis);
                }
                None => dis.push(Disagreement {
                    file: name,
                    field: "file",
                    expected: format!("sha256 {sha}"),
                    got: "not in ht output (missing, unreadable, or bytes changed)".into(),
                }),
            }
        }
        println!(
            "differential[{label}]: {compared} of {} files compared, {} disagreements",
            files_obj.len(),
            dis.len()
        );
        total_files += files_obj.len();
        total_compared += compared;
        all.extend(dis);
    }
    println!(
        "differential: {total_compared} of {total_files} files across {} corpora, {} disagreements\n{}",
        dirs.len(),
        all.len(),
        render(&all)
    );
    assert!(
        all.is_empty(),
        "{} disagreement(s):\n{}",
        all.len(),
        render(&all)
    );
}
