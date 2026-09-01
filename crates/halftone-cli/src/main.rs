//! `halftone` CLI.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use halftone_container::fingerprints::{FingerprintDb, WriterClass};
use halftone_core::{Asset, Layer, Registry, Status, ToolInfo};

#[derive(Parser)]
#[command(name = "halftone", version, about = "Layered provenance and forensics")]
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
        input: PathBuf,
        /// Signing key.
        #[arg(long)]
        key: PathBuf,
    },
    /// Run the evaluation harness on a corpus.
    Bench {
        /// Corpus manifest.
        corpus: PathBuf,
    },
    /// Manage model packs.
    Packs {
        #[command(subcommand)]
        cmd: PacksCmd,
    },
}

#[derive(Subcommand)]
enum PacksCmd {
    /// Show installed packs and their calibration.
    List,
    /// Download and verify current packs (the only networked command).
    Update,
}

#[derive(Args)]
struct InspectArgs {
    /// Files to inspect.
    #[arg(required = true)]
    inputs: Vec<PathBuf>,
    /// Emit JSON (one object per line).
    #[arg(long)]
    json: bool,
    /// Only run these layers.
    #[arg(long, value_delimiter = ',')]
    only: Option<Vec<LayerArg>>,
    /// Write an HTML report next to each input.
    #[arg(long)]
    report: bool,
    /// Extra JPEG writer-fingerprint DB (JSON) merged over the built-in one.
    #[arg(long)]
    fingerprints: Option<PathBuf>,
    /// C2PA trust-anchor bundle (PEM). Default: the c2pa crate's built-in trust list.
    #[arg(long)]
    trust_anchors: Option<PathBuf>,
}

#[derive(Args)]
struct FingerprintArgs {
    /// JPEG files all written by the same, known software at the same settings.
    #[arg(required = true)]
    inputs: Vec<PathBuf>,
    /// Writer name, e.g. "Canon EOS R5 fw 1.8.1". `{q}` is replaced by the libjpeg
    /// quality when the tables are Annex-K scaled.
    #[arg(long)]
    writer: String,
    /// Writer class.
    #[arg(long, value_enum)]
    class: WriterClassArg,
    /// Where the reference files came from (recorded in the entry).
    #[arg(long, default_value = "manual")]
    source: String,
    /// Merge into an existing DB file instead of printing a fresh one.
    #[arg(long)]
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
    trust_anchors: Option<PathBuf>,
}

fn registry(o: &RegistryOpts) -> Result<Registry> {
    let want = |l: Layer| match &o.only {
        Some(list) => list.iter().any(|&x| Layer::from(x) == l),
        None => true,
    };
    let mut reg = Registry::new();
    if want(Layer::Manifest) {
        reg.push(Box::new(halftone_c2pa::C2paSource {
            trust_anchors: o.trust_anchors.clone(),
        }));
    }
    if want(Layer::Container) {
        let db = match &o.fingerprints {
            Some(p) => FingerprintDb::load(p)
                .map_err(|e| anyhow::anyhow!("loading fingerprints {}: {e}", p.display()))?,
            None => FingerprintDb::builtin(),
        };
        reg.push(Box::new(halftone_container::jpeg::QuantTables { db }));
        reg.push(Box::new(
            halftone_container::double::DoubleCompression::default(),
        ));
        reg.push(Box::new(halftone_container::png::PngWriter));
        reg.push(Box::new(halftone_container::webp::WebpWriter));
        reg.push(Box::new(halftone_container::exif::ExifConsistency));
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
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    match Cli::parse().cmd {
        Cmd::Inspect(a) => inspect(a),
        Cmd::Sources => {
            let reg = registry(&RegistryOpts {
                only: None,
                fingerprints: None,
                trust_anchors: None,
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
        Cmd::Bench { corpus } => anyhow::bail!("bench: not yet implemented ({})", corpus.display()),
        Cmd::Packs {
            cmd: PacksCmd::List,
        } => anyhow::bail!("packs list: not yet implemented"),
        Cmd::Packs {
            cmd: PacksCmd::Update,
        } => anyhow::bail!("packs update: not yet implemented"),
    }
}

fn inspect(a: InspectArgs) -> Result<()> {
    let reg = registry(&RegistryOpts {
        only: a.only.clone(),
        fingerprints: a.fingerprints.clone(),
        trust_anchors: a.trust_anchors.clone(),
    })?;
    let mut any_present = false;
    for input in &a.inputs {
        let asset =
            Asset::from_path(input).with_context(|| format!("loading {}", input.display()))?;
        let insp = reg.inspect(&asset, tool());
        any_present |= insp.evidence.iter().any(|e| e.status == Status::Present);
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
            let skipped = insp
                .evidence
                .iter()
                .filter(|e| {
                    e.status == Status::NotApplicable
                        && e.rationale == "source does not support this asset"
                })
                .count();
            for e in insp.evidence.iter().filter(|e| {
                !(e.status == Status::NotApplicable
                    && e.rationale == "source does not support this asset")
            }) {
                println!(
                    "  {:<9} {:<17} {:<14} {}",
                    format!("{:?}", e.layer),
                    e.source.name,
                    format!("{:?}", e.status),
                    e.rationale
                );
            }
            if skipped > 0 {
                println!("  ({skipped} sources not applicable to this format)");
            }
        }
        if a.report {
            let out = input.with_extension("halftone.html");
            std::fs::write(&out, halftone_report::to_html(&insp))?;
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
        let s = halftone_container::jpeg::parse_structure(&bytes)
            .map_err(|e| anyhow::anyhow!("{}: {e}", input.display()))?;
        let q = s
            .luma_table()
            .and_then(halftone_container::jpeg::estimate_libjpeg_quality)
            .map(|q| q.to_string())
            .unwrap_or_else(|| "n/a".into());
        let writer = a.writer.replace("{q}", &q);
        let entry = FingerprintDb::harvest(&s, writer, a.class.into(), a.source.clone());
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
