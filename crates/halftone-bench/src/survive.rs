//! `ht survive`: does the marking still read after this happens to the file?
//!
//! For every input × transform, produce the derivative (in memory), run the
//! registry on it exactly as `ht inspect` would, and record one [`FileRow`] with
//! `source` and `transform` set. Captured files (what WhatsApp, a screenshot or a
//! CDN actually produced) join the same table by name, since a real app cannot be
//! simulated honestly.
//!
//! Performance: inputs are processed in parallel with rayon, each input is read and
//! decoded once, and transforms of the same input run in parallel over the shared
//! decode. Nothing touches the disk unless `keep` is set. The registry is shared
//! read-only across threads (`EvidenceSource: Send + Sync`).
//!
//! The output is a [`Batch`] like any other, so the report and the differential
//! runner read it without special cases; the two tables here are projections.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use halftone_core::{summarize, Asset, Batch, FileRow, Inspection, Registry, ToolInfo};
use rayon::prelude::*;

use crate::distort::{decode, Decoded, Distortion};

/// Error from a survival run.
#[derive(Debug, thiserror::Error)]
pub enum SurviveError {
    /// Reading an input or writing a kept derivative.
    #[error("{path}: {source}")]
    Io {
        /// File involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The input is not a container the suite can transform.
    #[error("{path}: {reason}")]
    Unsupported {
        /// File involved.
        path: PathBuf,
        /// Why.
        reason: String,
    },
}

/// What to run.
#[derive(Debug, Clone)]
pub struct SurviveConfig {
    /// Synthetic transforms, in column order. `Distortion::None` should be first
    /// so every source has a baseline row.
    pub suite: Vec<Distortion>,
    /// Write every derivative here as `<stem>__<transform>.<ext>` so a cell can be
    /// reproduced with other tools. `None` keeps everything in memory.
    pub keep: Option<PathBuf>,
}

impl Default for SurviveConfig {
    fn default() -> Self {
        Self {
            suite: Distortion::survival_suite(),
            keep: None,
        }
    }
}

/// A captured file matched to its source: `<source stem>__<label>.<ext>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    /// Index into the inputs slice.
    pub input: usize,
    /// Route label (`whatsapp_photo`).
    pub label: String,
    /// The file the application produced.
    pub path: PathBuf,
}

/// Match captured files in `dir` to `inputs` by stem prefix: a file named
/// `<stem of input>__<label>.<ext>` belongs to that input with that label. Longer
/// stems win when one input's stem is a prefix of another's. Unmatched files are
/// returned separately so the caller can report them.
pub fn match_captured(inputs: &[PathBuf], dir: &Path) -> (Vec<Captured>, Vec<PathBuf>) {
    let mut stems: Vec<(usize, String)> = inputs
        .iter()
        .enumerate()
        .filter_map(|(i, p)| Some((i, p.file_stem()?.to_string_lossy().into_owned())))
        .collect();
    // Longest stem first so `a__b` is tried before `a`.
    stems.sort_by(|x, y| y.1.len().cmp(&x.1.len()).then(x.0.cmp(&y.0)));
    let mut matched = Vec::new();
    let mut unmatched = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return (matched, unmatched);
    };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    for f in files {
        let Some(stem) = f.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let hit = stems.iter().find_map(|(i, s)| {
            stem.strip_prefix(s.as_str())
                .and_then(|rest| rest.strip_prefix("__"))
                .filter(|label| !label.is_empty())
                .map(|label| (*i, label.to_string()))
        });
        match hit {
            Some((input, label)) => matched.push(Captured {
                input,
                label,
                path: f,
            }),
            None => unmatched.push(f),
        }
    }
    matched.sort_by(|a, b| a.input.cmp(&b.input).then(a.label.cmp(&b.label)));
    (matched, unmatched)
}

/// Run the suite over `inputs` (plus any captured files) and return one batch.
/// Rows are ordered by input, then by suite order, then captured labels.
pub fn run(
    registry: &Registry,
    tool: ToolInfo,
    inputs: &[PathBuf],
    captured: &[Captured],
    cfg: &SurviveConfig,
) -> Result<Batch, SurviveError> {
    if let Some(dir) = &cfg.keep {
        std::fs::create_dir_all(dir).map_err(|e| SurviveError::Io {
            path: dir.clone(),
            source: e,
        })?;
    }
    let needs_decode = cfg.suite.iter().any(Distortion::needs_decode);

    // (input index, column index, inspection, source, transform)
    type Row = (usize, usize, Inspection, String, String);

    let synthetic: Result<Vec<Vec<Row>>, SurviveError> = inputs
        .par_iter()
        .enumerate()
        .map(|(ix, path)| {
            let bytes = std::fs::read(path).map_err(|e| SurviveError::Io {
                path: path.clone(),
                source: e,
            })?;
            let decoded: Option<Decoded> = if needs_decode {
                Some(decode(&bytes).map_err(|e| SurviveError::Unsupported {
                    path: path.clone(),
                    reason: e.to_string(),
                })?)
            } else {
                None
            };
            let source = path.to_string_lossy().into_owned();
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "input".into());
            cfg.suite
                .par_iter()
                .enumerate()
                .map(|(cx, d)| {
                    let name = d.name();
                    let out = d.apply_decoded(decoded.as_ref(), &bytes).map_err(|e| {
                        SurviveError::Unsupported {
                            path: path.clone(),
                            reason: format!("{name}: {e}"),
                        }
                    })?;
                    let derived_path = cfg
                        .keep
                        .as_ref()
                        .map(|dir| dir.join(format!("{stem}__{}.{}", file_token(&name), out.ext)));
                    if let Some(p) = &derived_path {
                        std::fs::write(p, &out.bytes).map_err(|e| SurviveError::Io {
                            path: p.clone(),
                            source: e,
                        })?;
                    }
                    // The asset path is the kept file if any, else a virtual name so
                    // the row still reads in the matrix.
                    let virtual_path = derived_path.unwrap_or_else(|| {
                        PathBuf::from(format!("{stem}__{}.{}", file_token(&name), out.ext))
                    });
                    let asset = Asset::from_bytes(out.bytes, Some(virtual_path)).map_err(|e| {
                        SurviveError::Unsupported {
                            path: path.clone(),
                            reason: format!("{name}: {e}"),
                        }
                    })?;
                    let insp = registry.inspect(&asset, tool.clone());
                    Ok((ix, cx, insp, source.clone(), name))
                })
                .collect::<Result<Vec<Row>, SurviveError>>()
        })
        .collect();
    let mut rows: Vec<Row> = synthetic?.into_iter().flatten().collect();

    // Captured files: read, inspect, label. Their column index continues after the
    // suite so they sort after the synthetic columns of the same input.
    let base = cfg.suite.len();
    let captured_rows: Result<Vec<Row>, SurviveError> = captured
        .par_iter()
        .enumerate()
        .map(|(k, c)| {
            let asset = Asset::from_path(&c.path).map_err(|e| SurviveError::Unsupported {
                path: c.path.clone(),
                reason: e.to_string(),
            })?;
            let insp = registry.inspect(&asset, tool.clone());
            let source = inputs
                .get(c.input)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            Ok((
                c.input,
                base + k,
                insp,
                source,
                Distortion::Captured(c.label.clone()).name(),
            ))
        })
        .collect();
    rows.extend(captured_rows?);
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

    let mut batch = Batch::new(tool, Vec::new());
    for (_, _, insp, source, transform) in rows {
        let mut row: FileRow = summarize(&insp);
        row.source = Some(source);
        row.transform = Some(transform);
        batch.summary.push(row);
        batch.inspections.push(insp);
    }
    Ok(batch)
}

/// Transform name as a file-name token: `captured:whatsapp` → `captured-whatsapp`,
/// `resize_0.5` unchanged.
fn file_token(name: &str) -> String {
    name.replace(':', "-")
}

/// How a marking fared relative to its baseline (the `none` row of the same
/// source).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    /// The source had nothing to lose in this layer.
    NotApplicable,
    /// Still reads as it did on the original.
    Kept,
    /// Still present but no longer validates (manifest) — a broken hard binding.
    Broken,
    /// Gone.
    Stripped,
    /// Something appeared that the original did not have (a writer added marking).
    Gained,
}

impl Fate {
    /// One-character glyph for tables.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::NotApplicable => "·",
            Self::Kept => "✓",
            Self::Broken => "!",
            Self::Stripped => "✗",
            Self::Gained => "+",
        }
    }
}

/// What a row's manifest column amounts to, for comparison against a baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManifestKind {
    None,
    Remote,
    Valid,
    Invalid,
}

fn manifest_kind(r: &FileRow) -> ManifestKind {
    if r.manifest.remote_manifest_url.is_some() {
        return ManifestKind::Remote;
    }
    match r.manifest.validation_state.as_deref() {
        Some("Invalid") => ManifestKind::Invalid,
        Some(_) => ManifestKind::Valid,
        None => ManifestKind::None,
    }
}

/// Fate of the manifest in `row` given the `baseline` row.
pub fn manifest_fate(baseline: &FileRow, row: &FileRow) -> Fate {
    use ManifestKind as K;
    match (manifest_kind(baseline), manifest_kind(row)) {
        (K::None, K::None) => Fate::NotApplicable,
        (K::None, _) => Fate::Gained,
        (b, r) if b == r => Fate::Kept,
        (_, K::Invalid) => Fate::Broken,
        (_, K::None) => Fate::Stripped,
        // remote ↔ valid: a different kind of marking than before; treat as kept
        // only if identical, otherwise changed = broken for table purposes.
        _ => Fate::Broken,
    }
}

/// Fate of the XMP `DigitalSourceType` field in `row` given `baseline`.
pub fn xmp_fate(baseline: &FileRow, row: &FileRow) -> Fate {
    let b = !baseline.marking.digital_source_type.is_empty();
    let r = !row.marking.digital_source_type.is_empty();
    match (b, r) {
        (false, false) => Fate::NotApplicable,
        (false, true) => Fate::Gained,
        (true, true) => {
            if baseline.marking.digital_source_type == row.marking.digital_source_type {
                Fate::Kept
            } else {
                Fate::Broken
            }
        }
        (true, false) => Fate::Stripped,
    }
}

/// One cell of the survival table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// Manifest fate.
    pub manifest: Fate,
    /// XMP field fate.
    pub xmp: Fate,
}

/// Group rows by source, in first-seen order, and compute each row's fate against
/// its source's `none` row. Sources without a `none` row are skipped (no baseline).
pub fn fates(batch: &Batch) -> Vec<(String, Vec<(String, Cell)>)> {
    let mut by_source: BTreeMap<usize, (String, Vec<&FileRow>)> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for r in &batch.summary {
        let Some(src) = &r.source else { continue };
        let idx = match order.iter().position(|s| s == src) {
            Some(i) => i,
            None => {
                order.push(src.clone());
                order.len() - 1
            }
        };
        by_source
            .entry(idx)
            .or_insert_with(|| (src.clone(), Vec::new()))
            .1
            .push(r);
    }
    by_source
        .into_values()
        .filter_map(|(src, rows)| {
            let baseline = rows
                .iter()
                .find(|r| r.transform.as_deref() == Some("none"))?;
            let cells = rows
                .iter()
                .map(|r| {
                    (
                        r.transform.clone().unwrap_or_default(),
                        Cell {
                            manifest: manifest_fate(baseline, r),
                            xmp: xmp_fate(baseline, r),
                        },
                    )
                })
                .collect();
            Some((src, cells))
        })
        .collect()
}

/// Group key for the aggregate table: the corpus naming convention's
/// `<generator>__<variant>` prefix of the file stem, or the whole stem.
pub fn writer_of(source: &str) -> String {
    let stem = Path::new(source)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.to_string());
    let mut parts = stem.split("__");
    match (parts.next(), parts.next()) {
        (Some(a), Some(b)) => format!("{a}__{b}"),
        _ => stem,
    }
}

/// Long table: one line per source × transform with the two columns as read.
pub fn render_long(batch: &Batch) -> String {
    let mut out = String::from("source\ttransform\tmanifest\tsigner\tdeclared\txmp\n");
    for r in &batch.summary {
        let manifest = r
            .manifest
            .validation_state
            .clone()
            .or_else(|| {
                r.manifest
                    .remote_manifest_url
                    .as_ref()
                    .map(|_| "remote".into())
            })
            .unwrap_or_else(|| "-".into());
        let xmp = if r.marking.xmp_packets.unwrap_or(0) > 0 {
            if r.marking.digital_source_type.is_empty() {
                "present".to_string()
            } else {
                r.marking.digital_source_type.join(",")
            }
        } else {
            "none".to_string()
        };
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            r.source
                .as_deref()
                .map(|s| Path::new(s)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| s.to_string()))
                .unwrap_or_default(),
            r.transform.as_deref().unwrap_or(""),
            manifest,
            r.manifest.issuer.as_deref().unwrap_or("-"),
            if r.manifest.digital_source_type.is_empty() {
                "-".to_string()
            } else {
                r.manifest.digital_source_type.join(",")
            },
            xmp,
        ));
    }
    out
}

/// Aggregate table: writer × transform, each cell `M<glyph> X<glyph>`. When the
/// files of one writer disagree, the cell shows the count that kept, e.g. `M2/3`.
pub fn render_aggregate(batch: &Batch) -> String {
    let per_source = fates(batch);
    // Column order: first-seen transform order across all sources.
    let mut columns: Vec<String> = Vec::new();
    for (_, cells) in &per_source {
        for (t, _) in cells {
            if !columns.contains(t) {
                columns.push(t.clone());
            }
        }
    }
    // writer -> transform -> Vec<Cell>
    let mut groups: Vec<(String, BTreeMap<String, Vec<Cell>>)> = Vec::new();
    for (src, cells) in &per_source {
        let w = writer_of(src);
        let entry = match groups.iter_mut().find(|(k, _)| *k == w) {
            Some(e) => e,
            None => {
                groups.push((w, BTreeMap::new()));
                groups.last_mut().expect("just pushed")
            }
        };
        for (t, c) in cells {
            entry.1.entry(t.clone()).or_default().push(*c);
        }
    }
    let name_w = groups
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(6)
        .clamp(6, 48);
    let col_w = columns
        .iter()
        .map(|c| c.chars().count())
        .max()
        .unwrap_or(8)
        .max(8);
    let mut out = String::new();
    out.push_str(&format!("{:<name_w$}", "writer"));
    for c in &columns {
        out.push_str(&format!("  {c:<col_w$}"));
    }
    out.push('\n');
    for (w, by_t) in &groups {
        out.push_str(&format!("{w:<name_w$}"));
        for c in &columns {
            let cell = match by_t.get(c) {
                Some(cells) => summarise(cells),
                None => "-".into(),
            };
            out.push_str(&format!("  {cell:<col_w$}"));
        }
        out.push('\n');
    }
    out.push_str(
        "\nEach cell reads M<fate> X<fate>: M is the C2PA manifest, X the IPTC \
         DigitalSourceType field in XMP. The fate compares the transformed file with \
         the untouched original (the `none` column):\n\
         \x20 ✓  kept      still reads exactly as on the original\n\
         \x20 ✗  stripped  gone after the transform\n\
         \x20 !  broken    still present but changed: a manifest that no longer \
         validates, a remote reference that became embedded, a different term\n\
         \x20 +  gained    appeared although the original had none (a writer added it)\n\
         \x20 ·  n/a       the original carried nothing in this layer, so there was \
         nothing to lose\n\
         \x20 k/n         the writer's files disagree: k of n kept it\n\
         Synthetic transforms (jpeg_*, resize_*, crop_*, png, webp) are clean \
         re-encodes and strip everything by construction; captured:<label> columns \
         are files a real application produced and are where survival is measured.\n",
    );
    out
}

fn summarise(cells: &[Cell]) -> String {
    fn one(fates: impl Iterator<Item = Fate> + Clone) -> String {
        let n = fates.clone().count();
        let first = fates.clone().next().unwrap_or(Fate::NotApplicable);
        if fates.clone().all(|f| f == first) {
            first.glyph().to_string()
        } else {
            let kept = fates.filter(|f| *f == Fate::Kept).count();
            format!("{kept}/{n}")
        }
    }
    format!(
        "M{} X{}",
        one(cells.iter().map(|c| c.manifest)),
        one(cells.iter().map(|c| c.xmp))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use halftone_core::{Evidence, EvidenceSource, Layer, Modality, SourceId, Status};

    /// A stand-in for the two real layers: reports a manifest if the bytes contain
    /// `MANIFEST` and an XMP field if they contain `DigitalSourceType`. Synthetic
    /// transforms re-encode and lose both, which is exactly the property under test.
    struct MarkerSource;
    impl EvidenceSource for MarkerSource {
        fn id(&self) -> SourceId {
            SourceId {
                name: "c2pa".into(),
                version: "test".into(),
            }
        }
        fn layer(&self) -> Layer {
            Layer::Manifest
        }
        fn supports(&self, a: &Asset) -> bool {
            a.modality == Modality::Image
        }
        fn assess(&self, a: &Asset) -> halftone_core::Result<Evidence> {
            let has = a.bytes.windows(8).any(|w| w == b"MANIFEST");
            Ok(Evidence {
                layer: Layer::Manifest,
                source: self.id(),
                status: if has { Status::Present } else { Status::Absent },
                statistic: None,
                calibration: None,
                rationale: String::new(),
                details: if has {
                    serde_json::json!({"validation_state": "Trusted", "issuer": "Test"})
                } else {
                    serde_json::Value::Null
                },
                duration_ms: 0,
            })
        }
    }
    struct XmpSource;
    impl EvidenceSource for XmpSource {
        fn id(&self) -> SourceId {
            SourceId {
                name: "marking_metadata".into(),
                version: "test".into(),
            }
        }
        fn layer(&self) -> Layer {
            Layer::Container
        }
        fn supports(&self, a: &Asset) -> bool {
            a.modality == Modality::Image
        }
        fn assess(&self, a: &Asset) -> halftone_core::Result<Evidence> {
            let has = a.bytes.windows(17).any(|w| w == b"DigitalSourceType");
            Ok(Evidence {
                layer: Layer::Container,
                source: self.id(),
                status: if has { Status::Present } else { Status::Absent },
                statistic: None,
                calibration: None,
                rationale: String::new(),
                details: serde_json::json!({
                    "xmp_packets": if has { 1 } else { 0 },
                    "digital_source_type": if has { vec!["trainedAlgorithmicMedia"] } else { vec![] },
                }),
                duration_ms: 0,
            })
        }
    }

    fn registry() -> Registry {
        Registry::new().with(MarkerSource).with(XmpSource)
    }

    fn tool() -> ToolInfo {
        ToolInfo {
            name: "halftone".into(),
            version: "test".into(),
        }
    }

    /// A PNG carrying both markers in a tEXt chunk.
    fn marked_png() -> Vec<u8> {
        use image::{DynamicImage, ImageBuffer, Rgb};
        let img = DynamicImage::ImageRgb8(ImageBuffer::from_fn(24, 24, |x, y| {
            Rgb([x as u8 * 9, y as u8 * 9, 0])
        }));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let data = b"Comment\0MANIFEST DigitalSourceType".to_vec();
        let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(b"tEXt");
        chunk.extend_from_slice(&data);
        chunk.extend_from_slice(&[0, 0, 0, 0]);
        png.splice(33..33, chunk);
        png
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ht-survive-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn end_to_end_synthetic_transforms_strip_and_the_table_says_so() {
        let dir = tmpdir("e2e");
        let src = dir.join("gen__model__web-download__p1__1.png");
        std::fs::write(&src, marked_png()).unwrap();
        let cfg = SurviveConfig {
            suite: vec![
                Distortion::None,
                Distortion::Jpeg(80),
                Distortion::Png,
                Distortion::Resize(0.5),
            ],
            keep: Some(dir.join("out")),
        };
        let batch = run(&registry(), tool(), std::slice::from_ref(&src), &[], &cfg).unwrap();
        assert_eq!(batch.summary.len(), 4);
        assert!(batch
            .summary
            .iter()
            .all(|r| r.source.as_deref() == Some(src.to_str().unwrap())));
        let f = fates(&batch);
        assert_eq!(f.len(), 1);
        let cells: BTreeMap<_, _> = f[0].1.iter().cloned().collect();
        assert_eq!(cells["none"].manifest, Fate::Kept);
        assert_eq!(cells["none"].xmp, Fate::Kept);
        for t in ["jpeg_q80", "png", "resize_0.5"] {
            assert_eq!(cells[t].manifest, Fate::Stripped, "{t}");
            assert_eq!(cells[t].xmp, Fate::Stripped, "{t}");
        }
        // --keep wrote reproducible derivatives with the expected names.
        assert!(dir
            .join("out/gen__model__web-download__p1__1__jpeg_q80.jpg")
            .is_file());
        assert!(dir
            .join("out/gen__model__web-download__p1__1__none.png")
            .is_file());
        let text = render_aggregate(&batch);
        assert!(text.contains("gen__model"));
        assert!(text.contains("M✓ X✓"));
        assert!(text.contains("M✗ X✗"));
        let long = render_long(&batch);
        assert!(long.lines().count() == 5);
        assert!(long.contains("\tjpeg_q80\t-\t-\t-\tnone"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn captured_files_are_matched_by_stem_and_take_columns() {
        let dir = tmpdir("cap");
        let a = dir.join("gen__m__web-download__p1__1.png");
        let b = dir.join("gen__m__web-download__p1__10.png"); // prefix trap
        std::fs::write(&a, marked_png()).unwrap();
        std::fs::write(&b, marked_png()).unwrap();
        let cap = dir.join("captured");
        std::fs::create_dir_all(&cap).unwrap();
        // whatsapp stripped it; "kept" pretends an app preserved the field
        std::fs::write(
            cap.join("gen__m__web-download__p1__1__whatsapp_photo.png"),
            Distortion::Png.apply(&marked_png()).unwrap().bytes,
        )
        .unwrap();
        std::fs::write(
            cap.join("gen__m__web-download__p1__10__kept.png"),
            marked_png(),
        )
        .unwrap();
        std::fs::write(cap.join("unrelated.png"), marked_png()).unwrap();

        let (matched, unmatched) = match_captured(&[a.clone(), b.clone()], &cap);
        assert_eq!(matched.len(), 2);
        assert_eq!(matched[0].input, 0);
        assert_eq!(matched[0].label, "whatsapp_photo");
        assert_eq!(matched[1].input, 1, "longest stem wins over the p1 prefix");
        assert_eq!(matched[1].label, "kept");
        assert_eq!(unmatched.len(), 1);

        let cfg = SurviveConfig {
            suite: vec![Distortion::None],
            keep: None,
        };
        let batch = run(&registry(), tool(), &[a, b], &matched, &cfg).unwrap();
        let f = fates(&batch);
        let c0: BTreeMap<_, _> = f[0].1.iter().cloned().collect();
        let c1: BTreeMap<_, _> = f[1].1.iter().cloned().collect();
        assert_eq!(c0["captured:whatsapp_photo"].xmp, Fate::Stripped);
        assert_eq!(c0["captured:whatsapp_photo"].manifest, Fate::Stripped);
        assert_eq!(c1["captured:kept"].xmp, Fate::Kept);
        let agg = render_aggregate(&batch);
        assert!(agg.contains("captured:whatsapp_photo"));
        assert!(agg.contains("captured:kept"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fates_cover_every_transition() {
        fn row(state: Option<&str>, remote: bool, xmp: &[&str]) -> FileRow {
            let mut r = summarize(&Inspection {
                schema_version: halftone_core::SCHEMA_VERSION.into(),
                tool: tool(),
                asset: halftone_core::AssetInfo {
                    path: None,
                    modality: Modality::Image,
                    mime: "image/png".into(),
                    sha256: "0".repeat(64),
                    size_bytes: 1,
                },
                evidence: vec![],
                created_at: String::new(),
            });
            r.manifest.validation_state = state.map(str::to_string);
            r.manifest.remote_manifest_url = remote.then(|| "https://x".into());
            r.marking.digital_source_type = xmp.iter().map(|s| s.to_string()).collect();
            r
        }
        let trusted = row(Some("Trusted"), false, &["trainedAlgorithmicMedia"]);
        let none = row(None, false, &[]);
        let invalid = row(Some("Invalid"), false, &[]);
        let remote = row(None, true, &["trainedAlgorithmicMedia"]);
        let other_term = row(Some("Trusted"), false, &["digitalCapture"]);
        assert_eq!(manifest_fate(&trusted, &trusted), Fate::Kept);
        assert_eq!(manifest_fate(&trusted, &none), Fate::Stripped);
        assert_eq!(manifest_fate(&trusted, &invalid), Fate::Broken);
        assert_eq!(manifest_fate(&none, &none), Fate::NotApplicable);
        assert_eq!(manifest_fate(&none, &trusted), Fate::Gained);
        assert_eq!(manifest_fate(&remote, &remote), Fate::Kept);
        assert_eq!(manifest_fate(&remote, &none), Fate::Stripped);
        assert_eq!(manifest_fate(&remote, &trusted), Fate::Broken);
        assert_eq!(xmp_fate(&trusted, &trusted), Fate::Kept);
        assert_eq!(xmp_fate(&trusted, &none), Fate::Stripped);
        assert_eq!(xmp_fate(&none, &trusted), Fate::Gained);
        assert_eq!(xmp_fate(&trusted, &other_term), Fate::Broken);
        assert_eq!(xmp_fate(&none, &none), Fate::NotApplicable);
    }

    #[test]
    fn writer_key_follows_the_corpus_convention() {
        assert_eq!(
            writer_of("/x/google-flow__nano-banana-2__web-download-thumb__p1__1.jpeg"),
            "google-flow__nano-banana-2"
        );
        assert_eq!(writer_of("/x/plain.png"), "plain");
    }

    #[test]
    fn unreadable_input_is_an_error_not_a_panic() {
        let dir = tmpdir("bad");
        let bad = dir.join("x.png");
        std::fs::write(&bad, b"not a png").unwrap();
        let cfg = SurviveConfig::default();
        let err = run(&registry(), tool(), std::slice::from_ref(&bad), &[], &cfg).unwrap_err();
        assert!(matches!(err, SurviveError::Unsupported { .. }));
        let missing = dir.join("missing.png");
        let err = run(
            &registry(),
            tool(),
            std::slice::from_ref(&missing),
            &[],
            &cfg,
        )
        .unwrap_err();
        assert!(matches!(err, SurviveError::Io { .. }));
        let _ = std::fs::remove_dir_all(dir);
    }
}
