//! File-format reading shared by Halftone's evidence layers. Pure parsing of
//! hostile bytes: no I/O, no verdicts, never panics. Each layer interprets what it
//! reads here in its own terms, so two layers can share a parser without depending
//! on each other.
//!
//! - [`jpeg`]: the metadata segments before the first scan (XMP, ExtendedXMP, C2PA
//!   JUMBF presence, Photoshop IRB) and the end of the first complete image.
//! - [`gcontainer`]: the Motion Photo / Ultra HDR container directory and its
//!   layout against the file.
//! - [`parts`]: the parts a file declares itself to consist of, as named byte
//!   ranges, independent of which declaration they come from.

pub mod gcontainer;
pub mod jpeg;
pub mod parts;

/// Upper bound on one XMP packet's size, for every container that carries XMP.
pub const MAX_XMP_BYTES: usize = 4 * 1024 * 1024;

/// Group digits in threes: 3296847 -> "3,296,847", as every Halftone rationale
/// writes byte counts.
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
