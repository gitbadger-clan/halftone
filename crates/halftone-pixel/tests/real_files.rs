//! Regression tests against real PNG-encoded fixtures in `testdata/images/pixel/`.

use halftone_core::{Asset, EvidenceSource, Status};
use halftone_pixel::LatticeSource;
use std::path::PathBuf;

fn fixture(name: &str) -> Asset {
    let p: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "testdata",
        "images",
        "pixel",
        name,
    ]
    .iter()
    .collect();
    Asset::from_path(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn stat(name: &str) -> (f64, f64) {
    let ev = LatticeSource::default().assess(&fixture(name)).unwrap();
    assert_eq!(
        ev.status,
        Status::Inconclusive,
        "dark mode never verdicts: {}",
        ev.rationale
    );
    (
        ev.statistic.unwrap().value,
        ev.details["consistent_tiles"].as_f64().unwrap(),
    )
}

#[test]
fn separation_on_real_png_fixtures() {
    let (clean, _) = stat("clean.png");
    let (seam, seam_cons) = stat("seam8.png");
    let (jpeg, jpeg_cons) = stat("jpeg_history.png");
    let (nn, _) = stat("nn_upscale.png");
    let (ui, ui_cons) = stat("ui.png");
    assert!(clean < 0.035, "clean {clean:.4}");
    assert!(ui < 0.01 && ui_cons < 0.2, "ui {ui:.4}/{ui_cons:.2}");
    // A weak decoder-like seam is a *marginal* positive: clearly above clean, but
    // exactly the region where only a calibrated threshold can draw the line.
    assert!(
        seam > 0.04 && seam_cons > 0.6,
        "seam {seam:.4}/{seam_cons:.2}"
    );
    assert!(
        jpeg > 0.1 && jpeg_cons > 0.9,
        "jpeg-history {jpeg:.4}/{jpeg_cons:.2}"
    );
    assert!(nn > 0.5, "nn {nn:.4}");
}

#[test]
fn thresholded_mode_verdicts_and_names_all_causes() {
    let src = LatticeSource {
        threshold: Some(0.05),
    };
    let ev = src.assess(&fixture("jpeg_history.png")).unwrap();
    assert_eq!(ev.status, Status::Present);
    assert!(
        ev.rationale.contains("JPEG"),
        "must name the JPEG-history cause: {}",
        ev.rationale
    );
    assert!(ev.rationale.contains("nearest-neighbour"));
    let ev = src.assess(&fixture("clean.png")).unwrap();
    assert_eq!(ev.status, Status::Absent);
    assert!(ev.rationale.contains("resize"));
}
