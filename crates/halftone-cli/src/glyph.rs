//! Verdict glyphs for the CLI renderer. Shape carries the meaning so the table
//! reads the same in a monochrome terminal, a log file and the HTML report.
//!
//! Drop into `halftone-cli` (or `halftone-core` if the report crate wants it too)
//! and call `status.glyph(Glyphs::detect())` from the human formatter.

use halftone_core::Status;

/// Which character set the terminal can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyphs {
    /// UTF-8 terminal: ● ○ ◌ –
    Unicode,
    /// Plain ASCII fallback: * o ? -
    Ascii,
}

impl Glyphs {
    /// Unicode unless `HALFTONE_ASCII` is set or the locale is not UTF-8.
    pub fn detect() -> Self {
        if std::env::var_os("HALFTONE_ASCII").is_some() {
            return Self::Ascii;
        }
        let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.is_empty())
            .map(|v| v.to_ascii_lowercase().contains("utf"))
            .unwrap_or(cfg!(not(windows)));
        if utf8 {
            Self::Unicode
        } else {
            Self::Ascii
        }
    }
}

/// Glyph for a [`Status`], mirroring the SVG verdict set:
/// solid dot / ring / halftone dot / dash.
pub trait StatusGlyph {
    fn glyph(self, set: Glyphs) -> &'static str;
}

impl StatusGlyph for Status {
    fn glyph(self, set: Glyphs) -> &'static str {
        match (set, self) {
            (Glyphs::Unicode, Status::Present) => "●",
            (Glyphs::Unicode, Status::Absent) => "○",
            (Glyphs::Unicode, Status::Inconclusive) => "◌",
            (Glyphs::Unicode, Status::NotApplicable) => "–",
            (Glyphs::Ascii, Status::Present) => "*",
            (Glyphs::Ascii, Status::Absent) => "o",
            (Glyphs::Ascii, Status::Inconclusive) => "?",
            (Glyphs::Ascii, Status::NotApplicable) => "-",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_has_a_distinct_glyph_in_both_sets() {
        for set in [Glyphs::Unicode, Glyphs::Ascii] {
            let g: std::collections::HashSet<_> = [
                Status::Present,
                Status::Absent,
                Status::Inconclusive,
                Status::NotApplicable,
            ]
            .map(|s| s.glyph(set))
            .into();
            assert_eq!(g.len(), 4);
        }
    }

    #[test]
    fn ascii_set_is_ascii() {
        for s in [
            Status::Present,
            Status::Absent,
            Status::Inconclusive,
            Status::NotApplicable,
        ] {
            assert!(s.glyph(Glyphs::Ascii).is_ascii());
        }
    }
}
