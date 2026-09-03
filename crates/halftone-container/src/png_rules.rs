//! Rule-based PNG writer recognition, built in — no harvesting needed.
//!
//! PNG carries no quantization tables, so the writer signal is the *ancillary chunk
//! inventory*: which optional chunks a writer emits, in what order, with which text
//! keywords and which ICC profile name. That is a small, readable rule rather than an
//! opaque hash, so the rules live here as data with an honest confidence level.
//! Exact-hash entries harvested from real files complement them in
//! [`crate::fingerprints::FingerprintDb`] (see [`crate::png::PngInfo::fingerprint`]).

use serde::Serialize;

use crate::fingerprints::WriterClass;
use crate::png::PngInfo;

/// How the rule was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Checked against real files from the named writer during development.
    Verified,
    /// From the writer's documented or widely reported layout; not yet checked here.
    Documented,
}

/// One writer rule. All listed conditions must hold.
#[derive(Debug, Clone, Serialize)]
pub struct PngRule {
    /// Writer name as it appears in rationales.
    pub name: &'static str,
    /// Writer class.
    pub class: WriterClass,
    /// Confidence.
    pub confidence: Confidence,
    /// Chunk types that must all be present.
    pub all_of: &'static [&'static str],
    /// Chunk types that must all be absent.
    pub none_of: &'static [&'static str],
    /// At least one of these text keywords must be present (empty = no requirement).
    pub any_text: &'static [&'static str],
    /// If set, the iCCP profile name must contain this (case-insensitive).
    pub icc_contains: Option<&'static str>,
    /// If non-empty, the XMP packet (`iTXt XML:com.adobe.xmp`) must contain at least one.
    pub xmp_contains_any: &'static [&'static str],
    /// What the rule is based on.
    pub notes: &'static str,
}

/// Built-in rules, most specific first. First match wins.
pub const RULES: &[PngRule] = &[
    PngRule {
        name: "ComfyUI (Pillow, workflow/prompt text chunks)",
        class: WriterClass::Generator,
        confidence: Confidence::Verified,
        all_of: &["tEXt"],
        none_of: &["pHYs", "gAMA", "sRGB", "cHRM"],
        any_text: &["workflow", "prompt"],
        icc_contains: None,
        xmp_contains_any: &[],
        notes: "ComfyUI SaveImage node writes the graph JSON into tEXt `workflow`/`prompt` via Pillow.",
    },
    PngRule {
        name: "Stable Diffusion WebUI (Pillow, `parameters` text chunk)",
        class: WriterClass::Generator,
        confidence: Confidence::Verified,
        all_of: &["tEXt"],
        none_of: &["pHYs", "gAMA", "sRGB", "cHRM"],
        any_text: &["parameters"],
        icc_contains: None,
        xmp_contains_any: &[],
        notes: "AUTOMATIC1111/Forge write the generation parameters into tEXt `parameters`.",
    },
    PngRule {
        name: "macOS screenshot (Apple ImageIO, XMP UserComment=Screenshot)",
        class: WriterClass::Screenshot,
        confidence: Confidence::Documented,
        all_of: &["iTXt"],
        none_of: &["gAMA", "sRGB"],
        any_text: &["XML:com.adobe.xmp"],
        icc_contains: None,
        xmp_contains_any: &[">Screenshot<"],
        notes: "Since macOS Monterey, screencapture writes an XMP packet with exif:UserComment = Screenshot, plus pHYs and iCCP (Display P3).",
    },
    PngRule {
        name: "Apple ImageIO (macOS/iOS export or screenshot)",
        class: WriterClass::Screenshot,
        confidence: Confidence::Documented,
        all_of: &["iDOT"],
        none_of: &[],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        notes: "Apple's private iDOT parallel-decode chunk, written by ImageIO on macOS/iOS.",
    },
    PngRule {
        name: "Adobe Photoshop / Lightroom (XMP CreatorTool)",
        class: WriterClass::Editor,
        confidence: Confidence::Documented,
        all_of: &["iTXt"],
        none_of: &["iDOT"],
        any_text: &["XML:com.adobe.xmp"],
        icc_contains: None,
        xmp_contains_any: &["CreatorTool=\"Adobe", "<xmp:CreatorTool>Adobe", "Adobe Photoshop", "Adobe Lightroom"],
        notes: "Adobe writers name themselves in xmp:CreatorTool inside the iTXt XMP packet.",
    },
    PngRule {
        name: "Windows Imaging Component (Snipping Tool, Paint)",
        class: WriterClass::Screenshot,
        confidence: Confidence::Documented,
        all_of: &["sRGB", "gAMA", "pHYs"],
        none_of: &["iDOT", "tEXt", "iTXt", "iCCP"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        notes: "WIC writes sRGB, gAMA, pHYs and nothing else; libpng-based apps usually add tEXt.",
    },
    PngRule {
        name: "libpng-based application (GIMP, browsers, many tools)",
        class: WriterClass::Library,
        confidence: Confidence::Documented,
        all_of: &["gAMA"],
        none_of: &["iDOT"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        notes: "Default libpng writers emit gAMA (often with sRGB/cHRM/pHYs and tEXt Software).",
    },
    PngRule {
        name: "Pillow default (bare IHDR/IDAT/IEND)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &[],
        none_of: &["pHYs", "gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "tEXt", "iTXt", "zTXt", "eXIf"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        notes: "Pillow writes no ancillary chunks unless asked; also the layout of many Python pipelines.",
    },
    PngRule {
        name: "Pillow with pHYs (dpi set, no colour chunks)",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &["pHYs"],
        none_of: &["gAMA", "sRGB", "cHRM", "iCCP", "iDOT", "iTXt"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        notes: "Pillow `save(dpi=...)` adds only pHYs; libpng/WIC writers add gAMA/sRGB alongside it.",
    },
    PngRule {
        name: "Pillow with text metadata",
        class: WriterClass::Library,
        confidence: Confidence::Verified,
        all_of: &[],
        none_of: &["pHYs", "gAMA", "sRGB", "cHRM", "iCCP", "iDOT"],
        any_text: &[],
        icc_contains: None,
        xmp_contains_any: &[],
        notes: "Pillow with pnginfo: tEXt/iTXt/zTXt before IDAT, still no colour chunks.",
    },
];

/// Result of matching.
#[derive(Debug, Clone, Serialize)]
pub struct RuleMatch {
    /// Writer name.
    pub name: &'static str,
    /// Class.
    pub class: WriterClass,
    /// Confidence.
    pub confidence: Confidence,
}

/// First rule that matches `info`.
pub fn match_rules(info: &PngInfo) -> Option<RuleMatch> {
    RULES.iter().find(|r| matches(r, info)).map(|r| RuleMatch {
        name: r.name,
        class: r.class,
        confidence: r.confidence,
    })
}

fn matches(r: &PngRule, info: &PngInfo) -> bool {
    if !r.all_of.iter().all(|c| info.has(c)) {
        return false;
    }
    if r.none_of.iter().any(|c| info.has(c)) {
        return false;
    }
    if !r.any_text.is_empty()
        && !r
            .any_text
            .iter()
            .any(|k| info.text.iter().any(|t| t.keyword.eq_ignore_ascii_case(k)))
    {
        return false;
    }
    if !r.xmp_contains_any.is_empty() {
        let xmp = info
            .text
            .iter()
            .find(|t| t.keyword == "XML:com.adobe.xmp")
            .map(|t| t.value.as_str());
        match xmp {
            Some(x) if r.xmp_contains_any.iter().any(|n| x.contains(n)) => {}
            _ => return false,
        }
    }
    if let Some(needle) = r.icc_contains {
        match &info.icc_name {
            Some(n) if n.to_lowercase().contains(&needle.to_lowercase()) => {}
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::png::TextEntry;

    fn info_v(chunks: &[&str], text: &[(&str, &str)]) -> PngInfo {
        PngInfo {
            chunks: chunks.iter().map(|s| s.to_string()).collect(),
            text: text
                .iter()
                .map(|(k, v)| TextEntry {
                    keyword: k.to_string(),
                    value: v.to_string(),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn info(chunks: &[&str], text: &[&str]) -> PngInfo {
        let t: Vec<(&str, &str)> = text.iter().map(|k| (*k, "xxxxxxxxxxxxxxxxxxxx")).collect();
        info_v(chunks, &t)
    }

    fn name(i: &PngInfo) -> Option<&'static str> {
        match_rules(i).map(|m| m.name)
    }

    #[test]
    fn rules_pick_expected_writers() {
        assert_eq!(
            name(&info(&["IHDR", "IDAT", "IEND"], &[])),
            Some("Pillow default (bare IHDR/IDAT/IEND)")
        );
        assert!(
            name(&info(&["IHDR", "tEXt", "IDAT", "IEND"], &["parameters"]))
                .unwrap()
                .starts_with("Stable Diffusion")
        );
        assert!(name(&info(
            &["IHDR", "tEXt", "tEXt", "IDAT", "IEND"],
            &["prompt", "workflow"]
        ))
        .unwrap()
        .starts_with("ComfyUI"));
        assert!(name(&info(
            &["IHDR", "iDOT", "pHYs", "iCCP", "IDAT", "IEND"],
            &[]
        ))
        .unwrap()
        .starts_with("Apple"));
        assert!(name(&info(
            &["IHDR", "sRGB", "gAMA", "pHYs", "IDAT", "IEND"],
            &[]
        ))
        .unwrap()
        .starts_with("Windows"));
        assert!(name(&info(
            &["IHDR", "gAMA", "cHRM", "tEXt", "IDAT", "IEND"],
            &["Software"]
        ))
        .unwrap()
        .starts_with("libpng"));
    }

    #[test]
    fn xmp_content_separates_mac_screenshot_from_photoshop() {
        let mac = info_v(
            &["IHDR", "pHYs", "iCCP", "iTXt", "IDAT", "IEND"],
            &[(
                "XML:com.adobe.xmp",
                "<x:xmpmeta><exif:UserComment><rdf:Alt><rdf:li>Screenshot</rdf:li></rdf:Alt></exif:UserComment></x:xmpmeta>",
            )],
        );
        assert!(
            name(&mac).unwrap().starts_with("macOS screenshot"),
            "{:?}",
            name(&mac)
        );
        let ps = info_v(
            &["IHDR", "pHYs", "iTXt", "IDAT", "IEND"],
            &[(
                "XML:com.adobe.xmp",
                "<rdf:Description xmp:CreatorTool=\"Adobe Photoshop 26.0\"/>",
            )],
        );
        assert!(name(&ps).unwrap().starts_with("Adobe"), "{:?}", name(&ps));
        // An XMP packet that names nobody is not evidence of either.
        assert_eq!(
            name(&info(
                &["IHDR", "pHYs", "iTXt", "IDAT", "IEND"],
                &["XML:com.adobe.xmp"]
            )),
            None
        );
    }

    #[test]
    fn every_rule_is_reachable_by_a_synthetic_layout() {
        for r in RULES {
            let mut chunks: Vec<&str> = vec!["IHDR"];
            chunks.extend(r.all_of.iter().copied());
            if !r.any_text.is_empty() && !chunks.contains(&"tEXt") && !chunks.contains(&"iTXt") {
                chunks.push("tEXt");
            }
            chunks.extend(["IDAT", "IEND"]);
            let needle = r
                .xmp_contains_any
                .first()
                .copied()
                .unwrap_or("xxxxxxxxxxxxxxxxxxxx");
            let text: Vec<(&str, &str)> = r.any_text.iter().map(|k| (*k, needle)).collect();
            let i = info_v(&chunks, &text);
            assert!(
                matches(r, &i),
                "rule `{}` cannot match its own layout",
                r.name
            );
        }
    }
}
