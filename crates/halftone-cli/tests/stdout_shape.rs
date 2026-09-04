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
