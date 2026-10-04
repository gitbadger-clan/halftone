//! Corpus integrity for `corpus/differential/04-generators`: each file is what its
//! name says. The differential test checks Halftone against reference tools; this
//! checks the rows themselves.
//!
//! Structure: names parse as `<generator>__<variant>__<path>__<prompt>__<n>.<ext>`;
//! the extension matches the bytes unless listed; every image is in
//! `expectations.json`; nothing else lives in the stratum.
//!
//! Semantics: files that differ only in the path token (same generator, variant,
//! prompt and n) show the same image; a file listed as `derived` (an upscale) shows
//! the same image as its source; files in different rows of the same prompt do not
//! look alike unless listed; two routes that delivered identical bytes keep one
//! file unless listed (the SOURCE.md policy).
//!
//! Images are compared as 128×128 luma thumbnails by PSNR. Re-encoding, resizing
//! and format changes keep it high; a different generation of the same prompt does
//! not (the 2026-10-01 Firefly variant mix-up scored 9.5 dB).
//! `HALFTONE_CORPUS_REPORT=1` prints every score and does not fail, to calibrate the
//! thresholds against the corpus before trusting them.
//!
//! Exceptions live in `corpus-rules.json` next to the files, each with a reason; an
//! exception naming a file that is not on disk is itself a failure, so the list
//! cannot outlive what it excuses.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use image::{DynamicImage, GrayImage, ImageDecoder, ImageReader, imageops::FilterType};
use serde_json::Value;

const STRATUM: &str = "04-generators";
/// Thumbnail side for comparisons.
const SIDE: u32 = 128;
/// Same generation through another route must score at least this (dB).
const SAME_MIN_PSNR: f64 = 30.0;
/// Rows of the same prompt scoring at least this (dB) are probably one image twice.
const DUP_MIN_PSNR: f64 = 40.0;
/// Relative width/height difference beyond which two files are not the same picture.
const ASPECT_TOL: f64 = 0.02;
/// Non-image files that belong in the stratum. Hidden files are skipped.
const KNOWN_FILES: &[&str] = &[
    "SOURCE.md",
    "expectations.json",
    "corpus-rules.json",
    "_api-log.jsonl",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Name {
    generator: String,
    variant: String,
    path: String,
    prompt: String,
    n: u32,
    ext: String,
}

/// Everything but the path token: files sharing it must show the same image.
type GroupKey = (String, String, String, u32);

impl Name {
    fn group(&self) -> GroupKey {
        (
            self.generator.clone(),
            self.variant.clone(),
            self.prompt.clone(),
            self.n,
        )
    }
}

fn token_ok(t: &str) -> bool {
    !t.is_empty()
        && t.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'.')
}

fn parse(file: &str) -> Result<Name, String> {
    let (stem, ext) = file
        .rsplit_once('.')
        .ok_or_else(|| "no extension".to_string())?;
    let parts: Vec<&str> = stem.split("__").collect();
    let [generator, variant, path, prompt, n] = parts[..] else {
        return Err(format!("{} `__`-separated fields, expected 5", parts.len()));
    };
    for (what, t) in [
        ("generator", generator),
        ("variant", variant),
        ("path", path),
    ] {
        if !token_ok(t) {
            return Err(format!("{what} token {t:?} is not [a-z0-9.-]+"));
        }
    }
    if !matches!(prompt.as_bytes(), [b'p', b'1'..=b'5'] | [b'e', b'1'..=b'3']) {
        return Err(format!("prompt {prompt:?} is not p1–p5 or e1–e3"));
    }
    let n: u32 = n.parse().map_err(|_| format!("n {n:?} is not a number"))?;
    if n == 0 {
        return Err("n starts at 1".into());
    }
    Ok(Name {
        generator: generator.into(),
        variant: variant.into(),
        path: path.into(),
        prompt: prompt.into(),
        n,
        ext: ext.to_ascii_lowercase(),
    })
}

/// Format family of the bytes, by magic number.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpeg")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        Some("heif")
    } else {
        None
    }
}

fn ext_family(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("png"),
        "jpg" | "jpeg" | "jpe" => Some("jpeg"),
        "webp" => Some("webp"),
        "heic" | "heif" | "avif" => Some("heif"),
        _ => None,
    }
}

struct Thumb {
    luma: GrayImage,
    aspect: f64,
}

/// Decode by content (not extension), apply EXIF orientation, shrink to luma.
fn thumb(path: &Path) -> Result<Thumb, String> {
    let err = |e: image::ImageError| e.to_string();
    let reader = ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(err)?;
    let orientation = decoder.orientation().map_err(err)?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(err)?;
    img.apply_orientation(orientation);
    let aspect = f64::from(img.width()) / f64::from(img.height());
    let luma = image::imageops::resize(&img.to_luma8(), SIDE, SIDE, FilterType::Triangle);
    Ok(Thumb { luma, aspect })
}

fn psnr(a: &GrayImage, b: &GrayImage) -> f64 {
    let sse: f64 = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(&x, &y)| {
            let d = f64::from(x) - f64::from(y);
            d * d
        })
        .sum();
    let mse = sse / f64::from(SIDE * SIDE);
    if mse <= 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0_f64 * 255.0 / mse).log10()
    }
}

fn aspect_close(a: f64, b: f64) -> bool {
    (a - b).abs() <= ASPECT_TOL * a.max(b)
}

/// `corpus-rules.json`: every entry carries its reason (or, for `derived`, its source).
#[derive(Default)]
struct Rules {
    /// file → reason: extension and bytes disagree because that is how it was delivered.
    extension_mismatch: BTreeMap<String, String>,
    /// file → reason: differs from the rest of its group on purpose (a crop, an edit).
    known_different: BTreeMap<String, String>,
    /// "a | b" (sorted) → reason: two rows, one image, kept deliberately.
    known_duplicates: BTreeMap<String, String>,
    /// file → reason: byte-identical to another route of its group, kept anyway.
    identical_kept: BTreeMap<String, String>,
    /// derived file → source file: must show the same image (an upscale of it).
    derived: BTreeMap<String, String>,
}

fn load_rules(dir: &Path) -> Rules {
    let Ok(text) = std::fs::read_to_string(dir.join("corpus-rules.json")) else {
        return Rules::default();
    };
    let v: Value = serde_json::from_str(&text).expect("corpus-rules.json is not valid JSON");
    let map = |k: &str| -> BTreeMap<String, String> {
        v[k].as_object()
            .map(|o| {
                o.iter()
                    .map(|(a, b)| (a.clone(), b.as_str().unwrap_or_default().to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    Rules {
        extension_mismatch: map("extension_mismatch"),
        known_different: map("known_different"),
        known_duplicates: map("known_duplicates"),
        identical_kept: map("identical_kept"),
        derived: map("derived"),
    }
}

fn pair_key(a: &str, b: &str) -> String {
    if a <= b {
        format!("{a} | {b}")
    } else {
        format!("{b} | {a}")
    }
}

struct Entry {
    file: String,
    name: Name,
    sha: Option<String>,
}

#[derive(Debug)]
struct Row {
    file: String,
    check: &'static str,
    detail: String,
}

fn row(file: &str, check: &'static str, detail: impl Into<String>) -> Row {
    Row {
        file: file.to_string(),
        check,
        detail: detail.into(),
    }
}

fn render(rows: &[Row]) -> String {
    let mut s = String::from("| file | check | detail |\n|---|---|---|\n");
    for r in rows {
        s.push_str(&format!("| {} | {} | {} |\n", r.file, r.check, r.detail));
    }
    s
}

/// Same corpus root as the differential test.
fn stratum_dir() -> Option<PathBuf> {
    let base = std::env::var_os("HALFTONE_DIFFERENTIAL_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/differential"));
    let dir = if base.ends_with(STRATUM) {
        base
    } else {
        base.join(STRATUM)
    };
    dir.is_dir().then_some(dir)
}

/// Compare `b` against `a` as "the same picture"; push a row if it is not.
fn compare_same(
    a: &str,
    b: &str,
    thumbs: &HashMap<&str, Thumb>,
    report: bool,
    check: &'static str,
    rows: &mut Vec<Row>,
) {
    let (Some(x), Some(y)) = (thumbs.get(a), thumbs.get(b)) else {
        return; // undecodable: already reported
    };
    let score = psnr(&x.luma, &y.luma);
    let aspect = aspect_close(x.aspect, y.aspect);
    if report {
        println!(
            "{check:<10} {score:6.1} dB  aspect {:<7}  {a}  ~  {b}",
            if aspect { "same" } else { "DIFFERS" }
        );
    }
    if !aspect {
        rows.push(row(
            b,
            check,
            format!(
                "aspect {:.3} vs {:.3} for {a}: a crop or another image",
                y.aspect, x.aspect
            ),
        ));
    } else if score < SAME_MIN_PSNR {
        rows.push(row(
            b,
            check,
            format!(
                "{score:.1} dB against {a} (< {SAME_MIN_PSNR}): another generation under this name?"
            ),
        ));
    }
}

#[test]
#[ignore = "needs corpus/differential/04-generators on disk; run by preflight or the nightly runner"]
fn corpus_files_are_what_their_names_say() {
    let Some(dir) = stratum_dir() else {
        eprintln!("corpus: {STRATUM} not on disk; skipped");
        return;
    };
    let report = std::env::var_os("HALFTONE_CORPUS_REPORT").is_some();
    let rules = load_rules(&dir);
    let exp: Value = std::fs::read_to_string(dir.join("expectations.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    let collected = exp["files"].as_object();
    let mut rows: Vec<Row> = Vec::new();

    // ---- structure ---------------------------------------------------------------
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read stratum")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    let mut entries: Vec<Entry> = Vec::new();
    for p in &paths {
        let file = p
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        if file.starts_with('.') || KNOWN_FILES.contains(&file.as_str()) {
            continue;
        }
        let name = match parse(&file) {
            Ok(n) => n,
            Err(e) => {
                rows.push(row(&file, "name", e));
                continue;
            }
        };
        let Some(family) = ext_family(&name.ext) else {
            rows.push(row(
                &file,
                "stray",
                format!(".{} is not an image extension", name.ext),
            ));
            continue;
        };
        let bytes = std::fs::read(p).expect("read file");
        match sniff(&bytes) {
            None => rows.push(row(
                &file,
                "format",
                "bytes are not PNG, JPEG, WebP or HEIF",
            )),
            Some(actual) if actual != family && !rules.extension_mismatch.contains_key(&file) => {
                rows.push(row(
                    &file,
                    "extension",
                    format!(
                        ".{} holds {actual} bytes; list under extension_mismatch if delivered that way",
                        name.ext
                    ),
                ));
            }
            _ => {}
        }
        let entry = collected.and_then(|o| o.get(&file));
        if entry.is_none() {
            rows.push(row(
                &file,
                "expectations",
                "not in expectations.json; re-run scripts/differential.py",
            ));
        }
        let sha = entry.and_then(|e| e["sha256"].as_str()).map(str::to_string);
        entries.push(Entry { file, name, sha });
    }

    // Exceptions must point at files that exist.
    let on_disk: BTreeSet<&str> = entries.iter().map(|e| e.file.as_str()).collect();
    let listed: BTreeSet<&str> = rules
        .extension_mismatch
        .keys()
        .chain(rules.known_different.keys())
        .chain(rules.identical_kept.keys())
        .chain(rules.derived.keys())
        .chain(rules.derived.values())
        .map(String::as_str)
        .chain(rules.known_duplicates.keys().flat_map(|k| k.split(" | ")))
        .collect();
    for f in listed {
        if !on_disk.contains(f) {
            rows.push(row(
                f,
                "rules",
                "corpus-rules.json names a file that is not on disk",
            ));
        }
    }

    // ---- semantics -----------------------------------------------------------------
    let mut thumbs: HashMap<&str, Thumb> = HashMap::new();
    for e in &entries {
        match thumb(&dir.join(&e.file)) {
            Ok(t) => {
                thumbs.insert(e.file.as_str(), t);
            }
            Err(err) => rows.push(row(&e.file, "decode", err)),
        }
    }

    // Same group (only the path token differs): same picture, and not the same bytes
    // twice unless kept on purpose.
    let mut groups: BTreeMap<GroupKey, Vec<&Entry>> = BTreeMap::new();
    for e in &entries {
        groups.entry(e.name.group()).or_default().push(e);
    }
    for members in groups.values() {
        if members.len() < 2 {
            continue;
        }
        let Some(reference) = members
            .iter()
            .find(|e| !rules.known_different.contains_key(&e.file))
        else {
            continue;
        };
        for e in members {
            if e.file == reference.file {
                continue;
            }
            if let (Some(a), Some(b)) = (&reference.sha, &e.sha)
                && a == b
                && !rules.identical_kept.contains_key(&e.file)
            {
                rows.push(row(
                    &e.file,
                    "identical",
                    format!(
                        "byte-identical to {}: keep one file and say so in SOURCE.md, or list under identical_kept",
                        reference.file
                    ),
                ));
            }
            if !rules.known_different.contains_key(&e.file) {
                compare_same(
                    &reference.file,
                    &e.file,
                    &thumbs,
                    report,
                    "same-image",
                    &mut rows,
                );
            }
        }
    }

    // Declared derivations (upscales) must still be the same picture.
    for (derived, source) in &rules.derived {
        compare_same(source, derived, &thumbs, report, "derived", &mut rows);
    }

    // Different rows of the same prompt must not be the same picture.
    // A derived file (an upscale) may look like every route of its source's row.
    let source_group: HashMap<&str, GroupKey> = rules
        .derived
        .iter()
        .filter_map(|(d, s)| {
            entries
                .iter()
                .find(|e| e.file == *s)
                .map(|e| (d.as_str(), e.name.group()))
        })
        .collect();
    let linked = |a: &Entry, b: &Entry| {
        source_group.get(a.file.as_str()) == Some(&b.name.group())
            || source_group.get(b.file.as_str()) == Some(&a.name.group())
    };
    let mut cross: Vec<(f64, String)> = Vec::new();
    for (i, a) in entries.iter().enumerate() {
        for b in &entries[i + 1..] {
            if a.name.group() == b.name.group() || a.name.prompt != b.name.prompt {
                continue;
            }
            let (Some(x), Some(y)) = (thumbs.get(a.file.as_str()), thumbs.get(b.file.as_str()))
            else {
                continue;
            };
            if !aspect_close(x.aspect, y.aspect) {
                continue;
            }
            let score = psnr(&x.luma, &y.luma);
            let key = pair_key(&a.file, &b.file);
            if score >= DUP_MIN_PSNR && !linked(a, b) && !rules.known_duplicates.contains_key(&key)
            {
                rows.push(row(
                    &b.file,
                    "duplicate",
                    format!(
                        "{score:.1} dB against {} in another row: one image under two names?",
                        a.file
                    ),
                ));
            }
            cross.push((score, key));
        }
    }

    println!(
        "corpus[{STRATUM}]: {} files, {} groups with more than one route, {} problems",
        entries.len(),
        groups.values().filter(|m| m.len() > 1).count(),
        rows.len()
    );
    if report {
        cross.sort_by(|a, b| b.0.total_cmp(&a.0));
        println!("highest cross-row scores (same prompt, same aspect):");
        for (score, key) in cross.iter().take(15) {
            println!("cross-row  {score:6.1} dB  {key}");
        }
        if !rows.is_empty() {
            println!("{}", render(&rows));
        }
        return;
    }
    assert!(
        rows.is_empty(),
        "{} problem(s):\n{}",
        rows.len(),
        render(&rows)
    );
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn parses_the_naming_scheme() {
        let n = parse("adobe-firefly__gpt-image-2.5-flare__web-download__p1__1.png").unwrap();
        assert_eq!(n.generator, "adobe-firefly");
        assert_eq!(n.variant, "gpt-image-2.5-flare");
        assert_eq!(n.path, "web-download");
        assert_eq!((n.prompt.as_str(), n.n, n.ext.as_str()), ("p1", 1, "png"));
    }

    #[test]
    fn rejects_bad_names() {
        assert!(parse("SOURCE.md").is_err());
        assert!(parse("a__b__c__p6__1.png").is_err());
        assert!(parse("a__b__Web-Download__p1__1.png").is_err());
        assert!(parse("a__b__c__p1__0.png").is_err());
        assert!(parse("a__b__c__d__p1__1.png").is_err());
    }

    #[test]
    fn groups_ignore_only_the_path() {
        let a = parse("meta-ai__instant__web-download__p1__1.jpg").unwrap();
        let b = parse("meta-ai__instant__app-save-open__p1__1.webp").unwrap();
        let c = parse("meta-ai__thinking__web-download__p1__1.jpg").unwrap();
        assert_eq!(a.group(), b.group());
        assert_ne!(a.group(), c.group());
    }

    #[test]
    fn sniffs_formats() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0]), Some("jpeg"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff(b"GIF89a"), None);
        assert_eq!(
            ext_family("JPG".to_ascii_lowercase().as_str()),
            Some("jpeg")
        );
    }

    #[test]
    fn psnr_bounds() {
        let black = GrayImage::from_pixel(SIDE, SIDE, image::Luma([0]));
        let white = GrayImage::from_pixel(SIDE, SIDE, image::Luma([255]));
        assert!(psnr(&black, &black).is_infinite());
        assert!(psnr(&black, &white).abs() < 1e-9);
    }

    #[test]
    fn pair_keys_ignore_order() {
        assert_eq!(pair_key("b", "a"), pair_key("a", "b"));
    }
}
