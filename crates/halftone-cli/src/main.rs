//! `halftone` CLI.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
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
    /// Embed a C2PA manifest and an open watermark (provenance side).
    Sign { #[arg()] input: PathBuf, #[arg(long)] key: PathBuf },
    /// Run the evaluation harness on a corpus.
    Bench { #[arg()] corpus: PathBuf },
    /// Manage model packs.
    Packs { #[command(subcommand)] cmd: PacksCmd },
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

fn registry(only: Option<&[LayerArg]>) -> Registry {
    let want = |l: Layer| match only {
        Some(o) => o.iter().any(|&x| Layer::from(x) == l),
        None => true,
    };
    let mut reg = Registry::new();
    if want(Layer::Manifest) {
        reg.push(Box::new(halftone_c2pa::C2paSource::default()));
    }
    if want(Layer::Container) {
        reg.push(Box::new(halftone_container::jpeg::QuantTables));
    }
    if want(Layer::Mark) {
        reg.push(Box::new(halftone_mark::DwtDct { payload: b"SDV2".to_vec() }));
    }
    if want(Layer::Blind) {
        reg.push(Box::new(halftone_blind::FeatureProbe { pack: "blind-image-dinov2-probe".into() }));
    }
    reg
}

fn tool() -> ToolInfo {
    ToolInfo { name: "halftone".into(), version: env!("CARGO_PKG_VERSION").into() }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    match Cli::parse().cmd {
        Cmd::Inspect(a) => inspect(a),
        Cmd::Sources => {
            for (layer, id) in registry(None).ids() {
                println!("{layer:?}\t{}\t{}", id.name, id.version);
            }
            Ok(())
        }
        Cmd::Sign { input, key } => {
            anyhow::bail!("sign: not yet implemented ({} with key {})", input.display(), key.display())
        }
        Cmd::Bench { corpus } => anyhow::bail!("bench: not yet implemented ({})", corpus.display()),
        Cmd::Packs { cmd: PacksCmd::List } => anyhow::bail!("packs list: not yet implemented"),
        Cmd::Packs { cmd: PacksCmd::Update } => anyhow::bail!("packs update: not yet implemented"),
    }
}

fn inspect(a: InspectArgs) -> Result<()> {
    let reg = registry(a.only.as_deref());
    let mut any_present = false;
    for input in &a.inputs {
        let asset = Asset::from_path(input).with_context(|| format!("loading {}", input.display()))?;
        let insp = reg.inspect(&asset, tool());
        any_present |= insp.evidence.iter().any(|e| e.status == Status::Present);
        if a.json {
            println!("{}", serde_json::to_string(&insp)?);
        } else {
            println!("{}  ({} bytes, {})", input.display(), insp.asset.size_bytes, insp.asset.mime);
            for e in &insp.evidence {
                println!("  {:<9} {:<14} {:<14} {}", format!("{:?}", e.layer), e.source.name, format!("{:?}", e.status), e.rationale);
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
