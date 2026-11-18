// crates/halftone-cli/tests/stdout_shape.rs
//! Stdout must stay machine-readable: `--json` emits JSON lines and nothing else.

use std::process::Command;

fn fixture(name: &str) -> String {
    format!(
        "{}/../../testdata/images/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn json_mode_emits_only_json_lines() {
    let out = Command::new(env!("CARGO_BIN_EXE_ht"))
        .args([
            "inspect",
            "--json",
            "--only",
            "container",
            &fixture("comfy.png"),
        ])
        .output()
        .expect("run ht");
    let stderr = String::from_utf8_lossy(&out.stderr);
    // Exit 0 = nothing Present, 2 = something Present; anything else is a crash or bad args.
    assert!(
        matches!(out.status.code(), Some(0 | 2)),
        "ht exited with {:?}\nstderr:\n{stderr}",
        out.status.code()
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.trim().is_empty(), "no output\nstderr:\n{stderr}");
    for (i, line) in stdout.lines().enumerate() {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|e| panic!("stdout line {i} is not JSON: {e}\n{line}"));
    }
}

#[test]
fn batch_json_is_one_object_with_summary_rows() {
    let out = Command::new(env!("CARGO_BIN_EXE_ht"))
        .args([
            "inspect",
            "--json",
            "--batch",
            "--only",
            "container",
            &fixture("comfy.png"),
            &fixture("dst_gen_exiftool.jpg"),
        ])
        .output()
        .expect("run ht");
    assert!(matches!(out.status.code(), Some(0 | 2)));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout.lines().count(),
        1,
        "batch mode emits exactly one line"
    );
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["schema_version"], "1.5.0");
    assert_eq!(v["summary"].as_array().unwrap().len(), 2);
    assert_eq!(v["inspections"].as_array().unwrap().len(), 2);
    assert_eq!(
        v["summary"][1]["marking"]["digital_source_type"][0],
        "trainedAlgorithmicMedia"
    );
}

#[test]
fn table_is_a_header_and_one_line_per_file() {
    let run = |flag: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_ht"))
            .args([
                "inspect",
                flag,
                "--only",
                "container",
                &fixture("comfy.png"),
                &fixture("dst_gen_exiftool.jpg"),
            ])
            .output()
            .expect("run ht");
        // Same exit code as any inspect: 2 because the second file carries the field.
        assert_eq!(out.status.code(), Some(2), "{flag}");
        String::from_utf8(out.stdout).unwrap()
    };

    let tsv = run("--tsv");
    let rows: Vec<Vec<&str>> = tsv.lines().map(|l| l.split('\t').collect()).collect();
    assert_eq!(rows.len(), 3, "header + two files\n{tsv}");
    assert_eq!(
        rows[0],
        [
            "file",
            "manifest",
            "signer",
            "declared",
            "ingredients",
            "actions",
            "xmp"
        ]
    );
    assert!(rows.iter().all(|r| r.len() == 7), "{tsv}");
    assert_eq!(rows[1][0], "comfy.png");
    // The manifest layer did not run (--only container): its columns are empty.
    assert_eq!(
        &rows[2][..6],
        ["dst_gen_exiftool.jpg", "-", "-", "-", "-", "-"]
    );
    assert_eq!(rows[2][6], "trainedAlgorithmicMedia");

    let aligned = run("--table");
    assert_eq!(aligned.lines().count(), 3);
    assert!(!aligned.contains('\t'));
    let header = aligned.lines().next().unwrap();
    let last = aligned.lines().nth(2).unwrap();
    assert_eq!(
        header.find("xmp"),
        last.find("trainedAlgorithmicMedia"),
        "columns line up\n{aligned}"
    );
}

#[test]
fn a_path_that_is_no_image_is_left_out_of_a_listing_and_an_error_alone() {
    // What `dir/*` brings along in a corpus stratum: a JSON file and a directory.
    let dir = std::env::temp_dir().join(format!("ht-left-out-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let notes = dir.join("expectations.json");
    std::fs::write(&notes, "{}").unwrap();
    let notes = notes.to_str().unwrap().to_string();
    let sub = dir.join("sub").to_str().unwrap().to_string();
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_ht"))
            .arg("inspect")
            .args(["--only", "container"])
            .args(args)
            .output()
            .expect("run ht");
        (
            out.status.code(),
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };

    // Among several paths: the images get their table, the others a line on stderr.
    let (code, stdout, stderr) = run(&[
        "--table",
        &notes,
        &fixture("comfy.png"),
        &sub,
        &fixture("dst_gen_exiftool.jpg"),
    ]);
    assert!(matches!(code, Some(0 | 2)), "{code:?}\n{stderr}");
    assert_eq!(
        stdout.lines().count(),
        3,
        "a header and two files\n{stdout}"
    );
    assert!(stderr.contains("expectations.json: left out: "), "{stderr}");
    assert!(stderr.contains("sub: left out: a directory"), "{stderr}");
    assert!(!stdout.contains("expectations.json"));

    // The same in the batch document and in JSON lines: stdout stays parseable.
    let (code, stdout, _) = run(&["--batch", "--json", &notes, &fixture("comfy.png")]);
    assert!(matches!(code, Some(0 | 2)));
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["inspections"].as_array().unwrap().len(), 1);
    let (_, stdout, _) = run(&["--json", &notes, &fixture("comfy.png")]);
    assert_eq!(stdout.lines().count(), 1);

    // Asked for by name, alone, it is the error it always was.
    let (code, stdout, stderr) = run(&["--table", &notes]);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stdout.is_empty());
    assert!(stderr.contains("expectations.json"), "{stderr}");
    // Nothing left to inspect is an error too, not an empty table.
    let (code, stdout, stderr) = run(&["--table", &notes, &sub]);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stdout.is_empty());
    assert!(stderr.contains("none of the 2 paths"), "{stderr}");
    // A path that does not exist is an error among several as well.
    let missing = dir.join("gone.jpg").to_str().unwrap().to_string();
    let (code, _, stderr) = run(&["--table", &fixture("comfy.png"), &missing]);
    assert_eq!(code, Some(1), "{stderr}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn table_and_json_are_refused_together() {
    let out = Command::new(env!("CARGO_BIN_EXE_ht"))
        .args(["inspect", "--table", "--json", &fixture("comfy.png")])
        .output()
        .expect("run ht");
    assert_eq!(out.status.code(), Some(2), "clap usage error");
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot be used with"));
    assert!(out.stdout.is_empty());
}
