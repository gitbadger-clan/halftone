//! The IPTC *Digital Source Type* vocabulary, shared by every layer that reads a
//! `digitalSourceType` declaration: the manifest layer finds it signed inside C2PA
//! assertions, the container layer finds it unsigned in XMP. One table, one
//! [`VOCABULARY_VERSION`], so a report can show both readings side by side and say
//! which term list they were judged against.
//!
//! Terms are interpreted by [`TermKind`]; anything not in [`VOCABULARY`] is reported
//! verbatim by the layers and never interpreted. Bump [`VOCABULARY_VERSION`] whenever
//! the table changes.

use serde::Serialize;

/// Namespace URI of the IPTC Extension schema, where `DigitalSourceType` is defined.
pub const IPTC_EXT_NS: &str = "http://iptc.org/std/Iptc4xmpExt/2008-02-29/";

/// Scheme URI prefix of the IPTC Digital Source Type vocabulary.
pub const DST_SCHEME: &str = "http://cv.iptc.org/newscodes/digitalsourcetype/";

/// Identifier of the vocabulary snapshot compiled into this build. Bump when
/// [`VOCABULARY`] changes so old reports say which list they were judged against.
pub const VOCABULARY_VERSION: &str = "iptc-digitalsourcetype-2024-1";

/// What a vocabulary term says about the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TermKind {
    /// Created with, or including, generative AI ("trained algorithmic media").
    Generative,
    /// A declared non-generative source (capture, film, human edits, pure algorithm…).
    NonGenerative,
    /// A retired term still seen in older files. Interpreted, flagged as retired.
    Retired,
}

/// One entry of the IPTC Digital Source Type vocabulary as known to this build.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Term {
    /// Term code, the last path segment of the concept URI.
    pub code: &'static str,
    /// Short plain-language label.
    pub label: &'static str,
    /// Kind.
    pub kind: TermKind,
}

/// The IPTC Digital Source Type vocabulary snapshot. Unknown codes are reported
/// verbatim and yield `Inconclusive`; this list decides interpretation, not existence.
pub const VOCABULARY: &[Term] = &[
    Term {
        code: "trainedAlgorithmicMedia",
        label: "created using generative AI",
        kind: TermKind::Generative,
    },
    Term {
        code: "compositeWithTrainedAlgorithmicMedia",
        label: "composite including generative AI",
        kind: TermKind::Generative,
    },
    Term {
        code: "digitalCapture",
        label: "original digital capture",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "computationalCapture",
        label: "multi-frame computational capture",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "negativeFilm",
        label: "digitised from negative film",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "positiveFilm",
        label: "digitised from positive film",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "print",
        label: "digitised from a print",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "humanEdits",
        label: "edited by a human",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "compositeCapture",
        label: "composite of captured elements",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "algorithmicallyEnhanced",
        label: "algorithmically enhanced capture",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "dataDrivenMedia",
        label: "data-driven media",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "digitalCreation",
        label: "digital creation by a human",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "virtualRecording",
        label: "recording of a virtual environment",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "compositeSynthetic",
        label: "composite including synthetic elements",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "algorithmicMedia",
        label: "pure algorithmic media, not trained on data",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "screenCapture",
        label: "screen capture",
        kind: TermKind::NonGenerative,
    },
    Term {
        code: "digitalArt",
        label: "digital art (retired; now digitalCreation)",
        kind: TermKind::Retired,
    },
    Term {
        code: "minorHumanEdits",
        label: "minor human edits (retired; now humanEdits)",
        kind: TermKind::Retired,
    },
];

/// Look a term code up in [`VOCABULARY`].
pub fn lookup(code: &str) -> Option<&'static Term> {
    VOCABULARY.iter().find(|t| t.code == code)
}

/// Term code of a declared value: the last path segment of a concept URI, or the
/// value itself when it is already a bare term. Trailing slashes are ignored.
pub fn code_of(raw: &str) -> String {
    raw.trim()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocabulary_is_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for t in VOCABULARY {
            assert!(seen.insert(t.code), "duplicate term {}", t.code);
            assert!(!t.label.is_empty());
        }
        assert_eq!(
            lookup("trainedAlgorithmicMedia").map(|t| t.kind),
            Some(TermKind::Generative)
        );
        assert!(lookup("nope").is_none());
    }

    #[test]
    fn code_of_handles_uri_and_bare_forms() {
        assert_eq!(
            code_of("http://cv.iptc.org/newscodes/digitalsourcetype/digitalCapture"),
            "digitalCapture"
        );
        assert_eq!(
            code_of("https://cv.iptc.org/newscodes/digitalsourcetype/print/"),
            "print"
        );
        assert_eq!(code_of(" humanEdits "), "humanEdits");
        assert_eq!(code_of(""), "");
    }
}
