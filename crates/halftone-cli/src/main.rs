//! `halftone` CLI.
mod complete;
mod glyph;

use glyph::{Glyphs, StatusGlyph};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Args, CommandFactory as _, Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::CompleteEnv;
use halftone_c2pa::{InternalList, TrustConfig};
use halftone_container::fingerprints::{FingerprintDb, WriterClass};
use halftone_core::{
    Asset, BuildInfo, FileRef, Layer, PackRef, Registry, RunInfo, RunOptions, Status, ToolInfo,
};

const BANNER: &str = concat!(
    "  ▄██  ● • ·\n",
    " ▐███  ● ● • ·   halftone ",
    env!("CARGO_PKG_VERSION"),
    "\n",
    "  ▀██  ● • ·",
);

#[derive(Parser)]
#[command(name = "ht", version, about = "Layered provenance and forensics", before_help = BANNER)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Inspect one or more files.
    Inspect(InspectArgs),
    /// Apply a transform suite to marked files and report whether the manifest and
    /// the XMP field still read afterwards (the survival table).
    Survive(SurviveArgs),
    /// List registered evidence sources.
    Sources,
    /// Harvest JPEG writer fingerprints from files whose writer you know.
    Fingerprint(FingerprintArgs),
    /// Build a corpus manifest from a labelled folder tree
    /// (`real-<class>/` and `gen-<generator>/` subfolders).
    Corpus {
        /// Directory containing `real-*` / `gen-*` subfolders of images.
        #[arg(add = complete::dirs())]
        dir: PathBuf,
        /// Output manifest path.
        #[arg(long, default_value = "corpus.json", value_hint = ValueHint::AnyPath)]
        out: PathBuf,
    },
    /// Run every statistical source over a corpus: per-class statistics, a threshold
    /// at the target FPR, and per-source calibration JSON.
    Bench {
        /// Corpus manifest (see `ht corpus`).
        #[arg(add = complete::json_files())]
        corpus: PathBuf,
        /// Target false-positive rate for thresholding.
        #[arg(long, default_value_t = 0.01)]
        fpr: f64,
        /// Append one JSON line per (file, source) with the raw statistic.
        #[arg(long, value_hint = ValueHint::AnyPath)]
        stats_out: Option<PathBuf>,
        /// Write `<source>.calibration.json` files into this directory.
        #[arg(long, add = complete::dirs())]
        calib_dir: Option<PathBuf>,
    },
    /// Manage model packs.
    Packs {
        #[command(subcommand)]
        cmd: PacksCmd,
    },
    /// Print the shell snippet that enables tab completion (dynamic: candidates are
    /// computed by `ht` itself at every `<TAB>`).
    ///
    /// fish:  `ht completions fish > ~/.config/fish/completions/ht.fish`
    /// zsh:   `ht completions zsh >> ~/.zshrc`
    /// bash:  `ht completions bash >> ~/.bashrc`
    Completions {
        /// Shell to generate for.
        #[arg(value_enum)]
        shell: ShellArg,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ShellArg {
    Bash,
    Zsh,
    Fish,
    Elvish,
    Powershell,
}

impl ShellArg {
    fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Elvish => "elvish",
            Self::Powershell => "powershell",
        }
    }
}

#[derive(Subcommand)]
enum PacksCmd {
    /// Show installed trust lists and packs.
    List,
    /// Download and verify trust lists and packs. Uses the network, unless --from
    /// is given.
    Update(UpdateArgs),
}

#[derive(Args)]
struct UpdateArgs {
    /// Report what would change without writing anything.
    #[arg(long)]
    dry_run: bool,
    /// Restrict to one artifact kind.
    #[arg(long, value_enum)]
    only: Option<OnlyArg>,
    /// Fetch the C2PA trust lists straight from the C2PA conformance repository
    /// (TLS only, no publisher signature, no index). The way to update while this
    /// version has no publisher key built in.
    #[arg(long)]
    upstream: bool,
    /// Install from a directory holding index.json, index.json.sig and the artifacts,
    /// instead of the network (air-gapped hosts).
    #[arg(long, add = complete::dirs())]
    from: Option<PathBuf>,
    /// Index URL (mirrors, staging).
    #[arg(long, default_value = halftone_packs::index::DEFAULT_INDEX_URL, value_hint = ValueHint::Url)]
    index_url: String,
    /// Additional publisher verifying key (hex). Repeatable. Also read from
    /// HALFTONE_PUBLISHER_KEYS (comma-separated).
    #[arg(long, action = clap::ArgAction::Append)]
    publisher_key: Vec<String>,
}

#[derive(Clone, Copy, ValueEnum)]
enum OnlyArg {
    Trust,
    Packs,
}

#[derive(Args)]
struct InspectArgs {
    /// Files to inspect. Given several paths, one that is a directory or not a
    /// supported file type (the JSON and Markdown next to the images in `dir/*`) is
    /// left out and named on stderr; given one path, that is an error.
    #[arg(required = true, add = complete::asset_files())]
    inputs: Vec<PathBuf>,
    /// Emit JSON (one object per line).
    #[arg(long)]
    json: bool,
    /// One document for all inputs: with --json a batch object (full inspections plus
    /// per-file summary rows); otherwise a file × source matrix. Exit code unchanged.
    #[arg(long)]
    batch: bool,
    /// One line per file with the named columns: manifest state, signer, declared
    /// source type, ingredient count, actions that carry a source type, and the XMP
    /// field. Aligned for reading; a projection of the same rows --batch --json
    /// carries, with no score. Exit code unchanged.
    #[arg(long, conflicts_with_all = ["json", "batch"])]
    table: bool,
    /// The --table columns, tab-separated with a header line (for cut, sort and
    /// spreadsheets).
    #[arg(long, conflicts_with_all = ["json", "batch", "table"])]
    tsv: bool,
    /// Only run these layers. This version has sources in `manifest` and
    /// `container`; `mark` and `blind` have none yet, and the output says so.
    #[arg(long, value_delimiter = ',')]
    only: Option<Vec<LayerArg>>,
    /// Write an HTML report next to each input.
    #[arg(long)]
    report: bool,
    /// Write a PDF report next to each input (needs a build with the `pdf` feature).
    /// Typeset by ht itself: no browser, no network; the same inspection gives the
    /// same bytes.
    #[arg(long)]
    pdf: bool,
    /// The PDF's colours: light for filing and print, dark for reading on a screen.
    #[arg(long, value_enum, default_value_t = PdfTheme::Light, requires = "pdf")]
    pdf_theme: PdfTheme,
    /// Extra JPEG writer-fingerprint DB (JSON) merged over the built-in one.
    #[arg(long, add = complete::json_files())]
    fingerprints: Option<PathBuf>,
    /// Extra C2PA trust anchors (PEM bundle). Repeatable. Added to the internal list.
    #[arg(long, action = clap::ArgAction::Append, add = complete::pem_files())]
    trust_anchors: Vec<PathBuf>,
    /// Internal C2PA trust list: `auto` (installed copy, else vendored), `vendored`,
    /// `none`, or a path to a PEM bundle that replaces the official list.
    #[arg(long, default_value = "auto", add = complete::trust_list())]
    trust_list: String,
    /// Fetch the C2PA manifest when a file carries only a reference to a remote one
    /// (e.g. Adobe Firefly downloads). Off by default: without it, inspection never
    /// contacts the network. HTTPS only, host names only, 20 s per fetch; the result
    /// then depends on that server and the time of checking, and says so.
    #[arg(long)]
    fetch_remote_manifests: bool,
    /// Print the provenance chain recorded in the C2PA store under each file,
    /// oldest first: who signed each step, with which tool, when, and what it says
    /// was done. Human output only; with --json the chain is in the c2pa source's
    /// details.lineage.
    #[arg(long)]
    provenance: bool,
    /// Print the C2PA integrity and conformance rubric results under each file that
    /// carries a manifest: one line per rubric, then only the checks not met, the
    /// warnings and anything that could not be evaluated. A measurement of how the
    /// manifest is written, not of the file, and not part of any verdict. Human
    /// output only; with --json the results are in the c2pa source's
    /// details.conformance.
    #[arg(long)]
    conformance: bool,
    /// Under each file, list the metadata fields the listing sources found, each with
    /// its field name and value as written: rights statements, identifiers (names,
    /// account and tracking records, device and document identifiers, a display's
    /// name, location),
    /// vendor AI flags and the data-mining value. Unsigned fields, quoted, not
    /// judged. Human output only; with --json they are in those sources' details.
    /// Identifier values and free-text rights values are masked unless --reveal is
    /// given.
    #[arg(long)]
    metadata: bool,
    /// Include the values of embedded identifiers (names, contact details, account and
    /// tracking records, serial numbers, display names, document identifiers,
    /// coordinates) and the
    /// free text of rights statements (notices, credits, owners, terms, links) and of
    /// a data-mining reservation (its Other Constraints text) in the output. Without
    /// it every output, JSON included, carries their field names, kinds and lengths
    /// only, so a report can be shared without repeating what the file discloses.
    #[arg(long)]
    reveal: bool,
}

#[derive(Args)]
// Doc comments on clap args are `--help` text; `<placeholder>` is meant literally there.
#[allow(rustdoc::invalid_html_tags)]
struct SurviveArgs {
    /// Files to run the suite on (the originals).
    #[arg(required = true)]
    inputs: Vec<PathBuf>,
    /// Directory of files real applications produced from the inputs, named
    /// <input stem>__<label>.<ext> (e.g. foo__whatsapp_photo.jpg). Each joins the
    /// table as captured:<label>.
    #[arg(long)]
    captured: Option<PathBuf>,
    /// Comma-separated transforms; default is the survival suite
    /// (none,jpeg_q95,jpeg_q80,jpeg_q70,resize_0.5,crop_0.1,png,webp). `multipart`
    /// stands for the byte-level edits of multi-part JPEGs, which re-encode nothing:
    /// keep_still (cut after the still image), drop_clip, cut_clip, flip_clip (a
    /// Motion Photo's clip removed, cut in the middle, one bit flipped) and
    /// flip_still (one bit flipped in the still). Example: --suite none,multipart.
    /// An edit is left out for inputs without the part it needs.
    #[arg(long)]
    suite: Option<String>,
    /// Write every derivative here as <stem>__<transform>.<ext> for reproduction.
    #[arg(long)]
    keep: Option<PathBuf>,
    /// Emit the batch document (JSON) instead of the tables.
    #[arg(long)]
    json: bool,
    /// Print the long table (one line per file × transform) instead of the aggregate.
    #[arg(long)]
    long: bool,
    /// Only run these layers. Default: manifest,container.
    #[arg(long, value_delimiter = ',')]
    only: Option<Vec<LayerArg>>,
    /// Extra JPEG writer-fingerprint DB (JSON) merged over the built-in one.
    #[arg(long)]
    fingerprints: Option<PathBuf>,
    /// Extra C2PA trust anchors (PEM bundle). Repeatable.
    #[arg(long, action = clap::ArgAction::Append)]
    trust_anchors: Vec<PathBuf>,
    /// Internal C2PA trust list: auto, vendored, none, or a PEM path.
    #[arg(long, default_value = "auto")]
    trust_list: String,
}

fn internal_list(s: &str) -> InternalList {
    match s {
        "auto" => InternalList::Auto,
        "vendored" => InternalList::Vendored,
        "none" => InternalList::Disabled,
        p => InternalList::File(PathBuf::from(p)),
    }
}

#[derive(Args)]
struct FingerprintArgs {
    /// JPEG or PNG files all written by the same, known software at the same settings.
    #[arg(required = true, add = complete::jpeg_png_files())]
    inputs: Vec<PathBuf>,
    /// Writer name, e.g. "Canon EOS R5 fw 1.8.1". `{q}` is replaced by the libjpeg
    /// quality when the tables are Annex-K scaled.
    #[arg(long, add = complete::known_writers())]
    writer: String,
    /// Writer class.
    #[arg(long, value_enum)]
    class: WriterClassArg,
    /// Where the reference files came from (recorded in the entry).
    #[arg(long, default_value = "manual", add = complete::known_sources())]
    source: String,
    /// Merge into an existing DB file instead of printing a fresh one.
    #[arg(long, add = complete::json_files())]
    into: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum LayerArg {
    Manifest,
    Container,
    Mark,
    Blind,
}

impl From<LayerArg> for Layer {
    fn from(l: LayerArg) -> Self {
        match l {
            LayerArg::Manifest => Layer::Manifest,
            LayerArg::Container => Layer::Container,
            LayerArg::Mark => Layer::Mark,
            LayerArg::Blind => Layer::Blind,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum WriterClassArg {
    Camera,
    Phone,
    Editor,
    Library,
    Messaging,
    Screenshot,
    Generator,
    Unknown,
}

impl From<WriterClassArg> for WriterClass {
    fn from(c: WriterClassArg) -> Self {
        match c {
            WriterClassArg::Camera => WriterClass::Camera,
            WriterClassArg::Phone => WriterClass::Phone,
            WriterClassArg::Editor => WriterClass::Editor,
            WriterClassArg::Library => WriterClass::Library,
            WriterClassArg::Messaging => WriterClass::Messaging,
            WriterClassArg::Screenshot => WriterClass::Screenshot,
            WriterClassArg::Generator => WriterClass::Generator,
            WriterClassArg::Unknown => WriterClass::Unknown,
        }
    }
}

struct RegistryOpts {
    only: Option<Vec<LayerArg>>,
    fingerprints: Option<PathBuf>,
    trust: TrustConfig,
    /// Only `inspect` exposes this; every other command stays offline.
    fetch_remote_manifests: bool,
    /// Only `inspect` exposes this; every other command keeps identifier and free-text
    /// rights values masked.
    reveal: bool,
}

fn registry(o: &RegistryOpts) -> Result<Registry> {
    let want = |l: Layer| match &o.only {
        Some(list) => list.iter().any(|&x| Layer::from(x) == l),
        None => true,
    };
    let mut reg = Registry::new();
    let mut run = run_info(o, &want);
    if want(Layer::Manifest) {
        reg.push(Box::new(halftone_c2pa::C2paSource {
            trust: o.trust.clone(),
            fetch_remote_manifests: o.fetch_remote_manifests,
        }));
    }
    if want(Layer::Container) {
        let db = match &o.fingerprints {
            Some(p) => FingerprintDb::load(p)
                .map_err(|e| anyhow::anyhow!("loading fingerprints {}: {e}", p.display()))?,
            None => FingerprintDb::builtin(),
        };
        if let Some(p) = &o.fingerprints {
            // Loaded just above, so it can be read; the record names it by content.
            let bytes = std::fs::read(p)
                .with_context(|| format!("reading fingerprints {}", p.display()))?;
            run.options.fingerprints = Some(FileRef {
                name: halftone_core::run::file_name_only(p),
                sha256: halftone_core::sha256_hex(&bytes),
            });
        }
        reg.push(Box::new(halftone_container::jpeg::QuantTables {
            db: db.clone(),
        }));
        reg.push(Box::<halftone_container::double::DoubleCompression>::default());
        reg.push(Box::new(halftone_container::png::PngWriter {
            db: db.clone(),
        }));
        reg.push(Box::new(halftone_container::webp::WebpWriter));
        reg.push(Box::new(halftone_container::exif::ExifConsistency));
        reg.push(Box::new(halftone_container::marking::MarkingMetadata));
        reg.push(Box::new(
            halftone_container::data_mining::DataMiningReservation { reveal: o.reveal },
        ));
        reg.push(Box::new(halftone_container::vendor_ai::VendorAiFlag));
        reg.push(Box::new(halftone_container::rights::RightsStatements {
            reveal: o.reveal,
        }));
        reg.push(Box::new(
            halftone_container::identifiers::EmbeddedIdentifiers { reveal: o.reveal },
        ));
        reg.push(Box::new(halftone_container::gcontainer::ContainerDirectory));
        reg.push(Box::new(halftone_container::mpf::MpfIndex));

        // Pixel-domain lattice runs dark (no threshold) until a calibration exists.
        reg.push(Box::new(halftone_pixel::LatticeSource { threshold: None }));
    }
    // The Mark and Blind layers have no source in this version: no watermark decoder
    // and no classifier is registered. An inspection asked for them all the same
    // has no row there, and every output says so (`halftone_core::empty_layers`).
    reg.set_run(run);
    Ok(reg)
}

/// The conditions of this run, for `run` in every inspection: the options that
/// change what is reported, the trust lists as they resolve now, and the installed
/// packs. Taken once, before the first file. What cannot be established is named in
/// `gaps` with no path in it; the record is meant for reports that get passed on.
fn run_info(o: &RegistryOpts, want: &dyn Fn(Layer) -> bool) -> RunInfo {
    let mut gaps = Vec::new();
    // Without the `c2pa` feature the manifest layer loads no list, so none is named.
    let trust = if want(Layer::Manifest) && cfg!(feature = "c2pa") {
        match o.trust.resolve() {
            Ok(resolved) => resolved.run_inputs(),
            Err(_) => {
                gaps.push(
                    "trust: the trust configuration could not be loaded; the c2pa \
                     source's rationale says why"
                        .to_string(),
                );
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    let packs = match halftone_packs::store::Store::resolve().and_then(|s| s.load_installed()) {
        Ok(installed) => installed
            .packs
            .into_iter()
            .map(|(name, p)| PackRef {
                name,
                version: p.version,
                tier: p.tier,
                archive_sha256: p.archive_sha256,
            })
            .collect(),
        Err(_) => {
            gaps.push("packs: the record of installed packs could not be read".to_string());
            Vec::new()
        }
    };
    RunInfo {
        options: RunOptions {
            layers: [Layer::Manifest, Layer::Container, Layer::Mark, Layer::Blind]
                .into_iter()
                .filter(|&l| want(l))
                .collect(),
            trust_list: match &o.trust.internal {
                InternalList::Auto => "auto",
                InternalList::Vendored => "vendored",
                InternalList::Disabled => "none",
                InternalList::File(_) => "file",
            }
            .to_string(),
            fetch_remote_manifests: o.fetch_remote_manifests,
            reveal: o.reveal,
            fingerprints: None, // set by `registry` once the file has loaded
        },
        trust,
        packs,
        gaps,
    }
}

fn tool() -> ToolInfo {
    ToolInfo {
        name: "halftone".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        build: Some(build_info()),
    }
}

/// The colours of a PDF report.
#[derive(Clone, Copy, ValueEnum)]
enum PdfTheme {
    /// The brand ink on a white page.
    Light,
    /// The brand's light ink on its dark ground.
    Dark,
}

/// Write the PDF report of one inspection next to its input.
#[cfg(feature = "pdf")]
fn write_pdf(insp: &halftone_core::Inspection, input: &Path, theme: PdfTheme) -> Result<()> {
    let theme = match theme {
        PdfTheme::Light => halftone_report::pdf::Theme::Light,
        PdfTheme::Dark => halftone_report::pdf::Theme::Dark,
    };
    let pdf = halftone_report::pdf::to_pdf(insp, theme)?;
    let out = input.with_extension("halftone.pdf");
    std::fs::write(&out, pdf.bytes).with_context(|| format!("writing {}", out.display()))
}

/// Without the `pdf` feature there is nothing to typeset with.
#[cfg(not(feature = "pdf"))]
fn write_pdf(_: &halftone_core::Inspection, _: &Path, _: PdfTheme) -> Result<()> {
    anyhow::bail!("{NO_PDF}")
}

/// Said when `--pdf` is asked of a build that cannot write one.
const NO_PDF: &str = "this ht was built without PDF reports; build it with the `pdf` feature \
(cargo install --path crates/halftone-cli --features pdf --locked), or use --report and \
print the page";

/// What a person at the terminal should know before reading or passing on the
/// output: the two conditions a table does not show by itself. One line each.
fn terminal_notes(build: &BuildInfo, reveal: bool) -> Vec<String> {
    let mut notes = Vec::new();
    if let (Some(commit), Some(true)) = (&build.commit, build.modified) {
        notes.push(format!(
            "this ht was built from modified source (commit {} plus local changes); \
             reports and JSON say so",
            commit.get(..12).unwrap_or(commit)
        ));
    }
    if reveal {
        notes.push(
            "--reveal: values are shown in full; the output repeats what the files \
             disclose, so handle it like the files"
                .to_string(),
        );
    }
    notes
}

/// Which build this is, as `build.rs` recorded it at compile time. No commit when it
/// could not tell; the features are known either way.
fn build_info() -> BuildInfo {
    let commit = option_env!("HALFTONE_BUILD_COMMIT").map(str::to_string);
    let modified = commit
        .as_ref()
        .and(option_env!("HALFTONE_BUILD_MODIFIED"))
        .map(|m| m == "true");
    BuildInfo {
        commit,
        modified,
        features: [("c2pa", cfg!(feature = "c2pa"))]
            .into_iter()
            .filter(|&(_, on)| on)
            .map(|(name, _)| name.to_string())
            .collect(),
    }
}

fn main() -> Result<()> {
    // Must run before anything touches stdout. A no-op unless `COMPLETE=<shell>` is set,
    // in which case it prints candidates (or the registration script) and exits.
    CompleteEnv::with_factory(Cli::command).complete();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    match Cli::parse().cmd {
        Cmd::Inspect(a) => inspect(a),
        Cmd::Survive(a) => survive(a),
        Cmd::Sources => {
            let reg = registry(&RegistryOpts {
                only: None,
                fingerprints: None,
                trust: TrustConfig::default(),
                fetch_remote_manifests: false,
                reveal: false,
            })?;
            for (layer, id) in reg.ids() {
                println!("{layer:?}\t{}\t{}", id.name, id.version);
            }
            Ok(())
        }
        Cmd::Fingerprint(a) => fingerprint(a),
        Cmd::Corpus { dir, out } => make_corpus(&dir, &out),
        Cmd::Bench {
            corpus,
            fpr,
            stats_out,
            calib_dir,
        } => bench(&corpus, fpr, stats_out.as_deref(), calib_dir.as_deref()),
        Cmd::Packs {
            cmd: PacksCmd::List,
        } => packs_list(),
        Cmd::Packs {
            cmd: PacksCmd::Update(a),
        } => packs_update(a),
        Cmd::Completions { shell } => {
            let mut out = std::io::stdout().lock();
            complete::write_registration(shell.name(), &mut out)?;
            Ok(())
        }
    }
}

fn packs_update(a: UpdateArgs) -> Result<()> {
    use halftone_packs::index::{OFFICIAL_PUBLISHER_KEYS_HEX, parse_keys};
    use halftone_packs::store::Store;
    use halftone_packs::update::{Only, UpdateOptions, run};

    let mut keys: Vec<String> = OFFICIAL_PUBLISHER_KEYS_HEX
        .iter()
        .map(|s| s.to_string())
        .collect();
    keys.extend(a.publisher_key);
    if let Ok(env) = std::env::var("HALFTONE_PUBLISHER_KEYS") {
        keys.extend(
            env.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from),
        );
    }
    // An index is only installed from when its signature verifies against a
    // publisher key. With none there is nothing to fetch it for, so no request is
    // made, and the message says what does work.
    if !a.upstream && keys.is_empty() {
        anyhow::bail!("{NO_PUBLISHER_KEY}");
    }
    let opts = UpdateOptions {
        index_url: a.index_url,
        publisher_keys: parse_keys(&keys).map_err(|e| anyhow::anyhow!("{e}"))?,
        only: match a.only {
            None => Only::All,
            Some(OnlyArg::Trust) => Only::Trust,
            Some(OnlyArg::Packs) => Only::Packs,
        },
        dry_run: a.dry_run,
        upstream: a.upstream,
        from_dir: a.from,
        ..Default::default()
    };
    let store = Store::resolve().map_err(|e| anyhow::anyhow!("{e}"))?;
    let report = run(&store, &opts).map_err(|e| anyhow::anyhow!("{e}"))?;
    // Report goes to stderr so stdout stays clean for scripting.
    for line in &report {
        eprintln!("{line}");
    }
    eprintln!("home: {}", store.root().display());
    Ok(())
}

/// Said by `packs update` when no publisher key is configured.
const NO_PUBLISHER_KEY: &str = "no publisher key is configured, and this version of ht has none \
built in, so a pack index cannot be verified and was not fetched.\n\
To install the C2PA trust lists, run `ht packs update --upstream`: they come straight from \
the C2PA conformance repository, over TLS.\n\
For an index of your own, pass --publisher-key or set HALFTONE_PUBLISHER_KEYS.";

fn packs_list() -> Result<()> {
    use halftone_packs::index::OFFICIAL_PUBLISHER_KEYS_HEX;
    use halftone_packs::store::Store;
    let store = Store::resolve().map_err(|e| anyhow::anyhow!("{e}"))?;
    let inst = store.load_installed().map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("home: {}", store.root().display());
    if inst.packs.is_empty() && inst.trust.is_empty() {
        // Without a built-in publisher key a plain update cannot verify an index.
        if OFFICIAL_PUBLISHER_KEYS_HEX.is_empty() {
            println!(
                "nothing installed; `ht packs update --upstream` installs the C2PA trust lists"
            );
        } else {
            println!("nothing installed; run `ht packs update`");
        }
    }
    for (name, p) in &inst.packs {
        println!(
            "pack   {name:<32} {:<12} {:<5} {}",
            p.version, p.tier, p.installed_at
        );
    }
    for (name, t) in &inst.trust {
        println!(
            "trust  {name:<32} {:<12} {:<8} {}",
            t.version, t.origin, t.fetched_at
        );
        for (f, h) in &t.files {
            println!("       {f:<32} {}", &h[..16]);
        }
    }
    Ok(())
}

fn inspect(a: InspectArgs) -> Result<()> {
    let reg = registry(&RegistryOpts {
        only: a.only.clone(),
        fingerprints: a.fingerprints.clone(),
        trust: TrustConfig {
            internal: internal_list(&a.trust_list),
            custom_anchors: a.trust_anchors.clone(),
            home: None,
        },
        fetch_remote_manifests: a.fetch_remote_manifests,
        reveal: a.reveal,
    })?;
    // Asked for a PDF this build cannot write: say so before any file is inspected.
    if a.pdf && !cfg!(feature = "pdf") {
        anyhow::bail!("{NO_PDF}");
    }
    // Said once, on stderr, where a person reads the result. A JSON document carries
    // the same facts in `tool.build` and `run`, so it gets no extra line.
    if !a.json {
        for note in terminal_notes(&build_info(), a.reveal) {
            eprintln!("note: {note}");
        }
    }
    let glyphs = Glyphs::detect();
    let mut any_present = false;
    let mut collected: Vec<halftone_core::Inspection> = Vec::new();
    // A listing such as `dir/*` brings along whatever sits next to the images. With
    // several paths, one that is no supported file is left out and said so on stderr,
    // so the rest still get their table. A single path was asked for by name: there
    // it stays the error it was. A file that cannot be read is an error either way.
    let several = a.inputs.len() > 1;
    let mut inspected = 0usize;
    for input in &a.inputs {
        if several && input.is_dir() {
            eprintln!("{}: left out: a directory", input.display());
            continue;
        }
        let asset = match Asset::from_path(input) {
            Ok(asset) => asset,
            Err(halftone_core::Error::Unsupported(why)) if several => {
                eprintln!("{}: left out: {why}", input.display());
                continue;
            }
            Err(e) => {
                return Err(e).with_context(|| format!("loading {}", input.display()));
            }
        };
        inspected += 1;
        let insp = reg.inspect(&asset, tool());
        any_present |= insp.evidence.iter().any(|e| e.status == Status::Present);
        if a.report {
            let out = input.with_extension("halftone.html");
            std::fs::write(&out, halftone_report::to_html(&insp))?;
        }
        if a.pdf {
            write_pdf(&insp, input, a.pdf_theme)?;
        }
        if a.batch || a.table || a.tsv {
            collected.push(insp);
            continue;
        }
        if a.json {
            println!("{}", serde_json::to_string(&insp)?);
        } else {
            println!(
                "{}  ({} bytes, {})",
                input.display(),
                insp.asset.size_bytes,
                insp.asset.mime
            );
            // Sources that simply don't apply to this format are noise in the human view;
            // they stay in the JSON. NotApplicable for other reasons (no pack) is shown.
            let unsupported = |e: &&halftone_core::Evidence| {
                e.status == Status::NotApplicable
                    && e.rationale == "source does not support this asset"
            };
            let skipped = insp.evidence.iter().filter(unsupported).count();
            if a.provenance
                && let Some(l) = insp
                    .evidence
                    .iter()
                    .find(|e| e.source.name == "c2pa")
                    .and_then(|e| e.details.get("lineage"))
            {
                print!("{}", halftone_c2pa::lineage::render_timeline(l));
            }
            for e in insp.evidence.iter().filter(|e| !unsupported(e)) {
                // With --provenance the block above already tells the source-type story
                // in plain words; the c2pa line keeps its signature result and points there.
                let rationale = match e.details.get("source_type_note").and_then(|v| v.as_str()) {
                    Some(note)
                        if a.provenance && !note.is_empty() && e.rationale.contains(note) =>
                    {
                        e.rationale.replacen(
                            note,
                            " What the records say happened: see the provenance block above.",
                            1,
                        )
                    }
                    _ => e.rationale.clone(),
                };
                println!(
                    "  {} {:<13} {:<9} {:<17} {}",
                    e.status.glyph(glyphs),
                    format!("{:?}", e.status),
                    format!("{:?}", e.layer),
                    e.source.name,
                    rationale
                );
            }
            if a.metadata {
                print!(
                    "{}",
                    halftone_container::embedded::render_listing(&insp.evidence)
                );
            }
            if a.conformance {
                let details = insp
                    .evidence
                    .iter()
                    .find(|e| e.source.name == "c2pa")
                    .and_then(|e| e.details.get("conformance"));
                if let Some(c) = details {
                    if c.get("rubrics").is_some() {
                        print!("{}", halftone_c2pa::conformance::render(c));
                    } else if let Some(err) = c.get("error").and_then(|v| v.as_str()) {
                        println!("  conformance: not measured: {err}");
                    }
                }
            }
            if skipped > 0 {
                println!("  ({skipped} sources not applicable to this format)");
            }
        }
    }
    if inspected == 0 {
        anyhow::bail!(
            "none of the {} paths is a supported file: nothing inspected",
            a.inputs.len()
        );
    }
    // The plain listing shows each file's rows. Said once, after them: which layers
    // were asked for and had no source, so that no row is not read as nothing found.
    // The batch matrix says the same under its counts; JSON carries `run.options.layers`.
    if !(a.json || a.batch || a.table || a.tsv)
        && let Some(note) = no_source_note(&reg)
    {
        println!("{note}");
    }
    if a.table || a.tsv {
        let batch = halftone_core::Batch::new(tool(), collected);
        let format = if a.tsv {
            halftone_core::TableFormat::Tsv
        } else {
            halftone_core::TableFormat::Aligned
        };
        print!("{}", halftone_core::render_table(&batch, format));
    } else if a.batch {
        let batch = halftone_core::Batch::new(tool(), collected);
        if a.json {
            println!("{}", serde_json::to_string(&batch)?);
        } else {
            print!(
                "{}",
                halftone_core::render_matrix(&batch, |s| s.glyph(glyphs))
            );
        }
    }
    // Exit code for CI: 0 = nothing present, 2 = at least one layer reported Present.
    if any_present {
        std::process::exit(2);
    }
    Ok(())
}

/// The sentence naming the layers this registry was asked for and has no source in.
fn no_source_note(reg: &Registry) -> Option<String> {
    let run = reg.run()?;
    let ids = reg.ids();
    halftone_core::empty_layers_note(&halftone_core::empty_layers(&run.options.layers, |l| {
        ids.iter().any(|(layer, _)| *layer == l)
    }))
}

fn survive(a: SurviveArgs) -> Result<()> {
    use halftone_bench::distort::{Distortion, parse_suite};
    use halftone_bench::survive::{
        SurviveConfig, dedup_inputs, match_captured, render_aggregate, render_long, run_reporting,
    };

    let reg = registry(&RegistryOpts {
        only: Some(
            a.only
                .clone()
                .unwrap_or_else(|| vec![LayerArg::Manifest, LayerArg::Container]),
        ),
        fingerprints: a.fingerprints.clone(),
        trust: TrustConfig {
            internal: internal_list(&a.trust_list),
            custom_anchors: a.trust_anchors.clone(),
            home: None,
        },
        // A survival table measures what the bytes carry; a fetched manifest is not
        // carried by the file, so survive never fetches.
        fetch_remote_manifests: false,
        reveal: false,
    })?;
    let suite = match &a.suite {
        Some(s) => parse_suite(s).map_err(|e| anyhow::anyhow!(e))?,
        None => Distortion::survival_suite(),
    };
    if !suite.contains(&Distortion::None) {
        anyhow::bail!("the suite must include `none` so every input has a baseline row");
    }
    // Byte-identical inputs (a route that serves the same file twice) are a
    // finding, not two rows: fold them and say so.
    let dedup = dedup_inputs(&a.inputs);
    for (dropped, kept) in &dedup.dropped {
        eprintln!(
            "same bytes as {}: {} (folded)",
            kept.display(),
            dropped.display()
        );
    }
    let inputs = dedup.kept;
    let (captured, unmatched) = match &a.captured {
        Some(dir) => match_captured(&inputs, dir),
        None => (Vec::new(), Vec::new()),
    };
    for u in &unmatched {
        eprintln!("captured file matches no input by stem: {}", u.display());
    }
    let cfg = SurviveConfig {
        suite,
        keep: a.keep.clone(),
    };
    let (batch, skipped) = run_reporting(&reg, tool(), &inputs, &captured, &cfg)?;
    for s in &skipped {
        eprintln!("{}: {} left out: {}", s.source, s.transform, s.reason);
    }
    if a.json {
        println!("{}", serde_json::to_string(&batch)?);
    } else if a.long {
        print!("{}", render_long(&batch));
    } else {
        print!("{}", render_aggregate(&batch));
    }
    Ok(())
}

fn fingerprint(a: FingerprintArgs) -> Result<()> {
    let mut db = match &a.into {
        Some(p) if p.exists() => {
            let s = std::fs::read_to_string(p)?;
            FingerprintDb::from_json(&s).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?
        }
        _ => FingerprintDb::empty(),
    };
    for input in &a.inputs {
        let bytes = std::fs::read(input).with_context(|| format!("reading {}", input.display()))?;
        let entry = if bytes.starts_with(b"\x89PNG") {
            let info = halftone_container::png::parse_png(&bytes)
                .map_err(|e| anyhow::anyhow!("{}: {e}", input.display()))?;
            let writer = a.writer.replace("{q}", "n/a");
            FingerprintDb::harvest_png(&info, writer, a.class.into(), a.source.clone())
        } else {
            let s = halftone_container::jpeg::parse_structure(&bytes)
                .map_err(|e| anyhow::anyhow!("{}: {e}", input.display()))?;
            let q = s
                .luma_table()
                .and_then(halftone_container::jpeg::estimate_libjpeg_quality)
                .map(|q| q.to_string())
                .unwrap_or_else(|| "n/a".into());
            let writer = a.writer.replace("{q}", &q);
            FingerprintDb::harvest(&s, writer, a.class.into(), a.source.clone())
        };
        eprintln!(
            "{}  {}  {}",
            input.display(),
            &entry.fingerprint[..12],
            entry.notes
        );
        db.insert(entry);
    }
    let json = db.to_json();
    match &a.into {
        Some(p) => {
            std::fs::write(p, json)?;
            eprintln!("wrote {} entries to {}", db.len(), p.display());
        }
        None => println!("{json}"),
    }
    Ok(())
}

fn make_corpus(dir: &std::path::Path, out: &std::path::Path) -> Result<()> {
    use halftone_bench::corpus::{Corpus, Entry, Label};
    let mut entries = Vec::new();
    for sub in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let sub = sub?;
        if !sub.file_type()?.is_dir() {
            continue;
        }
        let name = sub.file_name().to_string_lossy().into_owned();
        let label = if let Some(class) = name.strip_prefix("real-") {
            Label::Real {
                source: class.to_string(),
            }
        } else if let Some(generator) = name.strip_prefix("gen-") {
            Label::Generated {
                generator: generator.to_string(),
            }
        } else {
            eprintln!("skipping {name}: folder is neither real-<class> nor gen-<generator>");
            continue;
        };
        let mut files: Vec<_> = std::fs::read_dir(sub.path())?
            .filter_map(|f| f.ok())
            .map(|f| f.path())
            .filter(|p| {
                matches!(
                    p.extension()
                        .and_then(|e| e.to_str())
                        .map(str::to_lowercase)
                        .as_deref(),
                    Some("jpg" | "jpeg" | "png" | "webp")
                )
            })
            .collect();
        files.sort();
        for f in files {
            entries.push(Entry {
                path: format!(
                    "{name}/{}",
                    f.file_name().unwrap_or_default().to_string_lossy()
                ),
                label: label.clone(),
            });
        }
    }
    anyhow::ensure!(
        !entries.is_empty(),
        "no labelled images found under {}",
        dir.display()
    );
    let id = format!(
        "{}-{}",
        dir.file_name().unwrap_or_default().to_string_lossy(),
        entries.len()
    );
    let n = entries.len();
    let corpus = Corpus { id, entries };
    std::fs::write(out, serde_json::to_string_pretty(&corpus)?)?;
    eprintln!("wrote {} entries to {}", n, out.display());
    Ok(())
}

fn bench(
    manifest: &std::path::Path,
    fpr: f64,
    stats_out: Option<&std::path::Path>,
    calib_dir: Option<&std::path::Path>,
) -> Result<()> {
    use halftone_bench::corpus::{Corpus, Label};
    use halftone_bench::metrics::{threshold_at_fpr, tpr};
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;

    let manifest_bytes = std::fs::read(manifest)?;
    let corpus: Corpus = serde_json::from_slice(&manifest_bytes)?;
    let corpus_sha256 = hex_lower(&Sha256::digest(&manifest_bytes));
    let base = manifest.parent().unwrap_or(std::path::Path::new("."));
    let reg = registry(&RegistryOpts {
        only: None,
        fingerprints: None,
        trust: TrustConfig::default(),
        fetch_remote_manifests: false,
        reveal: false,
    })?;

    // (source → (label, value)) for every statistic-bearing evidence.
    let mut by_source: BTreeMap<String, Vec<(Label, f64)>> = BTreeMap::new();
    let mut log = match stats_out {
        Some(p) => Some(std::io::BufWriter::new(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)?,
        )),
        None => None,
    };
    let mut failed = 0usize;
    for e in &corpus.entries {
        let path = base.join(&e.path);
        let asset = match Asset::from_path(&path) {
            Ok(a) => a,
            Err(err) => {
                eprintln!("skip {}: {err}", path.display());
                failed += 1;
                continue;
            }
        };
        let insp = reg.inspect(&asset, tool());
        for ev in &insp.evidence {
            if let Some(stat) = &ev.statistic {
                by_source
                    .entry(ev.source.name.clone())
                    .or_default()
                    .push((e.label.clone(), stat.value));
                if let Some(w) = log.as_mut() {
                    use std::io::Write as _;
                    writeln!(
                        w,
                        "{}",
                        serde_json::json!({
                            "path": e.path,
                            "label": e.label,
                            "source": ev.source.name,
                            "statistic": stat.name,
                            "value": stat.value,
                            "sha256": insp.asset.sha256,
                        })
                    )?;
                }
            }
        }
    }
    anyhow::ensure!(
        failed < corpus.entries.len(),
        "no corpus entries could be read"
    );

    for (source, rows) in &by_source {
        let negatives: Vec<f64> = rows
            .iter()
            .filter(|(l, _)| matches!(l, Label::Real { .. }))
            .map(|(_, v)| *v)
            .collect();
        let mut by_class: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut by_gen: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for (l, v) in rows {
            match l {
                Label::Real { source } => by_class.entry(source.clone()).or_default().push(*v),
                Label::Generated { generator } => {
                    by_gen.entry(generator.clone()).or_default().push(*v)
                }
            }
        }
        println!(
            "\n== {source}  ({} real, {} generated) ==",
            negatives.len(),
            rows.len() - negatives.len()
        );
        if negatives.is_empty() {
            println!("  no real-labelled samples; cannot set a threshold");
            continue;
        }
        let t = threshold_at_fpr(&negatives, fpr);
        println!("  threshold @ FPR {fpr}: {t:.5}");
        for (class, vals) in &by_class {
            let f = vals.iter().filter(|&&v| v >= t).count() as f64 / vals.len() as f64;
            println!(
                "  real/{class:<12} n={:<4} fpr@t={f:.3}  median={:.4}",
                vals.len(),
                median_of(vals)
            );
        }
        let mut overall_pos = Vec::new();
        for (generator, vals) in &by_gen {
            overall_pos.extend_from_slice(vals);
            println!(
                "  gen/{generator:<13} n={:<4} tpr@t={:.3}  median={:.4}",
                vals.len(),
                tpr(vals, t),
                median_of(vals)
            );
        }
        if let Some(dir) = calib_dir {
            std::fs::create_dir_all(dir)?;
            let fpr_by_real_source: BTreeMap<&String, f64> = by_class
                .iter()
                .map(|(c, vals)| {
                    (
                        c,
                        vals.iter().filter(|&&v| v >= t).count() as f64 / vals.len() as f64,
                    )
                })
                .collect();
            // Shape matches halftone_packs::manifest::Calibration.
            let calib = serde_json::json!({
                "set_id": corpus.id,
                "corpus_sha256": corpus_sha256,
                "fpr_target": fpr,
                "threshold": t,
                "heldout_generators": by_gen.keys().collect::<Vec<_>>(),
                "tpr_at_fpr": if overall_pos.is_empty() { 0.0 } else { tpr(&overall_pos, t) },
                "fpr_by_real_source": fpr_by_real_source,
                "robustness": [],
            });
            let out = dir.join(format!("{source}.calibration.json"));
            std::fs::write(&out, serde_json::to_string_pretty(&calib)?)?;
            println!("  wrote {}", out.display());
        }
    }
    Ok(())
}

fn median_of(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if s.is_empty() { 0.0 } else { s[s.len() / 2] }
}

fn hex_lower(b: &[u8]) -> String {
    use std::fmt::Write as _;
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}
