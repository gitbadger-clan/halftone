//! A declared length is never an allocation size: hostile ExtendedXMP headers
//! (found by the formats fuzz target) must not make the reader allocate more than
//! the XMP limit, whatever they declare.

// Observing allocation sizes takes a counting global allocator, and implementing
// `GlobalAlloc` is unsafe by definition. Confined to this test binary: every
// unsafe block forwards to `System` unchanged and only records the size.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Largest;
static LARGEST: AtomicUsize = AtomicUsize::new(0);

// SAFETY: delegates every call to the system allocator unchanged; it only records
// the largest size requested.
unsafe impl GlobalAlloc for Largest {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LARGEST.fetch_max(l.size(), Ordering::Relaxed);
        // SAFETY: same layout, forwarded as received.
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        LARGEST.fetch_max(l.size(), Ordering::Relaxed);
        // SAFETY: same layout, forwarded as received.
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: `p` was allocated by `System` with this layout.
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static ALLOC: Largest = Largest;

fn ext_chunk(full: u32, off: u32, data: &[u8]) -> Vec<u8> {
    let mut p = b"http://ns.adobe.com/xmp/extension/\0".to_vec();
    p.extend_from_slice(&[b'G'; 32]);
    p.extend_from_slice(&full.to_be_bytes());
    p.extend_from_slice(&off.to_be_bytes());
    p.extend_from_slice(data);
    let mut s = vec![0xFF, 0xE1];
    s.extend_from_slice(&u16::try_from(p.len() + 2).unwrap().to_be_bytes());
    s.extend(p);
    s
}

#[test]
fn a_huge_declared_extended_xmp_length_costs_no_allocation() {
    // The fuzz finding: one chunk declaring 0xFFFFFFC8 bytes in total.
    for full in [
        0xFFFF_FFC8_u32,
        0xFFFF_FFFF,
        (halftone_formats::MAX_XMP_BYTES + 1) as u32,
    ] {
        let mut f = vec![0xFF, 0xD8];
        f.extend(ext_chunk(full, 0, b"<x:ext>aaaa"));
        f.extend_from_slice(&[0xFF, 0xD9]);
        LARGEST.store(0, Ordering::Relaxed);
        let m = halftone_formats::jpeg::metadata(&f).unwrap();
        let largest = LARGEST.load(Ordering::Relaxed);
        assert!(
            m.xmp_extended && m.xmp_extended_incomplete,
            "declared {full:#x}"
        );
        assert!(m.xmp_packets.is_empty());
        assert!(
            largest <= halftone_formats::MAX_XMP_BYTES,
            "declared {full:#x}: allocated {largest} bytes"
        );
    }
}

#[test]
fn a_declared_length_the_chunks_do_not_cover_costs_no_allocation() {
    // Within the limit, but only 4 bytes of 4 MiB arrive: nothing to assemble.
    let mut f = vec![0xFF, 0xD8];
    f.extend(ext_chunk(
        halftone_formats::MAX_XMP_BYTES as u32,
        0,
        b"abcd",
    ));
    f.extend_from_slice(&[0xFF, 0xD9]);
    LARGEST.store(0, Ordering::Relaxed);
    let m = halftone_formats::jpeg::metadata(&f).unwrap();
    assert!(m.xmp_extended_incomplete);
    assert!(
        LARGEST.load(Ordering::Relaxed) < 64 * 1024,
        "allocated for a packet that was never complete"
    );
}
