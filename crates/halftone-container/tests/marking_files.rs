//! Regression tests for `marking_metadata` against files written by ExifTool 12.76
//! (`testdata/images/dst_*`). Each fixture was produced from a 64×64 Pillow image with
//! `exiftool -o <out> -XMP-iptcExt:DigitalSourceType=<uri> [...]`, so ExifTool's own
//! reading of them is the ground truth these assertions encode.

use halftone_container::marking::MarkingMetadata;
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
fn exiftool_generative_marking_in_all_three_containers() {
    for f in [
        "dst_gen_exiftool.jpg",
        "dst_gen_exiftool.png",
        "dst_gen_exiftool.webp",
    ] {
        let ev = MarkingMetadata.assess(&fixture(f)).unwrap();
        assert_eq!(ev.status, Status::Present, "{f}: {}", ev.rationale);
        assert_eq!(
            ev.details["digital_source_type"][0],
            "trainedAlgorithmicMedia"
        );
        assert_eq!(ev.details["values"][0]["prefix"], "Iptc4xmpExt");
        assert_eq!(ev.details["values"][0]["namespace_bound"], true);
        assert_eq!(ev.details["xmp_packets"], 1);
    }
}

#[test]
fn exiftool_capture_marking_with_iim_block() {
    let ev = MarkingMetadata
        .assess(&fixture("dst_capture_iim_exiftool.jpg"))
        .unwrap();
    assert_eq!(ev.status, Status::Absent, "{}", ev.rationale);
    assert_eq!(ev.details["digital_source_type"][0], "digitalCapture");
    assert_eq!(ev.details["iim"]["originating_program"], "TestCam");
    assert_eq!(ev.details["iim"]["program_version"], "1.0");
}

#[test]
fn exiftool_extended_xmp_is_reassembled() {
    // A >64 KiB packet forces ExifTool to split it into a main packet plus
    // ExtendedXMP segments; the field lives in the extension.
    let ev = MarkingMetadata
        .assess(&fixture("dst_gen_extendedxmp_exiftool.jpg"))
        .unwrap();
    assert_eq!(ev.status, Status::Present, "{}", ev.rationale);
    assert_eq!(
        ev.details["digital_source_type"][0],
        "compositeWithTrainedAlgorithmicMedia"
    );
    assert_eq!(ev.details["xmp_extended"], true);
    assert_eq!(ev.details["xmp_extended_incomplete"], false);
    assert_eq!(ev.details["xmp_packets"], 2);
}

#[test]
fn unmarked_pillow_output_is_absent() {
    // Reuses an existing fixture: plain Pillow JPEG, no XMP at all.
    let ev = MarkingMetadata.assess(&fixture("pil_q80.jpg")).unwrap();
    assert_eq!(ev.status, Status::Absent, "{}", ev.rationale);
    assert_eq!(ev.details["xmp_packets"], 0);
}
