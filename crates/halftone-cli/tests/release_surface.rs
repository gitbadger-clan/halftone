//! What `ht` offers, and what it says about what it does not offer.
//!
//! This version has sources in two of the four layers. These tests hold the surface
//! to that: no command or source that does nothing, a sentence wherever a reader
//! could take an empty layer for "nothing found", and a `packs update` that says what
//! works before it touches the network. They make their own files and their own empty
//! data directory.

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

const NO_SOURCE: &str = "No source in this build: Mark, Blind. No watermark was looked for and \
no classifier was run; a layer with no source says nothing about the file.";

struct Scene {
    dir: PathBuf,
    image: String,
    home: PathBuf,
}

impl Scene {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ht-surface-{name}-{}", std::process::id()));
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

    fn ht(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ht"))
            .env("HALFTONE_HOME", &self.home)
            .env_remove("HALFTONE_PUBLISHER_KEYS")
            .args(args)
            .output()
            .expect("run ht")
    }

    fn stdout(&self, args: &[&str]) -> String {
        String::from_utf8(self.ht(args).stdout).unwrap()
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn there_is_no_sign_command() {
    let s = Scene::new("sign");
    let help = s.stdout(&["--help"]);
    assert!(
        help.lines().any(|l| l.trim_start().starts_with("inspect ")),
        "{help}"
    );
    assert!(
        !help.lines().any(|l| l.trim_start().starts_with("sign")),
        "{help}"
    );
    let out = s.ht(&["sign", &s.image, "--key", "k.pem"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("unrecognized subcommand"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn every_registered_source_is_in_a_layer_that_does_something() {
    let s = Scene::new("sources");
    let listed = s.stdout(&["sources"]);
    assert!(listed.lines().count() >= 10, "{listed}");
    for line in listed.lines() {
        let layer = line.split('\t').next().unwrap();
        assert!(matches!(layer, "Manifest" | "Container"), "{line}");
    }
}

#[test]
fn an_inspection_has_no_row_in_a_layer_without_a_source_and_says_which() {
    let s = Scene::new("rows");
    let v: Value = serde_json::from_str(s.stdout(&["inspect", "--json", &s.image]).trim()).unwrap();
    // Asked for by default, and recorded as asked for.
    assert_eq!(
        v["run"]["options"]["layers"],
        serde_json::json!(["manifest", "container", "mark", "blind"])
    );
    let layers: Vec<&str> = v["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["layer"].as_str().unwrap())
        .collect();
    assert!(layers.contains(&"manifest") && layers.contains(&"container"));
    assert!(
        !layers.contains(&"mark") && !layers.contains(&"blind"),
        "{layers:?}"
    );

    // The plain listing: once, after the rows.
    let plain = s.stdout(&["inspect", &s.image, &s.image]);
    assert_eq!(plain.matches(NO_SOURCE).count(), 1, "{plain}");
    assert!(plain.trim_end().ends_with(NO_SOURCE), "{plain}");
    // The batch matrix: under its counts, which count only layers that reported.
    let matrix = s.stdout(&["inspect", "--batch", &s.image]);
    assert!(matrix.trim_end().ends_with(NO_SOURCE), "{matrix}");
    assert!(
        !matrix.contains("mark present") && !matrix.contains("blind present"),
        "{matrix}"
    );
    assert!(
        matrix.contains("1 file: manifest present 0, container "),
        "{matrix}"
    );
    // The report page.
    s.ht(&["inspect", "--report", &s.image]);
    let page = std::fs::read_to_string(s.dir.join("grey.halftone.html")).unwrap();
    assert!(
        page.contains(&format!("<p class=\"empty-layers\">{NO_SOURCE}</p>")),
        "{page}"
    );

    // Not asked for is something else: no sentence, and the report's note says "not run".
    let only = s.stdout(&["inspect", "--only", "container", &s.image]);
    assert!(!only.contains("No source in this build"), "{only}");
    // Asked for by name: no rows, and the reason.
    let mark = s.stdout(&["inspect", "--only", "mark", &s.image]);
    assert!(
        mark.trim_end().ends_with(
            "No source in this build: Mark. No watermark was looked for; a layer with no \
             source says nothing about the file."
        ),
        "{mark}"
    );
    // The named columns never had one for these layers.
    assert!(
        !s.stdout(&["inspect", "--table", &s.image])
            .contains("No source")
    );
}

#[test]
fn packs_update_without_a_publisher_key_says_what_works_and_fetches_nothing() {
    let s = Scene::new("packs");
    // Port 9 (discard) on this machine: a request would fail in its own words.
    let out = s.ht(&[
        "packs",
        "update",
        "--index-url",
        "http://127.0.0.1:9/index.json",
    ]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let said = stderr(&out);
    assert!(said.contains("no publisher key is configured"), "{said}");
    assert!(said.contains("was not fetched"), "{said}");
    assert!(said.contains("`ht packs update --upstream`"), "{said}");
    assert!(!said.contains("127.0.0.1"), "a request was made: {said}");
    // Nothing was written.
    assert!(std::fs::read_dir(&s.home).unwrap().next().is_none());

    let listed = s.stdout(&["packs", "list"]);
    assert!(
        listed.contains(
            "nothing installed; `ht packs update --upstream` installs the C2PA trust lists"
        ),
        "{listed}"
    );
}
