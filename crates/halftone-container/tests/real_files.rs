//! Regression tests against real encoder output in `testdata/images/` (Pillow /
//! libjpeg-turbo / libwebp). These guard the coefficient decoder and the verdict logic
//! against the files that unit-test byte-strings cannot represent.

use halftone_container::{double, exif, jpeg, png, webp};
use halftone_core::{Asset, EvidenceSource, Status};
use std::path::PathBuf;

fn fixture(name: &str) -> Asset {
    let p: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "testdata",
        "images",
        name,
    ]
    .iter()
    .collect();
    Asset::from_path(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn decoder_handles_baseline_variants_and_rejects_progressive() {
    for f in [
        "single_q90.jpg",
        "single_q85_opt.jpg",
        "single_q80_rst.jpg",
        "pil_q80.jpg",
    ] {
        let c = jpeg::coeffs::decode(&fixture(f).bytes).unwrap_or_else(|e| panic!("{f}: {e}"));
        let l = c.luma().unwrap();
        assert_eq!((l.blocks_w, l.blocks_h), (40, 30), "{f}");
        // A textured image: essentially no all-zero luma blocks.
        let zero = l
            .blocks
            .iter()
            .filter(|b| b.iter().all(|&v| v == 0))
            .count();
        assert!(zero < 5, "{f}: {zero} empty blocks");
    }
    assert!(jpeg::coeffs::decode(&fixture("single_q88_prog.jpg").bytes)
        .unwrap_err()
        .contains("progressive"));
}

#[test]
fn double_compression_separates_single_from_resaved() {
    let src = double::DoubleCompression::default();
    for f in ["single_q90.jpg", "single_q85_opt.jpg", "single_q80_rst.jpg"] {
        let ev = src.assess(&fixture(f)).unwrap();
        assert_eq!(ev.status, Status::Absent, "{f}: {}", ev.rationale);
        assert!(ev.statistic.unwrap().value < 0.15, "{f}");
    }
    let ev = src.assess(&fixture("double_q75_q90.jpg")).unwrap();
    assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
    assert!(ev.statistic.unwrap().value > 0.5);
    assert_eq!(
        src.assess(&fixture("single_q88_prog.jpg")).unwrap().status,
        Status::Inconclusive
    );
}

#[test]
fn quant_tables_classify_and_fingerprint_real_pillow_output() {
    let src = jpeg::QuantTables::default();
    let ev = src.assess(&fixture("single_q85_opt.jpg")).unwrap();
    assert_eq!(ev.status, Status::Present);
    assert_eq!(ev.details["luma_quality"], 85);
    assert_eq!(ev.details["chroma_quality"], 85);
    assert_eq!(ev.details["huffman"], "optimized");
    assert_eq!(ev.details["subsampling"], "4:2:0");
    // Pillow q=80 is in the built-in DB.
    let ev = src.assess(&fixture("pil_q80.jpg")).unwrap();
    assert_eq!(ev.details["writer_class"], "library", "{}", ev.rationale);
    assert_eq!(ev.details["huffman"], "standard");
}

#[test]
fn exif_consistency_on_real_files() {
    let src = exif::ExifConsistency;
    let ev = src.assess(&fixture("firefly.jpg")).unwrap();
    assert_eq!(ev.status, Status::Present);
    assert!(ev.rationale.contains("Firefly"));
    let ev = src.assess(&fixture("fake_canon.jpg")).unwrap();
    assert_eq!(ev.status, Status::Inconclusive);
    assert_eq!(ev.details["camera_metadata_stripped"], true);
    let ev = src.assess(&fixture("dim_conflict.jpg")).unwrap();
    assert_eq!(ev.status, Status::Inconclusive);
    assert_eq!(ev.details["dimension_conflict"], true);
    assert_eq!(
        ev.details["exif_partial"], true,
        "piexif output is parsed in partial mode"
    );
    assert_eq!(
        src.assess(&fixture("single_q90.jpg")).unwrap().status,
        Status::Absent
    );
}

#[test]
fn png_and_webp_sources_on_real_files() {
    assert_eq!(
        png::PngWriter
            .assess(&fixture("sd_params.png"))
            .unwrap()
            .status,
        Status::Present
    );
    let ev = png::PngWriter.assess(&fixture("comfy.png")).unwrap();
    assert_eq!(ev.status, Status::Present);
    assert!(ev.rationale.contains("ComfyUI"));
    let ev = png::PngWriter.assess(&fixture("plain.png")).unwrap();
    assert_eq!(ev.status, Status::Inconclusive);
    assert_eq!(ev.details["writer_hint"], "minimal_library");
    let ev = webp::WebpWriter.assess(&fixture("pil.webp")).unwrap();
    assert_eq!(ev.status, Status::Inconclusive);
    assert_eq!(ev.details["kind"], "lossy");
}
