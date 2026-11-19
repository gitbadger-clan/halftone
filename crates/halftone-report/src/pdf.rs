//! The report as a PDF, typeset in this process.
//!
//! [`to_pdf`] draws the same [`Report`] the page draws, through one template
//! (`report.typ`) and the Typst compiler used as a library. Nothing outside the
//! binary takes part: no browser, no network, no system font. The template, the two
//! font families, the mark and the verdict glyphs are compiled in, and the compiler
//! is given a world that holds exactly those files and no others.
//!
//! Three properties follow, and the tests hold them:
//!
//! - The same inspection and theme give the same bytes, on any machine. The PDF's
//!   date is the inspection's `created_at`, not the time of typesetting, and its
//!   document identifier comes from the file's hash, that date and the theme. An
//!   inspection whose time cannot be read gets an error, not a guessed date.
//! - The file is written as PDF/A-2b, the archival profile: fonts embedded, nothing
//!   that depends on a device or a viewer. The compiler checks this as it writes and
//!   fails if the document does not meet it. It is not an independent validation.
//! - Text from an inspection is data. It reaches the template as JSON strings, which
//!   Typst places as text, so a file name full of markup stays a file name.
//!
//! The light theme is the document for filing and print. The dark theme is the same
//! report for reading on a screen; printed, it is a dark page.

use std::collections::HashMap;
use std::sync::OnceLock;

use halftone_core::Inspection;
use typst::diag::{FileError, FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration, Smart};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

use crate::model::Report;

const TEMPLATE: &str = include_str!("report.typ");
/// The fonts the template sets text in, and the only ones the compiler is given:
/// Libertinus Serif in two weights and DejaVu Sans Mono. Our own copies, with their
/// licences and hashes in `fonts/README.md`.
const FONTS: [&[u8]; 3] = [
    include_bytes!("../fonts/LibertinusSerif-Regular.otf"),
    include_bytes!("../fonts/LibertinusSerif-Semibold.otf"),
    include_bytes!("../fonts/DejaVuSansMono.ttf"),
];

/// Which pigment on which paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// The brand ink on a white page: for filing and print.
    #[default]
    Light,
    /// The brand's light ink on its dark ground: for reading on a screen.
    Dark,
}

impl Theme {
    fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// Paper and ink (brand/README.md). Print-white, not the page's off-white, for
    /// the light theme: a filed document is white.
    fn colours(self) -> (&'static str, &'static str) {
        match self {
            Self::Light => ("#FFFFFF", "#1B3A5C"),
            Self::Dark => ("#1B1B1D", "#9CC0E8"),
        }
    }

    /// The mark and the four glyphs in this theme's ink, by the path the template
    /// asks for.
    fn assets(self) -> [(&'static str, &'static str); 5] {
        match self {
            Self::Light => [
                (
                    "/mark.svg",
                    include_str!("../assets/halftone-mark-light.svg"),
                ),
                (
                    "/glyph/present.svg",
                    include_str!("../assets/verdict/present-light.svg"),
                ),
                (
                    "/glyph/absent.svg",
                    include_str!("../assets/verdict/absent-light.svg"),
                ),
                (
                    "/glyph/inconclusive.svg",
                    include_str!("../assets/verdict/inconclusive-light.svg"),
                ),
                (
                    "/glyph/not_applicable.svg",
                    include_str!("../assets/verdict/not_applicable-light.svg"),
                ),
            ],
            Self::Dark => [
                (
                    "/mark.svg",
                    include_str!("../assets/halftone-mark-dark.svg"),
                ),
                (
                    "/glyph/present.svg",
                    include_str!("../assets/verdict/present-dark.svg"),
                ),
                (
                    "/glyph/absent.svg",
                    include_str!("../assets/verdict/absent-dark.svg"),
                ),
                (
                    "/glyph/inconclusive.svg",
                    include_str!("../assets/verdict/inconclusive-dark.svg"),
                ),
                (
                    "/glyph/not_applicable.svg",
                    include_str!("../assets/verdict/not_applicable-dark.svg"),
                ),
            ],
        }
    }
}

/// A typeset report.
#[derive(Debug, Clone)]
pub struct Pdf {
    /// The file's bytes.
    pub bytes: Vec<u8>,
    /// How many pages it has.
    pub pages: usize,
}

/// Why a report could not be typeset. With the built-in template this means a defect
/// in the template or the model, not in the inspection; the message says where.
#[derive(Debug, Clone)]
pub struct PdfError(String);

impl std::fmt::Display for PdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the PDF report could not be typeset: {}", self.0)
    }
}

impl std::error::Error for PdfError {}

/// The compiler's messages, each with the template line it points at.
fn diagnostics(what: &str, world: &dyn World, errors: &[SourceDiagnostic]) -> PdfError {
    use typst::WorldExt as _;
    let messages: Vec<String> = errors
        .iter()
        .map(|e| match world.range(e.span) {
            Some(range) => {
                let line = TEMPLATE
                    .get(..range.start)
                    .map_or(0, |before| before.matches('\n').count() + 1);
                format!("report.typ line {line}: {}", e.message)
            }
            None => e.message.to_string(),
        })
        .collect();
    PdfError(format!("{what}: {}", messages.join("; ")))
}

fn id(path: &str) -> Result<FileId, PdfError> {
    let vpath = VirtualPath::new(path).map_err(|e| PdfError(format!("{path}: {e:?}")))?;
    Ok(RootedPath::new(VirtualRoot::Project, vpath).intern())
}

/// The standard library and the built-in fonts, set up once per process.
struct Shared {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| {
        let fonts: Vec<Font> = FONTS
            .into_iter()
            .flat_map(|data| Font::iter(Bytes::new(data)))
            .collect();
        Shared {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(FontBook::from_fonts(&fonts)),
            fonts,
        }
    })
}

/// Everything the compiler may see: the template, the report as JSON, five images.
struct Sealed {
    main: FileId,
    source: Source,
    files: HashMap<FileId, Bytes>,
}

impl World for Sealed {
    fn library(&self) -> &LazyHash<Library> {
        &shared().library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &shared().book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            Ok(self.source.clone())
        } else {
            Err(FileError::NotSource)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        // Anything not put here does not exist: the template cannot read the disk.
        self.files.get(&id).cloned().ok_or(FileError::AccessDenied)
    }

    fn font(&self, index: usize) -> Option<Font> {
        shared().fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        // No clock: the document's date is the inspection's, given at export.
        None
    }
}

/// `2026-11-18T10:45:48Z` as a date and time. `None` for anything else; a date is
/// never guessed.
fn parse_utc(rfc3339: &str) -> Option<Datetime> {
    let s = rfc3339.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.splitn(3, '-');
    let mut t = time.splitn(3, ':');
    let year: i32 = d.next()?.parse().ok()?;
    let month: u8 = d.next()?.parse().ok()?;
    let day: u8 = d.next()?.parse().ok()?;
    let hour: u8 = t.next()?.parse().ok()?;
    let minute: u8 = t.next()?.parse().ok()?;
    // Fractions of a second are dropped; the PDF date has none.
    let second: u8 = t.next()?.split('.').next()?.parse().ok()?;
    Datetime::from_ymd_hms(year, month, day, hour, minute, second)
}

/// Typeset the report of one inspection.
pub fn to_pdf(insp: &Inspection, theme: Theme) -> Result<Pdf, PdfError> {
    report_pdf(&Report::new(insp), theme)
}

/// Typeset a report.
pub fn report_pdf(report: &Report, theme: Theme) -> Result<Pdf, PdfError> {
    // PDF/A requires the document's date, and the only honest one is the time of the
    // check. Without it there is no file to write; the time of typesetting is not it.
    let date = parse_utc(&report.created_at).ok_or_else(|| {
        PdfError(format!(
            "the inspection's time `{}` is not a UTC time such as 2026-11-18T10:45:48Z, \
             and the PDF's date is that time",
            report.created_at
        ))
    })?;
    let (paper, ink) = theme.colours();
    let data = serde_json::json!({
        "report": report,
        "theme": { "name": theme.name(), "paper": paper, "ink": ink },
    });
    let mut files = HashMap::new();
    files.insert(id("/report.json")?, Bytes::from_string(data.to_string()));
    for (path, svg) in theme.assets() {
        files.insert(id(path)?, Bytes::from_string(svg.to_string()));
    }
    let main = id("/report.typ")?;
    let world = Sealed {
        main,
        source: Source::new(main, TEMPLATE.to_string()),
        files,
    };
    let document = typst::compile::<PagedDocument>(&world)
        .output
        .map_err(|errors| diagnostics("template", &world, &errors))?;
    let standards = typst_pdf::PdfStandards::new(&[typst_pdf::PdfStandard::A_2b])
        .map_err(|e| PdfError(format!("PDF/A-2b: {}", e.message())))?;
    let options = typst_pdf::PdfOptions {
        ident: Smart::Custom(format!(
            "{}:{}:{}",
            report.sha256,
            report.created_at,
            theme.name()
        )),
        creator: Smart::Custom(Some(format!(
            "{} with Typst {}",
            report.tool,
            typst::utils::version().raw()
        ))),
        timestamp: Some(typst_pdf::Timestamp::new_utc(date)),
        standards,
        ..Default::default()
    };
    let bytes = typst_pdf::pdf(&document, &options)
        .map_err(|errors| diagnostics("export", &world, &errors))?;
    Ok(Pdf {
        bytes,
        pages: document.pages().len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use halftone_core::{
        AssetInfo, BuildInfo, Evidence, FileRef, Layer, Modality, PackRef, RunInfo, RunOptions,
        SourceId, Status, ToolInfo, TrustInput,
    };
    use serde_json::json;

    fn evidence(layer: Layer, name: &str, status: Status, rationale: &str) -> Evidence {
        Evidence {
            layer,
            source: SourceId {
                name: name.into(),
                version: "0.1.0".into(),
            },
            status,
            statistic: None,
            calibration: None,
            rationale: rationale.into(),
            details: serde_json::Value::Null,
            duration_ms: 0,
        }
    }

    /// A default run with one source of every status.
    fn inspection(file: &str) -> Inspection {
        Inspection {
            schema_version: "1.5.0".into(),
            tool: ToolInfo {
                name: "halftone".into(),
                version: "0.1.0".into(),
                build: Some(BuildInfo {
                    commit: Some("c146d01f0a0859253d06fb4ab955c75550c6f39a".into()),
                    modified: Some(false),
                    features: vec!["c2pa".into()],
                }),
            },
            asset: AssetInfo {
                path: Some(file.into()),
                modality: Modality::Image,
                mime: "image/jpeg".into(),
                sha256: "5c4081bb367965ab".repeat(4),
                size_bytes: 7918,
            },
            evidence: vec![
                evidence(Layer::Manifest, "c2pa", Status::Absent, "No C2PA manifest."),
                evidence(
                    Layer::Container,
                    "jpeg_quant",
                    Status::Present,
                    "libjpeg tables",
                ),
                evidence(
                    Layer::Container,
                    "jpeg_double",
                    Status::Inconclusive,
                    "too small",
                ),
                evidence(
                    Layer::Mark,
                    "dwtdct",
                    Status::NotApplicable,
                    "not implemented",
                ),
            ],
            run: Some(RunInfo {
                options: RunOptions {
                    layers: vec![Layer::Manifest, Layer::Container, Layer::Mark, Layer::Blind],
                    trust_list: "auto".into(),
                    ..Default::default()
                },
                trust: vec![TrustInput {
                    kind: "manifest".into(),
                    origin: "vendored".into(),
                    name: "C2PA-TRUST-LIST.pem".into(),
                    version: Some("c2pa-org/conformance-public@d6dec63476e2".into()),
                    fetched_at: None,
                    sha256: "b3".repeat(32),
                    certificates: Some(32),
                }],
                packs: vec![],
                gaps: vec![],
            }),
            created_at: "2026-11-18T10:45:48Z".into(),
        }
    }

    fn contains(haystack: &[u8], needle: &str) -> bool {
        haystack
            .windows(needle.len())
            .any(|w| w == needle.as_bytes())
    }

    #[test]
    fn the_same_inspection_gives_the_same_bytes_and_the_theme_changes_them() {
        let insp = inspection("/cases/photo.jpg");
        let light = to_pdf(&insp, Theme::Light).unwrap();
        assert!(light.bytes.starts_with(b"%PDF-"), "not a PDF");
        assert!(light.pages >= 1);
        assert_eq!(to_pdf(&insp, Theme::Light).unwrap().bytes, light.bytes);
        let dark = to_pdf(&insp, Theme::Dark).unwrap();
        assert_ne!(dark.bytes, light.bytes);
        assert_eq!(
            dark.pages, light.pages,
            "a theme changes colours, not layout"
        );
        // The PDF's date is the inspection's, so a later check is a different file.
        let mut later = insp.clone();
        later.created_at = "2026-11-19T10:45:48Z".into();
        assert_ne!(to_pdf(&later, Theme::Light).unwrap().bytes, light.bytes);
    }

    #[test]
    fn the_file_says_it_is_pdf_a_2b_and_names_its_title_and_maker() {
        let pdf = to_pdf(&inspection("/cases/photo.jpg"), Theme::Light).unwrap();
        // PDF/A keeps its metadata packet readable as plain text.
        assert!(contains(&pdf.bytes, "<pdfaid:part>2</pdfaid:part>"));
        assert!(contains(
            &pdf.bytes,
            "<pdfaid:conformance>B</pdfaid:conformance>"
        ));
        assert!(contains(&pdf.bytes, "Halftone report: photo.jpg"));
        assert!(contains(&pdf.bytes, "halftone 0.1.0 with Typst "));
        // No directory of the machine the check ran on.
        assert!(!contains(&pdf.bytes, "/cases"));
    }

    #[test]
    fn text_from_an_inspection_is_text_and_never_template_code() {
        // Each of these would change or break the document if it were read as markup.
        let hostile =
            "#import \"/etc/passwd\" *x* _y_ $1 + $ <label> @ref ] } \\ // `raw` #panic(\"no\")";
        let mut insp = inspection(&format!("/tmp/{}.jpg", hostile.replace('/', " ")));
        for e in &mut insp.evidence {
            e.rationale = hostile.into();
            e.source.name = hostile.into();
            e.source.version = hostile.into();
        }
        let run = insp.run.as_mut().unwrap();
        run.gaps = vec![hostile.into()];
        run.trust[0].name = hostile.into();
        run.trust[0].version = Some(hostile.into());
        let plain = to_pdf(&inspection("/tmp/a.jpg"), Theme::Light).unwrap();
        let pdf = to_pdf(&insp, Theme::Light).unwrap();
        assert!(pdf.bytes.starts_with(b"%PDF-"));
        assert!(pdf.pages >= plain.pages);
    }

    #[test]
    fn a_long_report_runs_over_pages_and_every_branch_of_the_template_typesets() {
        let mut insp = inspection("/cases/long.jpg");
        let words = "A rationale of ordinary length, as most sources write them. ".repeat(4);
        for i in 0..60 {
            insp.evidence.push(evidence(
                Layer::Blind,
                &format!("source_{i}"),
                Status::Absent,
                &words,
            ));
        }
        // The conformance section, measured, with a rubric that could not be evaluated.
        insp.evidence[0].details = json!({ "conformance": {
            "about": "Measurements of the manifest.",
            "rubrics": [
                { "id": "integrity", "name": "C2PA Asset Integrity Rubric", "version": "1.0",
                  "summary": { "met": 1, "not_met": 1, "warning": 0, "error": 0 },
                  "results": [
                      { "trait": "valid_success", "outcome": "met", "text": "No validity errors" },
                      { "trait": "trusted_success", "outcome": "not_met", "text": "Not trusted" } ] },
                { "id": "other", "name": "R", "version": "1.0", "error": "could not be evaluated" }
            ] } });
        // Every optional row of the conditions, and every origin of a trust input.
        let run = insp.run.as_mut().unwrap();
        run.options.reveal = true;
        run.options.fetch_remote_manifests = true;
        run.options.fingerprints = Some(FileRef {
            name: "extra.json".into(),
            sha256: "ab".repeat(32),
        });
        run.packs = vec![
            PackRef {
                name: "blind-image".into(),
                version: "0.3.0".into(),
                tier: "eval".into(),
                archive_sha256: "cd".repeat(32),
            },
            PackRef {
                name: "calibration".into(),
                version: "0.1.0".into(),
                tier: "pro".into(),
                archive_sha256: "ef".repeat(32),
            },
        ];
        run.gaps = vec!["packs: one gap".into(), "trust: another".into()];
        let mut installed = run.trust[0].clone();
        installed.origin = "installed".into();
        installed.fetched_at = Some("2026-09-02T08:00:00Z".into());
        let mut policy = run.trust[0].clone();
        policy.kind = "eku_policy".into();
        policy.certificates = None;
        policy.version = None;
        run.trust.extend([installed, policy]);
        let pdf = to_pdf(&insp, Theme::Dark).unwrap();
        assert!(pdf.pages >= 3, "{} pages", pdf.pages);

        // The other shapes: rubrics not measured, no trust list, no run record.
        insp.evidence[0].details = json!({ "conformance": { "error": "no export" } });
        insp.run.as_mut().unwrap().trust.clear();
        to_pdf(&insp, Theme::Light).unwrap();
        insp.run = None;
        insp.tool.build = None;
        to_pdf(&insp, Theme::Light).unwrap();
    }

    #[test]
    fn only_a_plain_utc_time_becomes_the_pdf_date() {
        assert!(parse_utc("2026-11-18T10:45:48Z").is_some());
        assert!(parse_utc("2026-11-18T10:45:48.250Z").is_some());
        for bad in [
            "",
            "2026-11-18",
            "2026-11-18T10:45:48+01:00",
            "2026-13-40T10:45:48Z",
            "now",
        ] {
            assert!(parse_utc(bad).is_none(), "{bad}");
        }
        // The PDF's date is the time of the check. Without one, no file is written:
        // the time of typesetting would be a made-up date.
        let mut insp = inspection("/cases/photo.jpg");
        insp.created_at = "yesterday".into();
        let err = to_pdf(&insp, Theme::Light).unwrap_err().to_string();
        assert!(err.contains("`yesterday` is not a UTC time"), "{err}");
    }

    #[test]
    fn the_compiler_is_given_three_fonts_of_two_families_and_no_others() {
        let fonts = &shared().fonts;
        assert_eq!(fonts.len(), FONTS.len(), "every file holds one usable font");
        let families: std::collections::BTreeSet<String> =
            fonts.iter().map(|f| f.info().family.clone()).collect();
        assert_eq!(
            families.into_iter().collect::<Vec<_>>(),
            ["DejaVu Sans Mono", "Libertinus Serif"]
        );
    }

    #[test]
    fn the_font_files_are_the_ones_the_readme_lists() {
        // The README's hashes are what a reviewer checks the files against.
        let readme = include_str!("../fonts/README.md");
        for bytes in FONTS {
            let hash = halftone_core::sha256_hex(bytes);
            assert!(readme.contains(&hash), "{hash} is not in fonts/README.md");
        }
    }
}
