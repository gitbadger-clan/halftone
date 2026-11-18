//! The parts of a report that say which file was checked and under which conditions
//! (the inspection's `asset`, `tool.build` and `run`, verdict schema 1.5.0).
//!
//! A reader gets them at three levels:
//!
//! - [`file_html`], at the top: the file's name, type, size and hash, so the report
//!   can be matched to the bytes it is about.
//! - [`notes_html`], right under it and before the evidence: only what differs from
//!   a default run and changes how the evidence reads (values shown in full, remote
//!   manifests fetched, a build from modified source, a layer that did not run, a
//!   trust list the operator changed, something that could not be established). A
//!   default run has no notes and the block is absent.
//! - [`conditions_html`], below the evidence: the whole record, collapsed, with a
//!   one-line summary. Hashes and versions are for the person who repeats the check.
//!
//! Every statement here is the tool's own, about itself and its inputs. The section
//! says so, in keeping with what the evidence rows say about unsigned fields.
//! Files are named without their directory.

use std::fmt::Write as _;

use halftone_core::{BuildInfo, Inspection, Layer, RunInfo, TrustInput};

use crate::html_escape;

const ALL_LAYERS: [Layer; 4] = [Layer::Manifest, Layer::Container, Layer::Mark, Layer::Blind];
/// The feature whose absence leaves the manifest layer unable to read a manifest.
const MANIFEST_FEATURE: &str = "c2pa";
/// How much of a commit the one-line forms show; the table has all of it.
const SHORT_COMMIT: usize = 12;

/// The file a report is about: name (without its directory), type, size, SHA-256.
pub fn file_html(insp: &Inspection) -> String {
    let a = &insp.asset;
    let name = a.path.as_deref().map_or_else(
        || "(not a file: content given directly)".to_string(),
        halftone_core::run::file_name_only,
    );
    format!(
        "<h2>File</h2><table class=\"file\">\
<tr><th>Name</th><td>{}</td></tr>\
<tr><th>Type</th><td>{}</td></tr>\
<tr><th>Size</th><td>{} bytes</td></tr>\
<tr><th>SHA-256</th><td><code>{}</code></td></tr></table>",
        html_escape(&name),
        html_escape(&a.mime),
        grouped(a.size_bytes),
        html_escape(&a.sha256),
    )
}

/// `1234567` as `1,234,567`.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn names(layers: &[Layer]) -> String {
    layers
        .iter()
        .map(|l| format!("{l:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn short(commit: &str) -> &str {
    commit.get(..SHORT_COMMIT).unwrap_or(commit)
}

fn has_manifest_feature(b: &BuildInfo) -> bool {
    b.features.iter().any(|f| f == MANIFEST_FEATURE)
}

/// What differs from a default run and changes how the evidence reads, as plain
/// sentences, most consequential first. Empty for a default run by an identified,
/// unmodified build.
pub fn notes(insp: &Inspection) -> Vec<String> {
    let mut out = Vec::new();
    match &insp.tool.build {
        None => out.push(
            "The build of the tool is not identified: the inspection carries no build record."
                .to_string(),
        ),
        Some(b) => {
            match (&b.commit, b.modified) {
                (None, _) => out.push(
                    "The build of the tool is not identified: no commit was recorded when it \
                     was compiled."
                        .to_string(),
                ),
                (Some(c), Some(true)) => out.push(format!(
                    "The tool was built from modified source. Commit {} names where the \
                     source started, not what was compiled.",
                    short(c)
                )),
                _ => {}
            }
            if !has_manifest_feature(b) {
                out.push(
                    "The tool was built without manifest validation, so the manifest layer \
                     could not read a C2PA manifest."
                        .to_string(),
                );
            }
        }
    }
    let Some(run) = &insp.run else {
        out.push(
            "The conditions of this check were not recorded: the options, trust lists and \
             packs it ran with are not known from this report."
                .to_string(),
        );
        return out;
    };
    let o = &run.options;
    if o.reveal {
        out.insert(
            0,
            "Values are shown in full. This report repeats what the file discloses about \
             people, accounts, devices and places; handle it like the file itself."
                .to_string(),
        );
    }
    if o.fetch_remote_manifests {
        out.push(
            "Remote manifests could be fetched. A result that used one depends on a server \
             and on the time of the check; the evidence row that used it says so."
                .to_string(),
        );
    }
    let skipped: Vec<Layer> = ALL_LAYERS
        .into_iter()
        .filter(|l| !o.layers.contains(l))
        .collect();
    if !skipped.is_empty() {
        out.push(format!(
            "Not run: {}. A layer that did not run says nothing about the file.",
            names(&skipped)
        ));
    }
    match o.trust_list.as_str() {
        "none" => out.push(
            "The internal C2PA trust list was disabled, so no signer could be found on it."
                .to_string(),
        ),
        "file" => out.push(
            "The internal C2PA trust list was replaced by a file the operator supplied. \
             A trusted signer is trusted against that file, not the official list."
                .to_string(),
        ),
        _ => {}
    }
    let custom: Vec<&str> = run
        .trust
        .iter()
        .filter(|t| t.origin == "custom")
        .map(|t| t.name.as_str())
        .collect();
    if !custom.is_empty() {
        out.push(format!(
            "Signers were also checked against anchors the operator added ({}).",
            custom.join(", ")
        ));
    }
    if let Some(f) = &o.fingerprints {
        out.push(format!(
            "An extra writer-fingerprint database was merged over the built-in one ({}).",
            f.name
        ));
    }
    out.extend(run.gaps.iter().map(|g| format!("Not established: {g}")));
    out
}

/// [`notes`] as a list placed before the evidence. Empty when there is none.
pub fn notes_html(insp: &Inspection) -> String {
    let notes = notes(insp);
    if notes.is_empty() {
        return String::new();
    }
    let items: String = notes
        .iter()
        .map(|n| format!("<li>{}</li>", html_escape(n)))
        .collect();
    format!("<ul class=\"notes\">{items}</ul>")
}

/// The record in one line, for the collapsed section's summary.
pub fn summary(insp: &Inspection) -> String {
    let tool = &insp.tool;
    let build = match tool.build.as_ref().and_then(|b| b.commit.as_deref()) {
        Some(c) => format!("build {}", short(c)),
        None => "build not identified".to_string(),
    };
    let mut parts = vec![format!("{} {}, {build}", tool.name, tool.version)];
    let Some(run) = &insp.run else {
        parts.push("conditions not recorded".to_string());
        return parts.join(" · ");
    };
    parts.push(trust_summary(&run.trust));
    parts.push(
        if run.options.fetch_remote_manifests {
            "remote manifests could be fetched"
        } else {
            "remote manifests not fetched"
        }
        .to_string(),
    );
    parts.push(
        if run.options.reveal {
            "values shown in full"
        } else {
            "values masked"
        }
        .to_string(),
    );
    parts.join(" · ")
}

/// The trust inputs in a few words: one origin and version when they all share it.
fn trust_summary(trust: &[TrustInput]) -> String {
    let Some(first) = trust.first() else {
        return "no trust list loaded".to_string();
    };
    let same = trust
        .iter()
        .all(|t| t.origin == first.origin && t.version == first.version);
    match (&first.version, same) {
        (Some(v), true) => format!("trust lists: {} ({v})", origin_words(&first.origin)),
        _ => format!("trust lists: {} loaded, listed below", trust.len()),
    }
}

fn kind_words(kind: &str) -> &str {
    match kind {
        "manifest" => "signing certificates",
        "tsa" => "time-stamp authorities",
        "eku_policy" => "permitted key usages",
        other => other,
    }
}

fn origin_words(origin: &str) -> &str {
    match origin {
        "vendored" => "built into the tool",
        "installed" => "installed copy",
        "file" => "operator's file, in place of the official list",
        "custom" => "anchors the operator added",
        other => other,
    }
}

/// Why no trust list is named, in the order the causes exclude each other.
fn no_trust_reason(insp: &Inspection, run: &RunInfo) -> &'static str {
    let built_in = insp.tool.build.as_ref().is_none_or(has_manifest_feature);
    if !built_in {
        "the tool was built without manifest validation"
    } else if !run.options.layers.contains(&Layer::Manifest) {
        "the manifest layer did not run"
    } else if run.gaps.iter().any(|g| g.starts_with("trust:")) {
        "the trust configuration could not be loaded"
    } else if run.options.trust_list == "none" {
        "the internal list was disabled and no anchors were given"
    } else {
        "none was configured"
    }
}

fn row(out: &mut String, label: &str, value_html: &str) {
    // Writing to a String cannot fail.
    let _ = write!(out, "<tr><th>{label}</th><td>{value_html}</td></tr>");
}

/// The whole record as a collapsed section: a summary line, a table of the build
/// and the options, and the trust inputs with their hashes.
pub fn conditions_html(insp: &Inspection) -> String {
    let mut rows = String::new();
    row(&mut rows, "Checked", &html_escape(&insp.created_at));
    row(
        &mut rows,
        "Tool",
        &html_escape(&format!(
            "{} {} (schema {})",
            insp.tool.name, insp.tool.version, insp.schema_version
        )),
    );
    match &insp.tool.build {
        None => row(&mut rows, "Build", "not recorded"),
        Some(b) => {
            let build = match (&b.commit, b.modified) {
                (None, _) => "not identified: no commit was recorded".to_string(),
                (Some(c), Some(true)) => format!(
                    "commit <code>{}</code>; the source differed from it when the tool was built",
                    html_escape(c)
                ),
                (Some(c), Some(false)) => format!(
                    "commit <code>{}</code>; the source matched it",
                    html_escape(c)
                ),
                (Some(c), None) => format!(
                    "commit <code>{}</code>; not recorded whether the source matched it",
                    html_escape(c)
                ),
            };
            row(&mut rows, "Build", &build);
            let features = if b.features.is_empty() {
                "none".to_string()
            } else {
                html_escape(&b.features.join(", "))
            };
            row(&mut rows, "Features", &features);
        }
    }

    let Some(run) = &insp.run else {
        return format!(
            "<details><summary>Conditions of this check: {}.</summary>\
<table class=\"conditions\">{rows}</table>\
<p>The options, trust lists and packs of this check were not recorded.</p>{STATEMENT}</details>",
            html_escape(&summary(insp)),
        );
    };
    let o = &run.options;
    let skipped: Vec<Layer> = ALL_LAYERS
        .into_iter()
        .filter(|l| !o.layers.contains(l))
        .collect();
    let mut layers = names(&o.layers);
    if !skipped.is_empty() {
        let _ = write!(layers, "; not run: {}", names(&skipped));
    }
    row(&mut rows, "Layers run", &layers);
    row(
        &mut rows,
        "Remote manifests",
        if o.fetch_remote_manifests {
            "could be fetched (<code>--fetch-remote-manifests</code>)"
        } else {
            "not fetched"
        },
    );
    row(
        &mut rows,
        "Disclosed values",
        if o.reveal {
            "shown in full (<code>--reveal</code>)"
        } else {
            "masked"
        },
    );
    let mode = match o.trust_list.as_str() {
        "auto" => "the installed copy if there is one, otherwise the one built in (auto)".into(),
        "vendored" => "the one built in (vendored)".into(),
        "none" => "disabled (none)".into(),
        "file" => "replaced by a file the operator supplied (file)".into(),
        other => html_escape(other),
    };
    row(&mut rows, "Internal trust list", &mode);
    if let Some(f) = &o.fingerprints {
        row(
            &mut rows,
            "Extra fingerprint database",
            &format!(
                "{}, SHA-256 <code>{}</code>",
                html_escape(&f.name),
                html_escape(&f.sha256)
            ),
        );
    }
    let packs = if run.packs.is_empty() {
        "none installed".to_string()
    } else {
        run.packs
            .iter()
            .map(|p| {
                format!(
                    "{} {} ({}), archive SHA-256 <code>{}</code>",
                    html_escape(&p.name),
                    html_escape(&p.version),
                    html_escape(&p.tier),
                    html_escape(&p.archive_sha256)
                )
            })
            .collect::<Vec<_>>()
            .join("<br>")
    };
    row(&mut rows, "Model packs", &packs);
    if !run.gaps.is_empty() {
        let gaps: Vec<String> = run.gaps.iter().map(|g| html_escape(g)).collect();
        row(&mut rows, "Not established", &gaps.join("<br>"));
    }

    let trust = if run.trust.is_empty() {
        format!(
            "<p>No trust list was loaded: {}.</p>",
            no_trust_reason(insp, run)
        )
    } else {
        let mut t = String::from(
            "<table class=\"trust\"><tr><th>Used for</th><th>This copy</th><th>File</th>\
<th>Version</th><th>Certificates</th><th>SHA-256</th></tr>",
        );
        for i in &run.trust {
            let mut copy = origin_words(&i.origin).to_string();
            if let Some(at) = &i.fetched_at {
                let _ = write!(copy, ", fetched {at}");
            }
            let _ = write!(
                t,
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><code>{}</code></td></tr>",
                html_escape(kind_words(&i.kind)),
                html_escape(&copy),
                html_escape(&i.name),
                html_escape(i.version.as_deref().unwrap_or("not stated")),
                i.certificates.map_or_else(String::new, |n| n.to_string()),
                html_escape(&i.sha256),
            );
        }
        t.push_str("</table>");
        t
    };
    format!(
        "<details><summary>Conditions of this check: {}.</summary>\
<table class=\"conditions\">{rows}</table><h3>Trust lists</h3>{trust}{STATEMENT}</details>",
        html_escape(&summary(insp)),
    )
}

/// Closes the section: whose statement this is.
const STATEMENT: &str = "<p><small>This section is the tool's own statement about itself and \
its inputs; nothing in the report signs or verifies it. A hash identifies a list, it does not \
vouch for it. Files are named without their directory.</small></p>";

#[cfg(test)]
mod tests {
    use super::*;
    use halftone_core::{AssetInfo, FileRef, Modality, PackRef, RunOptions, ToolInfo};

    const COMMIT: &str = "c146d01f0a0859253d06fb4ab955c75550c6f39a";
    const SNAPSHOT: &str = "c2pa-org/conformance-public@d6dec63476e2 2026-10-26T17:11:49Z";

    fn trust(kind: &str, origin: &str, name: &str) -> TrustInput {
        TrustInput {
            kind: kind.into(),
            origin: origin.into(),
            name: name.into(),
            version: (origin == "vendored").then(|| SNAPSHOT.to_string()),
            fetched_at: None,
            sha256: "b3".repeat(32),
            certificates: (kind != "eku_policy").then_some(32),
        }
    }

    /// A default run by an identified, unmodified build with manifest validation.
    fn plain() -> Inspection {
        Inspection {
            schema_version: "1.5.0".into(),
            tool: ToolInfo {
                name: "halftone".into(),
                version: "0.1.0".into(),
                build: Some(BuildInfo {
                    commit: Some(COMMIT.into()),
                    modified: Some(false),
                    features: vec!["c2pa".into()],
                }),
            },
            asset: AssetInfo {
                path: Some("/Users/someone/cases/photo 1.jpg".into()),
                modality: Modality::Image,
                mime: "image/jpeg".into(),
                sha256: "5c".repeat(32),
                size_bytes: 1_234_567,
            },
            evidence: vec![],
            run: Some(RunInfo {
                options: RunOptions {
                    layers: ALL_LAYERS.to_vec(),
                    trust_list: "auto".into(),
                    fetch_remote_manifests: false,
                    reveal: false,
                    fingerprints: None,
                },
                trust: vec![
                    trust("manifest", "vendored", "C2PA-TRUST-LIST.pem"),
                    trust("tsa", "vendored", "C2PA-TSA-TRUST-LIST.pem"),
                    trust("eku_policy", "vendored", "C2PA-EKU-CONFIG.cfg"),
                ],
                packs: vec![],
                gaps: vec![],
            }),
            created_at: "2026-11-18T10:45:48Z".into(),
        }
    }

    fn run_mut(i: &mut Inspection) -> &mut RunInfo {
        i.run.as_mut().unwrap()
    }

    #[test]
    fn the_file_is_named_without_its_directory_and_sized_readably() {
        let h = file_html(&plain());
        assert!(
            h.contains("<tr><th>Name</th><td>photo 1.jpg</td></tr>"),
            "{h}"
        );
        assert!(!h.contains("someone") && !h.contains("/Users"), "{h}");
        assert!(h.contains("<td>1,234,567 bytes</td>"), "{h}");
        assert!(h.contains(&format!("<code>{}</code>", "5c".repeat(32))));
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
    }

    #[test]
    fn a_file_name_cannot_inject_markup() {
        let mut i = plain();
        i.asset.path = Some("/tmp/<img src=x onerror=alert(1)> & co.jpg".into());
        let h = file_html(&i);
        assert!(!h.contains("<img"), "{h}");
        assert!(
            h.contains("<td>&lt;img src=x onerror=alert(1)&gt; &amp; co.jpg</td>"),
            "{h}"
        );
    }

    #[test]
    fn a_default_run_has_no_notes_and_a_one_line_summary() {
        let i = plain();
        assert_eq!(notes(&i), Vec::<String>::new());
        assert_eq!(notes_html(&i), "");
        assert_eq!(
            summary(&i),
            format!(
                "halftone 0.1.0, build c146d01f0a08 · trust lists: built into the tool \
                 ({SNAPSHOT}) · remote manifests not fetched · values masked"
            )
        );
    }

    #[test]
    fn the_section_is_collapsed_and_says_whose_statement_it_is() {
        let h = conditions_html(&plain());
        assert!(
            h.starts_with("<details><summary>Conditions of this check: halftone 0.1.0"),
            "{h}"
        );
        assert!(!h.contains("<details open"));
        assert!(h.contains("the tool's own statement"), "{h}");
        assert!(
            h.contains(&format!(
                "commit <code>{COMMIT}</code>; the source matched it"
            )),
            "{h}"
        );
        assert!(h.contains("<tr><th>Model packs</th><td>none installed</td></tr>"));
        assert!(h.contains("<tr><th>Disclosed values</th><td>masked</td></tr>"));
        // One row per trust input, in words, with the policy's empty certificate cell.
        assert!(h.contains("<td>signing certificates</td><td>built into the tool</td><td>C2PA-TRUST-LIST.pem</td>"), "{h}");
        assert!(h.contains("<td>time-stamp authorities</td>"));
        assert!(h.contains("<td>permitted key usages</td><td>built into the tool</td><td>C2PA-EKU-CONFIG.cfg</td>"), "{h}");
        assert!(
            h.contains(&format!("<td>{SNAPSHOT}</td><td></td><td><code>")),
            "{h}"
        );
    }

    #[test]
    fn revealed_values_are_the_first_note() {
        let mut i = plain();
        run_mut(&mut i).options.reveal = true;
        run_mut(&mut i).options.fetch_remote_manifests = true;
        let n = notes(&i);
        assert_eq!(n.len(), 2, "{n:?}");
        assert!(n[0].starts_with("Values are shown in full."), "{n:?}");
        assert!(
            n[1].starts_with("Remote manifests could be fetched."),
            "{n:?}"
        );
        assert!(notes_html(&i).starts_with("<ul class=\"notes\"><li>Values are shown in full."));
        assert!(summary(&i).ends_with("remote manifests could be fetched · values shown in full"));
        let h = conditions_html(&i);
        assert!(h.contains("shown in full (<code>--reveal</code>)"), "{h}");
        assert!(h.contains("could be fetched (<code>--fetch-remote-manifests</code>)"));
    }

    #[test]
    fn a_modified_or_unidentified_build_is_noted() {
        let mut i = plain();
        i.tool.build.as_mut().unwrap().modified = Some(true);
        let n = notes(&i);
        assert_eq!(n.len(), 1, "{n:?}");
        assert!(n[0].contains("built from modified source. Commit c146d01f0a08 names"));
        assert!(
            conditions_html(&i).contains("the source differed from it when the tool was built")
        );

        i.tool.build.as_mut().unwrap().commit = None;
        i.tool.build.as_mut().unwrap().modified = None;
        assert!(notes(&i)[0].contains("not identified: no commit was recorded"));
        assert!(summary(&i).starts_with("halftone 0.1.0, build not identified · "));
        assert!(
            conditions_html(&i)
                .contains("<tr><th>Build</th><td>not identified: no commit was recorded</td></tr>")
        );

        i.tool.build = None;
        assert!(notes(&i)[0].contains("carries no build record"));
        assert!(conditions_html(&i).contains("<tr><th>Build</th><td>not recorded</td></tr>"));
    }

    #[test]
    fn layers_that_did_not_run_are_named() {
        let mut i = plain();
        run_mut(&mut i).options.layers = vec![Layer::Container];
        run_mut(&mut i).trust.clear();
        assert_eq!(
            notes(&i),
            [
                "Not run: Manifest, Mark, Blind. A layer that did not run says nothing about the file."
            ]
        );
        let h = conditions_html(&i);
        assert!(
            h.contains("<td>Container; not run: Manifest, Mark, Blind</td>"),
            "{h}"
        );
        assert!(h.contains("<p>No trust list was loaded: the manifest layer did not run.</p>"));
        assert!(summary(&i).contains("no trust list loaded"));
    }

    #[test]
    fn a_trust_list_the_operator_changed_is_noted() {
        let mut none = plain();
        run_mut(&mut none).options.trust_list = "none".into();
        run_mut(&mut none).trust.clear();
        assert!(notes(&none)[0].contains("trust list was disabled"));
        assert!(conditions_html(&none).contains(
            "No trust list was loaded: the internal list was disabled and no anchors were given."
        ));

        let mut swapped = plain();
        run_mut(&mut swapped).options.trust_list = "file".into();
        run_mut(&mut swapped).trust[0] = trust("manifest", "file", "my-roots.pem");
        run_mut(&mut swapped)
            .trust
            .insert(2, trust("manifest", "custom", "ca.pem"));
        let n = notes(&swapped);
        assert!(
            n[0].contains("replaced by a file the operator supplied"),
            "{n:?}"
        );
        assert_eq!(
            n[1],
            "Signers were also checked against anchors the operator added (ca.pem)."
        );
        let h = conditions_html(&swapped);
        assert!(h.contains("<td>operator's file, in place of the official list</td><td>my-roots.pem</td><td>not stated</td>"), "{h}");
        // The inputs no longer share one origin and version, so the summary counts.
        assert!(summary(&swapped).contains("trust lists: 4 loaded, listed below"));
    }

    #[test]
    fn an_installed_list_shows_when_it_was_fetched() {
        let mut i = plain();
        let t = &mut run_mut(&mut i).trust[0];
        t.origin = "installed".into();
        t.version = Some("2026-09-01 (mirror)".into());
        t.fetched_at = Some("2026-09-02T08:00:00Z".into());
        let h = conditions_html(&i);
        assert!(h.contains("<td>installed copy, fetched 2026-09-02T08:00:00Z</td><td>C2PA-TRUST-LIST.pem</td><td>2026-09-01 (mirror)</td>"), "{h}");
        // The automatic choice of the installed copy is the default: no note.
        assert!(notes(&i).is_empty());
    }

    #[test]
    fn extra_inputs_and_gaps_are_shown_in_both_places() {
        let mut i = plain();
        run_mut(&mut i).options.fingerprints = Some(FileRef {
            name: "extra <writers>.json".into(),
            sha256: "ab".repeat(32),
        });
        run_mut(&mut i).packs = vec![PackRef {
            name: "blind-image".into(),
            version: "0.3.0".into(),
            tier: "eval".into(),
            archive_sha256: "cd".repeat(32),
        }];
        run_mut(&mut i).gaps = vec!["packs: the record could not be read".into()];
        let n = notes(&i);
        assert_eq!(n.len(), 2, "{n:?}");
        assert!(n[0].contains("(extra <writers>.json)"));
        assert_eq!(n[1], "Not established: packs: the record could not be read");
        assert!(notes_html(&i).contains("(extra &lt;writers&gt;.json)"));
        let h = conditions_html(&i);
        assert!(h.contains("<th>Extra fingerprint database</th><td>extra &lt;writers&gt;.json, SHA-256 <code>abab"), "{h}");
        assert!(
            h.contains("blind-image 0.3.0 (eval), archive SHA-256 <code>cdcd"),
            "{h}"
        );
        assert!(h.contains(
            "<tr><th>Not established</th><td>packs: the record could not be read</td></tr>"
        ));
    }

    #[test]
    fn a_build_without_manifest_validation_is_noted_and_explains_the_empty_list() {
        let mut i = plain();
        i.tool.build.as_mut().unwrap().features.clear();
        run_mut(&mut i).trust.clear();
        assert!(notes(&i)[0].contains("built without manifest validation"));
        let h = conditions_html(&i);
        assert!(h.contains("<tr><th>Features</th><td>none</td></tr>"), "{h}");
        assert!(
            h.contains("No trust list was loaded: the tool was built without manifest validation.")
        );
    }

    #[test]
    fn an_inspection_without_a_run_record_says_so() {
        let mut i = plain();
        i.run = None;
        let n = notes(&i);
        assert_eq!(n.len(), 1, "{n:?}");
        assert!(n[0].starts_with("The conditions of this check were not recorded"));
        assert!(summary(&i).ends_with("conditions not recorded"));
        let h = conditions_html(&i);
        assert!(h.contains("were not recorded.</p>"), "{h}");
        assert!(!h.contains("<h3>Trust lists</h3>"));
        // The build is known even then, and stays in the table.
        assert!(h.contains(&format!("commit <code>{COMMIT}</code>")));
    }
}
