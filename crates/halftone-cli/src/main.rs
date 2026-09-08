//! `halftone` CLI.
mod complete;
mod glyph;

use glyph::{Glyphs, StatusGlyph};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, CommandFactory as _, Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::CompleteEnv;
use halftone_c2pa::{InternalList, TrustConfig};
use halftone_container::fingerprints::{FingerprintDb, WriterClass};
use halftone_core::{Asset, Layer, Registry, Status, ToolInfo};

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
    /// List registered evidence sources.
    Sources,
    /// Harvest JPEG writer fingerprints from files whose writer you know.
    Fingerprint(FingerprintArgs),
    /// Embed a C2PA manifest and an open watermark (provenance side).
    Sign {
        /// Input file.
        #[arg(add = complete::asset_files())]
        input: PathBuf,
        /// Signing key.
        #[arg(long, value_hint = ValueHint::FilePath)]
        key: PathBuf,
    },
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
    /// Show installed packs and their calibration.
    List,
    /// Download and verify current packs and trust lists (the only networked command).
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
    /// (TLS only, no publisher signature, no index). Works before any key ceremony.
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
    /// Files to inspect.
    #[arg(required = true, add = complete::asset_files())]
    inputs: Vec<PathBuf>,
    /// Emit JSON (one object per line).
    #[arg(long)]
    json: bool,
    /// One document for all inputs: with --json a batch object (full inspections plus
    /// per-file summary rows); otherwise a file × source matrix. Exit code unchanged.
    #[arg(long)]
    batch: bool,
    /// Only run these layers.
    #[arg(long, value_delimiter = ',')]
    only: Option<Vec<LayerArg>>,
    /// Write an HTML report next to each input.
    #[arg(long)]
    report: bool,
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
}

fn registry(o: &RegistryOpts) -> Result<Registry> {
    let want = |l: Layer| match &o.only {
        Some(list) => list.iter().any(|&x| Layer::from(x) == l),
        None => true,
    };
    let mut reg = Registry::new();
    if want(Layer::Manifest) {
        reg.push(Box::new(halftone_c2pa::C2paSource {
            trust: o.trust.clone(),
        }));
    }
    if want(Layer::Container) {
        let db = match &o.fingerprints {
            Some(p) => FingerprintDb::load(p)
                .map_err(|e| anyhow::anyhow!("loading fingerprints {}: {e}", p.display()))?,
            None => FingerprintDb::builtin(),
        };
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

        // Pixel-domain lattice runs dark (no threshold) until a calibration exists.
        reg.push(Box::new(halftone_pixel::LatticeSource { threshold: None }));
    }
    if want(Layer::Mark) {
        reg.push(Box::new(halftone_mark::DwtDct {
            payload: b"SDV2".to_vec(),
        }));
    }
    if want(Layer::Blind) {
        reg.push(Box::new(halftone_blind::FeatureProbe {
            pack: "blind-image-dinov2-probe".into(),
        }));
    }
    Ok(reg)
}

fn tool() -> ToolInfo {
    ToolInfo {
        name: "halftone".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

fn main() -> Result<()> {
    // Must run before anything touches stdout. A no-op unless `COMPLETE=<shell>` is set,
    // in which case it prints candidates (or the registration script) and exits.
    CompleteEnv::with_factory(Cli::command).complete();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    match Cli::parse().cmd {
        Cmd::Inspect(a) => inspect(a),
        Cmd::Sources => {
            let reg = registry(&RegistryOpts {
                only: None,
                fingerprints: None,
                trust: TrustConfig::default(),
            })?;
            for (layer, id) in reg.ids() {
                println!("{layer:?}\t{}\t{}", id.name, id.version);
            }
            Ok(())
        }
        Cmd::Fingerprint(a) => fingerprint(a),
        Cmd::Sign { input, key } => {
            anyhow::bail!(
                "sign: not yet implemented ({} with key {})",
                input.display(),
                key.display()
            )
        }
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
    use halftone_packs::index::{parse_keys, OFFICIAL_PUBLISHER_KEYS_HEX};
    use halftone_packs::store::Store;
    use halftone_packs::update::{run, Only, UpdateOptions};

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

fn packs_list() -> Result<()> {
    use halftone_packs::store::Store;
    let store = Store::resolve().map_err(|e| anyhow::anyhow!("{e}"))?;
    let inst = store.load_installed().map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("home: {}", store.root().display());
    if inst.packs.is_empty() && inst.trust.is_empty() {
        println!("nothing installed; run `ht packs update`");
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
    })?;
    let glyphs = Glyphs::detect();
    let mut any_present = false;
    let mut collected: Vec<halftone_core::Inspection> = Vec::new();
    for input in &a.inputs {
        let asset =
            Asset::from_path(input).with_context(|| format!("loading {}", input.display()))?;
        let insp = reg.inspect(&asset, tool());
        any_present |= insp.evidence.iter().any(|e| e.status == Status::Present);
        if a.report {
            let out = input.with_extension("halftone.html");
            std::fs::write(&out, halftone_report::to_html(&insp))?;
        }
        if a.batch {
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
            for e in insp.evidence.iter().filter(|e| !unsupported(e)) {
                println!(
                    "  {} {:<13} {:<9} {:<17} {}",
                    e.status.glyph(glyphs),
                    format!("{:?}", e.status),
                    format!("{:?}", e.layer),
                    e.source.name,
                    e.rationale
                );
            }
            if skipped > 0 {
                println!("  ({skipped} sources not applicable to this format)");
            }
        }
    }
    if a.batch {
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
    if s.is_empty() {
        0.0
    } else {
        s[s.len() / 2]
    }
}

fn hex_lower(b: &[u8]) -> String {
    use std::fmt::Write as _;
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}
