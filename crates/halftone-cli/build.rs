//! Records which source `ht` is built from, for `tool.build` in every inspection
//! (verdict schema 1.5.0).
//!
//! Sets two compile-time variables that `src/main.rs` reads with `option_env!`:
//!
//! - `HALFTONE_BUILD_COMMIT`: the git commit, 40 hex digits (64 in a SHA-256 repository);
//! - `HALFTONE_BUILD_MODIFIED`: `true` when a build input differed from that commit,
//!   else `false`.
//!
//! Neither is set when the build cannot tell, and the inspection then carries no
//! commit. A wrong commit would be worse than none, so every doubt ends there.
//!
//! Two places can answer, asked in this order:
//!
//! 1. `.cargo_vcs_info.json` next to the manifest. `cargo package` writes it into a
//!    published crate, so it is what a crates.io install has. It comes first because
//!    a packaged crate can sit inside someone else's git repository (a vendored
//!    dependency), whose commit says nothing about this source.
//! 2. The git checkout, when this crate is at `crates/halftone-cli` of its top level
//!    and its manifest is tracked there.
//!
//! "Build input" follows the stale-binary check (`scripts/lib/htbin.py`): the files
//! under `crates/` without tests, benches, examples, fuzz targets and Markdown, plus
//! the workspace's `Cargo.toml`, `Cargo.lock` and `rust-toolchain.toml`. A tracked
//! file that changed and a new file git does not ignore both count as modified.
//!
//! The script runs again when one of those files changes or the checkout moves to
//! another commit, so the two variables describe the build they are compiled into.
//! When it cannot tell, it names nothing to watch, and cargo then runs it again
//! whenever a file of this crate changes.
//! It runs `git` and reads one file; no network. Without `git` the answer is "cannot
//! tell".

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Directory names whose files build no part of `ht` (as in `htbin.py`).
const NOT_INPUTS: [&str; 4] = ["tests", "benches", "examples", "fuzz"];
/// Workspace files that are build inputs besides `crates/`.
const WORKSPACE_FILES: [&str; 3] = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"];

fn main() {
    let Some(here) = env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from) else {
        return;
    };
    let packaged = here.join(".cargo_vcs_info.json");
    if packaged.exists() {
        println!("cargo:rerun-if-changed=build.rs");
        println!("cargo:rerun-if-changed={}", packaged.display());
        if let Some((commit, modified)) = std::fs::read_to_string(&packaged)
            .ok()
            .as_deref()
            .and_then(from_vcs_info)
        {
            emit(&commit, modified);
        }
        return;
    }
    if let Some((commit, modified)) = from_git(&here) {
        emit(&commit, modified);
    }
}

fn emit(commit: &str, modified: bool) {
    println!("cargo:rustc-env=HALFTONE_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=HALFTONE_BUILD_MODIFIED={modified}");
}

/// A full object name: 40 hex digits, or 64 in a SHA-256 repository.
fn is_commit(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The commit and dirty flag `cargo package` recorded:
/// `{"git": {"sha1": "<hex>", "dirty": true}, "path_in_vcs": "..."}`, where `dirty`
/// is written only when the crate was packaged from a modified tree.
fn from_vcs_info(text: &str) -> Option<(String, bool)> {
    let after_key = text.split_once("\"sha1\"")?.1;
    let commit = after_key.split('"').nth(1)?;
    if !is_commit(commit) {
        return None;
    }
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    Some((
        commit.to_ascii_lowercase(),
        compact.contains("\"dirty\":true"),
    ))
}

/// Run git in `dir`; its trimmed stdout when it succeeded.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn from_git(here: &Path) -> Option<(String, bool)> {
    let top = PathBuf::from(git(here, &["rev-parse", "--show-toplevel"])?);
    // Only the workspace this crate belongs to can speak for it: the crate sits where
    // the workspace keeps it, and its manifest is a tracked file there.
    let expected = top.join("crates").join("halftone-cli");
    if expected.canonicalize().ok()? != here.canonicalize().ok()? {
        return None;
    }
    git(here, &["ls-files", "--error-unmatch", "Cargo.toml"])?;
    let commit = git(&top, &["rev-parse", "--verify", "HEAD"])?;
    if !is_commit(&commit) {
        return None;
    }

    let mut spec: Vec<String> = vec!["crates".into()];
    spec.extend(WORKSPACE_FILES.iter().map(|f| f.to_string()));
    spec.extend(
        NOT_INPUTS
            .iter()
            .map(|d| format!(":(exclude,glob)crates/**/{d}/**")),
    );
    spec.push(":(exclude,glob)crates/**/*.md".into());
    let mut args = vec!["status", "--porcelain", "--untracked-files=all", "--"];
    args.extend(spec.iter().map(String::as_str));
    let modified = !git(&top, &args)?.is_empty();

    // Run again when a build input changes (cargo scans a directory for the newest
    // file) or when HEAD moves. A path that does not exist would make cargo run the
    // script on every build, so only existing ones are named.
    let mut watch = vec![top.join("crates")];
    watch.extend(WORKSPACE_FILES.iter().map(|f| top.join(f)));
    for name in ["HEAD", "logs/HEAD", "refs/heads", "packed-refs"] {
        if let Some(p) = git(&top, &["rev-parse", "--git-path", name]) {
            watch.push(top.join(p)); // an absolute path replaces `top`
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
    for p in watch.iter().filter(|p| p.exists()) {
        println!("cargo:rerun-if-changed={}", p.display());
    }
    Some((commit, modified))
}
