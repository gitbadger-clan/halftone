//! `ht inspect --pdf`: a PDF report next to the input, typeset by `ht` itself. With
//! the `pdf` feature the file is written; without it the option says how to get it.
//! The tests make their own input, as in `run_record.rs`.

use std::path::PathBuf;
use std::process::{Command, Output};

/// A fresh directory with one small PNG in it, and an empty data directory.
fn scene(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("ht-pdf-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let image = dir.join("grey.png");
    image::RgbImage::from_pixel(16, 16, image::Rgb([120, 120, 120]))
        .save(&image)
        .unwrap();
    (dir, image)
}

fn inspect(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ht"))
        .env("HALFTONE_HOME", dir.join("home"))
        .arg("inspect")
        .args(args)
        .output()
        .expect("run ht")
}

#[cfg(feature = "pdf")]
#[test]
fn a_pdf_is_written_next_to_the_input_in_either_theme() {
    let (dir, image) = scene("themes");
    let image = image.to_str().unwrap();
    let out = dir.join("grey.halftone.pdf");

    let run = inspect(&dir, &["--pdf", image]);
    assert!(
        matches!(run.status.code(), Some(0 | 2)),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let light = std::fs::read(&out).unwrap();
    assert!(light.starts_with(b"%PDF-"));
    let has =
        |bytes: &[u8], needle: &str| bytes.windows(needle.len()).any(|w| w == needle.as_bytes());
    assert!(has(&light, "Halftone report: grey.png"));
    assert!(has(&light, "<pdfaid:part>2</pdfaid:part>"));
    // A report is passed on: no directory of this machine in it.
    assert!(!has(&light, dir.to_str().unwrap()));

    let run = inspect(&dir, &["--pdf", "--pdf-theme", "dark", image]);
    assert!(matches!(run.status.code(), Some(0 | 2)));
    let dark = std::fs::read(&out).unwrap();
    assert!(dark.starts_with(b"%PDF-"));
    assert_ne!(dark, light);

    // The page and the PDF can be asked for together.
    let run = inspect(&dir, &["--pdf", "--report", image]);
    assert!(matches!(run.status.code(), Some(0 | 2)));
    assert!(dir.join("grey.halftone.html").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(not(feature = "pdf"))]
#[test]
fn without_the_feature_the_option_says_how_to_get_it_and_writes_nothing() {
    let (dir, image) = scene("absent");
    let run = inspect(&dir, &["--pdf", image.to_str().unwrap()]);
    assert_eq!(run.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("built without PDF reports"), "{stderr}");
    assert!(stderr.contains("--features c2pa,pdf"), "{stderr}");
    assert!(run.stdout.is_empty(), "nothing is inspected first");
    assert!(!dir.join("grey.halftone.pdf").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_theme_without_a_pdf_is_a_usage_error() {
    let (dir, image) = scene("usage");
    let run = inspect(&dir, &["--pdf-theme", "dark", image.to_str().unwrap()]);
    assert_eq!(run.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&run.stderr).contains("--pdf"));
    let _ = std::fs::remove_dir_all(&dir);
}
