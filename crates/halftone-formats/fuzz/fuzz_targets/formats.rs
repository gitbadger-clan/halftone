//! Every reader in halftone-formats on raw bytes: never panics, and what it returns
//! keeps the promises its callers rely on.
#![no_main]

use halftone_formats::{gcontainer, jpeg};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(m) = jpeg::metadata(data) {
        // "Incomplete" is only ever said of an ExtendedXMP that was there.
        assert!(!m.xmp_extended_incomplete || m.xmp_extended);
    }
    if let Some(end) = jpeg::primary_end(data) {
        assert!(end <= data.len());
    }
    if let Ok(Some(l)) = gcontainer::analyze(data) {
        // Agreement means exactly: nothing to report.
        assert_eq!(l.agrees(), l.disagreements.is_empty());
        assert!(l.declared_end >= l.primary_end);
        for p in &l.placements {
            assert!(p.present_bytes <= p.length);
        }
    }
    let _ = gcontainer::parse_directory(&String::from_utf8_lossy(data));
});
