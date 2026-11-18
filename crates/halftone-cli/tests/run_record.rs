//! Every inspection states the conditions it was made under (schema 1.5.0):
//! `tool.build` and `run`. These tests make their own input files and their own
//! empty data directory, so they do not depend on fixtures or on what is installed.

use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

/// A fresh directory with one small PNG in it, and an empty data directory.
struct Scene {
    dir: PathBuf,
    image: String,
    home: PathBuf,
}

impl Scene {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ht-run-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let image = dir.join("grey.png");
        image::RgbImage::from_pixel(16, 16, image::Rgb([120, 120, 120]))
            .save(&image)
            .unwrap();
        Self {
            image: image.to_str().unwrap().to_string(),
            dir,
            home,
        }
    }

    /// `ht inspect --json <args>`: the parsed lines of stdout.
    fn inspect(&self, args: &[&str]) -> Vec<Value> {
        let out = Command::new(env!("CARGO_BIN_EXE_ht"))
            .env("HALFTONE_HOME", &self.home)
            .args(["inspect", "--json"])
            .args(args)
            .output()
            .expect("run ht");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            matches!(out.status.code(), Some(0 | 2)),
            "ht exited with {:?}\n{stderr}",
            out.status.code()
        );
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {e}\n{l}")))
            .collect()
    }

    /// The record must not carry a directory of this machine.
    fn assert_no_path(&self, v: &Value) {
        let text = v.to_string();
        let dir = self.dir.to_str().unwrap();
        assert!(!text.contains(dir), "{dir} in {text}");
        assert!(
            !text.contains(std::env::temp_dir().to_str().unwrap()),
            "{text}"
        );
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn is_hex(v: &Value, len: usize) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[test]
fn a_plain_run_states_its_build_options_trust_and_packs() {
    let s = Scene::new("plain");
    let v = &s.inspect(&[&s.image])[0];
    assert_eq!(v["schema_version"], "1.5.0");

    // The build: the features always, a commit only when the build could tell, and
    // `modified` only next to a commit.
    let build = &v["tool"]["build"];
    let features: Vec<&str> = if cfg!(feature = "c2pa") {
        vec!["c2pa"]
    } else {
        vec![]
    };
    assert_eq!(build["features"], json!(features));
    match build.get("commit") {
        Some(c) => {
            assert!(is_hex(c, 40) || is_hex(c, 64), "{c}");
            assert!(build["modified"].is_boolean(), "{build}");
        }
        None => assert!(build.get("modified").is_none(), "{build}"),
    }

    let run = &v["run"];
    assert_eq!(
        run["options"],
        json!({
            "layers": ["manifest", "container", "mark", "blind"],
            "trust_list": "auto",
            "fetch_remote_manifests": false,
            "reveal": false,
        })
    );
    // Nothing is installed in the empty data directory, and the record says so.
    assert_eq!(run["packs"], json!([]));
    assert!(run.get("gaps").is_none(), "{run}");

    let trust = run["trust"].as_array().unwrap();
    if cfg!(feature = "c2pa") {
        // Nothing installed, so the lists are the ones built in.
        let rows: Vec<(&str, &str, &str)> = trust
            .iter()
            .map(|t| {
                (
                    t["kind"].as_str().unwrap(),
                    t["origin"].as_str().unwrap(),
                    t["name"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("manifest", "vendored", "C2PA-TRUST-LIST.pem"),
                ("tsa", "vendored", "C2PA-TSA-TRUST-LIST.pem"),
                ("eku_policy", "vendored", "C2PA-EKU-CONFIG.cfg"),
            ]
        );
        for t in trust {
            assert!(is_hex(&t["sha256"], 64), "{t}");
            assert!(t["version"].as_str().is_some_and(|s| !s.is_empty()), "{t}");
        }
        assert!(trust[0]["certificates"].is_u64());
        assert!(trust[2].get("certificates").is_none());
    } else {
        assert!(trust.is_empty(), "no list is loaded without the feature");
    }
    s.assert_no_path(&v["run"]);
    s.assert_no_path(&v["tool"]);
}

#[test]
fn the_options_given_are_the_options_recorded() {
    let s = Scene::new("options");
    let db = s.dir.join("extra-writers.json");
    let text = halftone_container::fingerprints::FingerprintDb::empty().to_json();
    std::fs::write(&db, &text).unwrap();
    let v = &s.inspect(&[
        "--only",
        "container",
        "--reveal",
        "--trust-list",
        "none",
        "--fingerprints",
        db.to_str().unwrap(),
        &s.image,
    ])[0];
    let run = &v["run"];
    assert_eq!(run["options"]["layers"], json!(["container"]));
    assert_eq!(run["options"]["reveal"], true);
    assert_eq!(run["options"]["trust_list"], "none");
    // The extra database is named without its directory and identified by content.
    assert_eq!(
        run["options"]["fingerprints"],
        json!({
            "name": "extra-writers.json",
            "sha256": halftone_core::sha256_hex(text.as_bytes()),
        })
    );
    // The manifest layer did not run: no list was loaded, and none is named.
    assert_eq!(run["trust"], json!([]));
    s.assert_no_path(run);
}

#[test]
fn a_batch_states_the_run_once_and_each_inspection_keeps_its_own() {
    let s = Scene::new("batch");
    let second = s.dir.join("second.png");
    std::fs::copy(&s.image, &second).unwrap();
    let docs = s.inspect(&[
        "--batch",
        "--only",
        "container",
        &s.image,
        second.to_str().unwrap(),
    ]);
    assert_eq!(docs.len(), 1, "a batch is one document");
    let b = &docs[0];
    assert_eq!(b["schema_version"], "1.5.0");
    assert!(b["run"]["options"].is_object(), "{}", b["run"]);
    for i in b["inspections"].as_array().unwrap() {
        assert_eq!(i["run"], b["run"]);
        assert_eq!(i["tool"], b["tool"]);
    }
    assert!(b["tool"]["build"]["features"].is_array());
}

#[test]
fn a_trust_list_that_cannot_be_loaded_is_a_named_gap_without_a_path() {
    if !cfg!(feature = "c2pa") {
        return; // without the feature no list is loaded, so none can fail
    }
    let s = Scene::new("gap");
    let missing = s.dir.join("no-such-anchors.pem");
    let v = &s.inspect(&["--trust-anchors", missing.to_str().unwrap(), &s.image])[0];
    let run = &v["run"];
    assert_eq!(run["trust"], json!([]));
    let gaps = run["gaps"].as_array().unwrap();
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert!(gaps[0].as_str().unwrap().starts_with("trust: "), "{gaps:?}");
    s.assert_no_path(run);
    // The source that needed the list still says what went wrong, in its own entry.
    let c2pa = v["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["source"]["name"] == "c2pa")
        .unwrap();
    assert_eq!(c2pa["status"], "inconclusive");
}

#[test]
fn the_report_shows_the_file_and_the_conditions_without_a_path() {
    let s = Scene::new("report");
    s.inspect(&["--report", "--reveal", "--only", "container", &s.image]);
    let html = std::fs::read_to_string(s.dir.join("grey.halftone.html")).unwrap();
    // The file by name, type and size, at the top.
    assert!(
        html.contains("<tr><th>Name</th><td>grey.png</td></tr>"),
        "{html}"
    );
    assert!(html.contains("<tr><th>Type</th><td>image/png</td></tr>"));
    // What changes how the evidence reads comes before the evidence.
    let pos = |needle: &str| {
        html.find(needle)
            .unwrap_or_else(|| panic!("{needle} not in {html}"))
    };
    assert!(pos("<li>Values are shown in full.") < pos("<h2>Evidence</h2>"));
    assert!(pos("<li>Not run: Manifest, Mark, Blind.") < pos("<h2>Evidence</h2>"));
    // The whole record below it, collapsed.
    assert!(
        pos("<h2>Evidence</h2>") < pos("<details><summary>Conditions of this check: halftone ")
    );
    assert!(html.contains(
        "<tr><th>Disclosed values</th><td>shown in full (<code>--reveal</code>)</td></tr>"
    ));
    let why = if cfg!(feature = "c2pa") {
        "the manifest layer did not run"
    } else {
        "the tool was built without manifest validation"
    };
    assert!(html.contains(&format!("No trust list was loaded: {why}.")));
    // A report is passed on: no directory of this machine in it.
    assert!(!html.contains(s.dir.to_str().unwrap()), "{html}");
    assert!(!html.contains(std::env::temp_dir().to_str().unwrap()));
}
