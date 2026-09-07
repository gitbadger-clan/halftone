//! Dynamic shell completion.
//!
//! Static things (subcommands, flags, `ValueEnum` values, comma-delimited `--only`
//! lists) are completed by `clap_complete` from the `Cli` definition. This module
//! adds the candidates that can only be known at `<TAB>` time:
//!
//! | argument                          | source of candidates                                   |
//! |-----------------------------------|--------------------------------------------------------|
//! | `inspect <inputs>`                | files with an extension `Asset::from_path` can sniff   |
//! | `fingerprint <inputs>`            | JPEG / PNG files                                       |
//! | `fingerprint --writer`            | writer names already in the built-in fingerprint DB    |
//! | `fingerprint --source`            | `manual` + sources already in the built-in DB          |
//! | `fingerprint --into`, `--fingerprints`, `bench <corpus>` | `*.json`                       |
//! | `inspect --trust-list`            | `auto` / `vendored` / `none` + PEM bundles             |
//! | `inspect --trust-anchors`         | PEM bundles                                            |
//! | `packs update --from`, `corpus <dir>`, `bench --calib-dir` | directories                  |
//!
//! Activation is environment-driven and costs nothing on a normal run: `main` calls
//! [`CompleteEnv::complete`] first thing; unless `COMPLETE=<shell>` is set it returns
//! immediately. `ht completions <shell>` prints the one-line registration snippet.

use std::ffi::OsStr;
use std::path::Path;

use clap_complete::engine::{
    ArgValueCandidates, ArgValueCompleter, CompletionCandidate, PathCompleter, ValueCompleter,
};
use halftone_container::fingerprints::FingerprintDb;

/// Extensions of every format `halftone_core::asset::sniff` recognises. Only a
/// completion hint: inspection sniffs magic bytes and never trusts the extension.
const ASSET_EXTS: &[&str] = &[
    "jpg", "jpeg", "jpe", "png", "webp", "heic", "heif", "avif", // image
    "wav", "flac", "mp3", // audio
    "mp4", "m4v", "mov", "webm", "mkv", // video
];

const JPEG_PNG_EXTS: &[&str] = &["jpg", "jpeg", "jpe", "png"];
const PEM_EXTS: &[&str] = &["pem", "crt", "cer"];
const JSON_EXTS: &[&str] = &["json"];

fn has_ext(p: &Path, exts: &[&str]) -> bool {
    p.extension()
        .and_then(OsStr::to_str)
        .map(|e| exts.iter().any(|x| x.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

/// Files with one of `exts`. Directories are still offered (after the matches) so the
/// user can descend into them.
fn files_with_ext(exts: &'static [&'static str]) -> PathCompleter {
    PathCompleter::any().filter(move |p| p.is_file() && has_ext(p, exts))
}

/// `inspect <inputs>`: anything the asset sniffer accepts.
pub fn asset_files() -> ArgValueCompleter {
    ArgValueCompleter::new(files_with_ext(ASSET_EXTS))
}

/// `fingerprint <inputs>`: the two containers the harvester understands.
pub fn jpeg_png_files() -> ArgValueCompleter {
    ArgValueCompleter::new(files_with_ext(JPEG_PNG_EXTS))
}

/// `*.json` (fingerprint DBs, corpus manifests).
pub fn json_files() -> ArgValueCompleter {
    ArgValueCompleter::new(files_with_ext(JSON_EXTS))
}

/// PEM bundles (`--trust-anchors`).
pub fn pem_files() -> ArgValueCompleter {
    ArgValueCompleter::new(files_with_ext(PEM_EXTS))
}

/// Directories only.
pub fn dirs() -> ArgValueCompleter {
    ArgValueCompleter::new(PathCompleter::dir())
}

/// `--trust-list`: the three keywords, then PEM bundles.
pub fn trust_list() -> ArgValueCompleter {
    ArgValueCompleter::new(TrustListCompleter)
}

struct TrustListCompleter;

impl ValueCompleter for TrustListCompleter {
    fn complete(&self, current: &OsStr) -> Vec<CompletionCandidate> {
        const KEYWORDS: [(&str, &str); 3] = [
            (
                "auto",
                "installed copy of the official list, else the vendored one",
            ),
            ("vendored", "the list compiled into this binary"),
            ("none", "no internal list; only --trust-anchors"),
        ];
        let typed = current.to_string_lossy();
        let mut out: Vec<CompletionCandidate> = KEYWORDS
            .iter()
            .filter(|(k, _)| k.starts_with(&*typed))
            .map(|(k, help)| CompletionCandidate::new(*k).help(Some((*help).into())))
            .collect();
        out.extend(files_with_ext(PEM_EXTS).complete(current));
        out
    }
}

/// `fingerprint --writer`: names already in the built-in DB, so a re-harvest of the
/// same writer spells it identically. Free-form text is still accepted.
pub fn known_writers() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        let db = FingerprintDb::builtin();
        let mut names: Vec<(String, String)> = db
            .entries()
            .map(|e| (e.writer.clone(), format!("{} · {:?}", e.format, e.class)))
            .collect();
        names.sort();
        names.dedup_by(|a, b| a.0 == b.0);
        names
            .into_iter()
            .map(|(w, help)| CompletionCandidate::new(w).help(Some(help.into())))
            .collect()
    })
}

/// `fingerprint --source`: `manual` plus whatever provenance strings the built-in DB
/// already uses.
pub fn known_sources() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        let db = FingerprintDb::builtin();
        let mut s: Vec<String> = db
            .entries()
            .map(|e| e.source.clone())
            .filter(|s| !s.is_empty())
            .collect();
        s.push("manual".into());
        s.sort();
        s.dedup();
        s.into_iter().map(CompletionCandidate::new).collect()
    })
}

/// Names of packs currently installed in the store (for future `packs remove`/`packs
/// show`; unused today but kept next to its siblings so it is not reinvented).
#[allow(dead_code)]
pub fn installed_packs() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        let Ok(store) = halftone_packs::store::Store::resolve() else {
            return Vec::new();
        };
        let Ok(inst) = store.load_installed() else {
            return Vec::new();
        };
        inst.packs
            .iter()
            .map(|(name, p)| {
                CompletionCandidate::new(name.clone())
                    .help(Some(format!("{} ({})", p.version, p.tier).into()))
            })
            .collect()
    })
}

/// Write the shell snippet that registers `ht` for completion. The snippet calls back
/// into `COMPLETE=<shell> ht -- …` at every `<TAB>`, so it must be regenerated (not
/// cached) across upgrades; `ht completions` prints it so it can be `source`d at
/// shell start-up.
pub fn write_registration(shell: &str, out: &mut dyn std::io::Write) -> std::io::Result<()> {
    let shells = clap_complete::env::Shells::builtins();
    let completer = shells.completer(shell).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unsupported shell `{shell}`"),
        )
    })?;
    // Resolve to argv[0] (what the shell will find on PATH), not `current_exe`, so a
    // reinstall to another location keeps working.
    let bin = std::env::args_os()
        .next()
        .map(|a| a.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ht".into());
    completer.write_registration("COMPLETE", "ht", "ht", &bin, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cli;
    use clap::CommandFactory as _;
    use std::ffi::OsString;

    /// Run the engine as the shell would: `args` is the whole word list including
    /// `ht`, `index` the word under the cursor.
    fn complete(args: &[&str], index: usize) -> Vec<String> {
        let mut cmd = Cli::command();
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        clap_complete::engine::complete(&mut cmd, args, index, None)
            .expect("engine")
            .into_iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn subcommands_are_offered() {
        let c = complete(&["ht", ""], 1);
        for want in [
            "inspect",
            "sources",
            "fingerprint",
            "corpus",
            "bench",
            "packs",
            "completions",
        ] {
            assert!(c.iter().any(|x| x == want), "missing {want} in {c:?}");
        }
    }

    #[test]
    fn nested_packs_subcommands() {
        let c = complete(&["ht", "packs", ""], 2);
        assert!(
            c.contains(&"list".to_string()) && c.contains(&"update".to_string()),
            "{c:?}"
        );
    }

    #[test]
    fn only_layers_complete_after_a_comma() {
        let c = complete(&["ht", "inspect", "--only", "container,m"], 3);
        assert!(c.contains(&"container,manifest".to_string()), "{c:?}");
        assert!(c.contains(&"container,mark".to_string()), "{c:?}");
    }

    #[test]
    fn value_enums_complete() {
        let c = complete(&["ht", "fingerprint", "--class", ""], 3);
        assert!(
            c.contains(&"camera".to_string()) && c.contains(&"generator".to_string()),
            "{c:?}"
        );
        let c = complete(&["ht", "packs", "update", "--only", ""], 4);
        assert!(
            c.contains(&"trust".to_string()) && c.contains(&"packs".to_string()),
            "{c:?}"
        );
        let c = complete(&["ht", "completions", ""], 2);
        for s in ["bash", "zsh", "fish"] {
            assert!(c.iter().any(|x| x == s), "{c:?}");
        }
    }

    #[test]
    fn trust_list_offers_keywords() {
        let c = complete(&["ht", "inspect", "--trust-list", ""], 3);
        for k in ["auto", "vendored", "none"] {
            assert!(c.iter().any(|x| x == k), "{c:?}");
        }
        let c = complete(&["ht", "inspect", "--trust-list", "v"], 3);
        assert!(
            c.iter().any(|x| x == "vendored") && !c.iter().any(|x| x == "auto"),
            "{c:?}"
        );
    }

    #[test]
    fn writers_come_from_builtin_db() {
        let db = FingerprintDb::builtin();
        let mut expect: Vec<String> = db.entries().map(|e| e.writer.clone()).collect();
        expect.sort();
        expect.dedup();
        let got: Vec<String> = known_writers()
            .candidates()
            .into_iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect();
        assert_eq!(got, expect);
    }

    #[test]
    fn asset_filter_hides_unrelated_files_but_keeps_dirs() {
        let tmp = std::env::temp_dir().join(format!("ht-complete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("sub")).unwrap();
        for f in ["a.JPG", "b.png", "c.txt", "d.wav", "e.json"] {
            std::fs::write(tmp.join(f), b"").unwrap();
        }
        let list = |pc: PathCompleter| -> Vec<String> {
            pc.current_dir(&tmp)
                .complete(OsStr::new(""))
                .into_iter()
                .map(|c| c.get_value().to_string_lossy().into_owned())
                .collect()
        };
        let assets = list(files_with_ext(ASSET_EXTS));
        assert!(assets.contains(&"a.JPG".into()), "{assets:?}");
        assert!(assets.contains(&"d.wav".into()), "{assets:?}");
        assert!(!assets.contains(&"c.txt".into()), "{assets:?}");
        assert!(!assets.contains(&"e.json".into()), "{assets:?}");
        assert!(
            assets.iter().any(|x| x.starts_with("sub")),
            "dirs must stay: {assets:?}"
        );
        let json = list(files_with_ext(JSON_EXTS));
        assert!(
            json.contains(&"e.json".into()) && !json.contains(&"b.png".into()),
            "{json:?}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn registration_script_mentions_the_binary() {
        let mut buf = Vec::new();
        write_registration("fish", &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.contains("COMPLETE=fish") && s.contains("--command ht"),
            "{s}"
        );
        assert!(write_registration("cmd.exe", &mut Vec::new()).is_err());
    }
}
