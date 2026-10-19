//! Provenance lineage across the manifests of one C2PA store (DIFFERENTIAL.md D-013).
//!
//! A C2PA store can hold several manifests: the active one, which describes the
//! last step, and the manifests of its ingredients, each signed when that
//! ingredient was made. Reading only the active manifest misses whatever an
//! earlier step declared. Google Flow can deliver an edit whose active manifest
//! records only a re-encode (`c2pa.opened` + `c2pa.transcoded`,
//! `algorithmicallyEnhanced`) while the generative step sits one ingredient down;
//! a second download of the same result carries the generation as its active
//! manifest. Both copies must get the same finding.
//!
//! Two kinds of ingredient are kept apart:
//! - `parentOf` is lineage: this file's content derives from that ingredient. A
//!   generative declaration anywhere on the `parentOf` chain is a declaration
//!   about this file's content.
//! - `inputTo` and `componentOf` (also the default when no relationship is given)
//!   are inputs: material that went into a step, such as a camera capture handed
//!   to a generator. Their declarations describe the input and are reported as
//!   context, never as this file's source type.
//!
//! The walk reads the JSON of `c2pa::Reader::json()` and nothing else, so it
//! compiles and is tested without the `c2pa` feature. It reports what the store
//! states. c2pa validates ingredient manifests as part of the store, and
//! [`ingredient_failures`] says whether any of them failed, so the rationale can
//! say when a lineage declaration comes from a manifest that did not validate.
//!
//! [`render_timeline`] prints the whole chain for a person, oldest first: who
//! signed each manifest, with which tool, when, and what it says was done (for a
//! Flow edit: a Pixel camera capture, Google's upload transcode, the generation
//! with its SynthID watermark, and a final re-encode). Every line is what that
//! manifest's signer asserts, not an observation of the content.

use crate::source_type::{self, SourceTypeSummary};
use halftone_core::dst::{TermKind, lookup};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

/// Longest chain followed before the walk stops and sets [`Lineage::truncated`].
pub const MAX_DEPTH: usize = 16;

/// One recorded action, as the manifest states it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    /// Action name, e.g. `c2pa.created`.
    pub action: String,
    /// The action's free-text `description`, e.g. "Created by Pixel Camera.".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Term code of the action's `digitalSourceType`, if it declares one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_type: Option<String>,
    /// The action's `softwareAgent`: a string, or `name version` from an object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_agent: Option<String>,
}

/// One manifest on a chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    /// Distance from the start of the chain: 0 is the active manifest (or, in an
    /// input's chain, the input's own manifest), 1 its parent, and so on.
    pub depth: usize,
    /// Manifest label (URN) in the store.
    pub label: String,
    /// Signer, from `signature_info.issuer`.
    pub issuer: Option<String>,
    /// Claim generator: the v1 string, or `name version` of the first
    /// `claim_generator_info` entry.
    pub generator: Option<String>,
    /// The generator's name without its build or version, for display.
    pub generator_name: Option<String>,
    /// Signing time from the signature's time-stamp token (`signature_info.time`);
    /// absent when the signature carries no time-stamp (Canva, D-010). The token's
    /// TSA may be untrusted (Bing, D-012): the time is shown, not vouched for.
    pub signed_at: Option<String>,
    /// Action names recorded in this manifest, in order.
    pub actions: Vec<String>,
    /// The same actions with their descriptions, source types and agents.
    pub events: Vec<Event>,
    /// `digitalSourceType` declarations in this manifest.
    pub source_type: SourceTypeSummary,
}

/// An ingredient that went into a lineage step without being its parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Input {
    /// The ingredient's relationship: `inputTo` or `componentOf`.
    pub relationship: String,
    /// Depth of the lineage step that records this ingredient.
    pub recorded_at: usize,
    /// The input's own parent chain, starting with its manifest.
    pub chain: Vec<Step>,
}

/// The parent lineage of the active manifest, plus the inputs recorded along it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Lineage {
    /// `parentOf` chain from the active manifest (depth 0) back.
    pub steps: Vec<Step>,
    /// Non-parent ingredients recorded by a lineage step, each with its own chain.
    pub inputs: Vec<Input>,
    /// Ingredient manifest labels referenced but absent from the store.
    pub missing: Vec<String>,
    /// The walk stopped at [`MAX_DEPTH`] or at a manifest it had already visited.
    pub truncated: bool,

    /// c2pa reported a validation failure for at least one ingredient manifest
    /// (see [`ingredient_failures`]).
    pub ingredient_failures: bool,
    /// The chain in a few words, oldest first, e.g. "(camera capture → re-encode)
    /// was the input to: generative AI creation (watermark declared) = this file".
    /// Inputs are bracketed and named in words, never only by a different arrow.
    /// Built only from what the manifests state.
    pub summary: Option<String>,
    /// What the chain adds up to, as the signers state it, when it is one of the
    /// cases a reader asks about: a camera capture turned into an AI-derived
    /// image, a camera capture edited with AI, an AI generation with no capture,
    /// or a capture with no AI step. `None` otherwise.
    pub bottom_line: Option<String>,
    /// c2pa's validation state for the store as a whole (`Trusted`, `Valid`,
    /// `Invalid`), copied from the reader's JSON so the timeline can say what was
    /// and was not checked.
    pub validation_state: Option<String>,
    /// The signing certificate's revocation status was checked: c2pa reported
    /// `signingCredential.ocsp.notRevoked`, which with Halftone offline can only
    /// come from a response stapled in the file. Otherwise revocation was not
    /// checked (no network lookup, DIFFERENTIAL.md D-004), and the timeline says so.
    pub revocation_checked: bool,
}

impl Lineage {
    /// Nearest lineage step, the active manifest included, that declares a
    /// generative term.
    pub fn nearest_generative(&self) -> Option<&Step> {
        self.steps.iter().find(|s| s.source_type.declares_ai)
    }

    /// Any manifest on the parent lineage declares a generative term. Inputs do
    /// not count: a generator's input is not the file.
    pub fn declares_ai(&self) -> bool {
        self.nearest_generative().is_some()
    }

    /// Steps back from the active manifest to the nearest manifest that declares
    /// a generative term: `Some(0)` is the active manifest itself, `None` means no
    /// manifest on the parent lineage declares one.
    pub fn generative_depth(&self) -> Option<usize> {
        self.nearest_generative().map(|s| s.depth)
    }

    /// Distinct term codes declared by the manifests behind the active one on its
    /// `parentOf` lineage (depth 1 and further), in first-seen order. Inputs are
    /// not included.
    pub fn parent_codes(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in self.steps.iter().skip(1).flat_map(|s| &s.source_type.codes) {
            if !out.contains(c) {
                out.push(c.clone());
            }
        }
        out
    }

    /// One manifest, nothing behind it: the case the old active-only reading
    /// already described correctly.
    fn is_single(&self) -> bool {
        self.steps.len() <= 1
            && self.inputs.is_empty()
            && self.missing.is_empty()
            && !self.truncated
    }
}

/// Walk the store from its active manifest. `store` is the root object of
/// `c2pa::Reader::json()`. An empty [`Lineage`] means there is no active manifest.
pub fn walk(store: &Value) -> Lineage {
    let mut lin = Lineage::default();
    let Some(active) = store.get("active_manifest").and_then(Value::as_str) else {
        return lin;
    };
    let manifests = store.get("manifests");
    let mut seen = HashSet::new();
    let steps = chain(manifests, active, &mut seen, &mut lin, true);
    lin.steps = steps;
    lin.ingredient_failures = ingredient_failures(store);
    lin.summary = summary(&lin);
    lin.bottom_line = bottom_line(&lin);
    lin.validation_state = store
        .get("validation_state")
        .and_then(Value::as_str)
        .map(str::to_string);
    lin.revocation_checked = has_code(store, "signingCredential.ocsp.notRevoked");
    lin
}

/// Codes that declare a capture of the physical world, with how to say it.
const CAPTURES: [(&str, &str); 7] = [
    ("digitalCapture", "camera capture"),
    ("computationalCapture", "camera capture"),
    ("compositeCapture", "camera capture (composite)"),
    ("negativeFilm", "film scan"),
    ("positiveFilm", "film scan"),
    ("print", "scan of a print"),
    ("screenCapture", "screen capture"),
];

/// Actions that re-save content without changing what it shows.
const REENCODES: [&str; 4] = [
    "c2pa.opened",
    "c2pa.transcoded",
    "c2pa.converted",
    "c2pa.repackaged",
];

/// A step that only re-saves content: opened, transcoded, converted, repackaged.
fn is_reencode_only(step: &Step) -> bool {
    !step.actions.is_empty() && step.actions.iter().all(|a| REENCODES.contains(&a.as_str()))
}

/// A step's declared terms for a sentence. On a re-encode the IPTC label (e.g.
/// "algorithmically enhanced capture" for Google's transcodes) reads like an
/// edit of the picture, so the code is given as declared, without the label.
fn step_terms(step: &Step) -> Vec<(String, String)> {
    step.source_type
        .codes
        .iter()
        .map(|c| {
            let shown = if is_reencode_only(step) {
                c.clone()
            } else {
                terms(std::slice::from_ref(c))
            };
            (c.clone(), shown)
        })
        .collect()
}

fn capture_of(step: &Step) -> Option<&'static str> {
    step.source_type
        .codes
        .iter()
        .find_map(|c| CAPTURES.iter().find(|(k, _)| k == c).map(|(_, v)| *v))
}

fn declares_watermark(step: &Step) -> bool {
    step.events.iter().any(|e| {
        e.action == "c2pa.watermarked"
            || e.description
                .as_deref()
                .is_some_and(|d| d.to_lowercase().contains("watermark"))
    })
}

/// One manifest in a few words.
fn stage(step: &Step) -> String {
    let codes = &step.source_type.codes;
    let created = step.actions.iter().any(|a| a == "c2pa.created");
    let base = if codes
        .iter()
        .any(|c| c == "compositeWithTrainedAlgorithmicMedia")
    {
        "composite including generative AI".to_string()
    } else if step.source_type.declares_ai && created {
        "generative AI creation".to_string()
    } else if step.source_type.declares_ai {
        "generative AI edit".to_string()
    } else if let Some(c) = capture_of(step) {
        c.to_string()
    } else if is_reencode_only(step) {
        "re-encode".to_string()
    } else if let Some(a) = step
        .actions
        .iter()
        .find(|a| !REENCODES.contains(&a.as_str()))
    {
        plain_action(a).unwrap_or(a.as_str()).to_string()
    } else {
        "signed record, no actions".to_string()
    };
    if declares_watermark(step) {
        format!("{base} (watermark declared)")
    } else {
        base
    }
}

fn summary(lin: &Lineage) -> Option<String> {
    if lin.steps.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    for step in lin.steps.iter().rev() {
        let ins: Vec<String> = lin
            .inputs
            .iter()
            .filter(|i| i.recorded_at == step.depth && !i.chain.is_empty())
            .map(|i| {
                i.chain
                    .iter()
                    .rev()
                    .map(stage)
                    .collect::<Vec<_>>()
                    .join(" → ")
            })
            .collect();
        let mut seg = match ins.len() {
            0 => String::new(),
            1 => format!("({}) was the input to: ", ins[0]),
            _ => format!(
                "{} were the inputs to: ",
                ins.iter()
                    .map(|x| format!("({x})"))
                    .collect::<Vec<_>>()
                    .join(" and ")
            ),
        };
        seg.push_str(&stage(step));
        if step.depth == 0 {
            seg.push_str(" = this file");
        }
        parts.push(seg);
    }
    Some(parts.join(" → "))
}

fn bottom_line(lin: &Lineage) -> Option<String> {
    let generative = lin.nearest_generative();
    let capture_input = lin
        .inputs
        .iter()
        .flat_map(|i| &i.chain)
        .find_map(capture_of);
    // A capture on the lineage older than the generative step (or anywhere, when
    // there is no generative step).
    let capture_lineage = lin
        .steps
        .iter()
        .filter(|s| match generative {
            Some(g) => s.depth > g.depth,
            None => true,
        })
        .find_map(capture_of);
    let unverified = if lin.ingredient_failures {
        " (earlier steps reported as stated; c2pa found validation failures in them)"
    } else {
        ""
    };
    let line = match (generative, capture_input, capture_lineage) {
        (Some(g), Some(c), _) => format!(
            "As the signers state it: a {c} was used as input to generative AI, and \
             this file derives from that generation: an AI-derived image, not an \
             unedited {c}{unverified}.{}{}",
            signer_class(g),
            extent(c)
        ),
        (Some(g), None, Some(c)) => format!(
            "As the signers state it: a {c} was edited with generative AI, and this \
             file derives from that edit{unverified}.{}{}",
            signer_class(g),
            extent(c)
        ),
        (Some(_), None, None) => format!(
            "As the signers state it: this file derives from generative AI; no camera \
             or scan capture is recorded in the store{unverified}."
        ),
        (None, _, Some(c)) => format!(
            "As the signers state it: a {c}, with no generative AI step recorded{unverified}."
        ),
        (None, Some(c), None) => format!(
            "As the signers state it: a {c} went into this file as an input; no \
             generative AI step is recorded{unverified}."
        ),
        (None, None, None) => return None,
    };
    Some(line)
}

/// The editor's category, as the generative step's own term puts it. IPTC's
/// trainedAlgorithmicMedia ("created using generative AI") is an AI generation;
/// compositeWithTrainedAlgorithmicMedia ("a mix of elements, at least one
/// generated") is an AI-edited image. Which term the signer chose is its statement,
/// not a measurement, and the sentence says so; `extent` says what no record can.
fn signer_class(g: &Step) -> &'static str {
    let has = |code: &str| g.source_type.codes.iter().any(|c| c == code);
    if has("compositeWithTrainedAlgorithmicMedia") {
        " By the signer's own term, a mix of elements with at least one generated \
         (compositeWithTrainedAlgorithmicMedia), this is an AI-edited image, not an AI \
         generation (trainedAlgorithmicMedia)."
    } else if has("trainedAlgorithmicMedia") {
        " By the signer's own term, created by generative AI (trainedAlgorithmicMedia), \
         this is an AI generation that used the photo as input, not an AI-edited photo \
         (compositeWithTrainedAlgorithmicMedia)."
    } else {
        ""
    }
}

/// The generative claim in the signer's own term, so a rationale never blurs the
/// two: "created by generative AI" (trainedAlgorithmicMedia) is not "a composite
/// that includes generative AI" (compositeWithTrainedAlgorithmicMedia). Only when a
/// manifest declares both, or neither by code, does it fall back to naming both.
fn ai_phrase(codes: &[String]) -> &'static str {
    let has = |code: &str| codes.iter().any(|c| c == code);
    match (
        has("trainedAlgorithmicMedia"),
        has("compositeWithTrainedAlgorithmicMedia"),
    ) {
        (true, false) => "created by generative AI",
        (false, true) => "a composite that includes generative AI",
        _ => "generated or composited with generative AI",
    }
}

/// What no record in a C2PA store can say: how much of the input survived.
fn extent(capture: &str) -> String {
    format!(
        " No record says how much of the {capture} remains in the image; only comparing \
         the two can tell."
    )
}

/// Plain words for the C2PA standard actions; `None` for anything else.
fn plain_action(action: &str) -> Option<&'static str> {
    Some(match action {
        "c2pa.created" => "created",
        "c2pa.opened" => "opened an existing file",
        "c2pa.edited" => "edited",
        "c2pa.transcoded" => "re-encoded",
        "c2pa.converted" => "converted to another format",
        "c2pa.repackaged" => "repackaged, content unchanged",
        "c2pa.placed" => "placed another image in",
        "c2pa.cropped" => "cropped",
        "c2pa.resized" => "resized",
        "c2pa.orientation" => "rotated or flipped",
        "c2pa.color_adjustments" => "colour adjusted",
        "c2pa.filtered" => "filtered",
        "c2pa.drawing" => "drawn on",
        "c2pa.removed" => "removed content",
        "c2pa.redacted" => "redacted",
        "c2pa.watermarked" => "watermarked",
        "c2pa.published" => "published",
        "c2pa.translated" => "translated",
        "c2pa.unknown" => "unspecified action",
        _ => return None,
    })
}

/// Plain words for an ingredient relationship that is not lineage.
fn plain_relationship(rel: &str) -> String {
    match rel {
        "inputTo" => "went into the next step as its input (inputTo)".into(),
        "componentOf" => "was placed into the next step as a component (componentOf)".into(),
        other => format!("went into the next step ({other})"),
    }
}

/// Any validation code with this name in the active manifest's results, in any
/// bucket (c2pa 0.91 logs a verified stapled OCSP response as success, 0.90 as
/// informational; DIFFERENTIAL.md D-011).
fn has_code(store: &Value, code: &str) -> bool {
    ["success", "informational", "failure"]
        .iter()
        .any(|bucket| {
            store
                .pointer(&format!("/validation_results/activeManifest/{bucket}"))
                .and_then(Value::as_array)
                .is_some_and(|codes| {
                    codes
                        .iter()
                        .any(|c| c.get("code").and_then(Value::as_str) == Some(code))
                })
        })
}

/// c2pa reported a validation failure for at least one ingredient manifest
/// (`validation_results.ingredientDeltas[].validationDeltas.failure`).
pub fn ingredient_failures(store: &Value) -> bool {
    store
        .pointer("/validation_results/ingredientDeltas")
        .and_then(Value::as_array)
        .is_some_and(|deltas| {
            deltas.iter().any(|d| {
                d.pointer("/validationDeltas/failure")
                    .and_then(Value::as_array)
                    .is_some_and(|f| !f.is_empty())
            })
        })
}

/// Follow `parentOf` from `start`. With `collect_inputs`, every other ingredient
/// with a manifest is walked as its own chain and recorded in `lin.inputs`; input
/// chains do not collect inputs of their own.
fn chain(
    manifests: Option<&Value>,
    start: &str,
    seen: &mut HashSet<String>,
    lin: &mut Lineage,
    collect_inputs: bool,
) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut next = Some(start.to_string());
    while let Some(label) = next.take() {
        if steps.len() >= MAX_DEPTH || !seen.insert(label.clone()) {
            lin.truncated = true;
            break;
        }
        let Some(m) = manifests.and_then(|ms| ms.get(label.as_str())) else {
            lin.missing.push(label);
            break;
        };
        let depth = steps.len();
        for ing in m
            .get("ingredients")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            // An ingredient without a manifest of its own has nothing to walk.
            let Some(target) = ing.get("active_manifest").and_then(Value::as_str) else {
                continue;
            };
            // C2PA's default relationship is componentOf.
            match ing
                .get("relationship")
                .and_then(Value::as_str)
                .unwrap_or("componentOf")
            {
                "parentOf" => {
                    // C2PA allows one parent; a second parentOf is not followed.
                    if next.is_none() {
                        next = Some(target.to_string());
                    }
                }
                rel if collect_inputs => {
                    let sub = chain(manifests, target, seen, lin, false);
                    lin.inputs.push(Input {
                        relationship: rel.to_string(),
                        recorded_at: depth,
                        chain: sub,
                    });
                }
                _ => {}
            }
        }
        let events = events(m);
        steps.push(Step {
            depth,
            issuer: m
                .pointer("/signature_info/issuer")
                .and_then(Value::as_str)
                .map(str::to_string),
            generator: generator(m),
            generator_name: generator_name(m),
            signed_at: m
                .pointer("/signature_info/time")
                .and_then(Value::as_str)
                .map(str::to_string),
            actions: events.iter().map(|e| e.action.clone()).collect(),
            events,
            source_type: source_type::find(m),
            label,
        });
    }
    steps
}

fn events(m: &Value) -> Vec<Event> {
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    m.get("assertions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|a| {
            a.get("label")
                .and_then(Value::as_str)
                .is_some_and(|l| l.starts_with("c2pa.actions"))
        })
        .filter_map(|a| a.pointer("/data/actions").and_then(Value::as_array))
        .flatten()
        .filter_map(|x| {
            Some(Event {
                action: text(x, "action")?,
                description: text(x, "description"),
                source_type: x
                    .get("digitalSourceType")
                    .and_then(Value::as_str)
                    .map(halftone_core::dst::code_of),
                software_agent: x.get("softwareAgent").and_then(name_version),
            })
        })
        .collect()
}

/// A string as is, or `name version` from an object with those fields.
fn name_version(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    let name = v.get("name").and_then(Value::as_str)?;
    Some(match v.get("version").and_then(Value::as_str) {
        Some(ver) => format!("{name} {ver}"),
        None => name.to_string(),
    })
}

fn generator_name(m: &Value) -> Option<String> {
    m.pointer("/claim_generator_info/0/name")
        .and_then(Value::as_str)
        .or_else(|| m.get("claim_generator").and_then(Value::as_str))
        .map(str::to_string)
}

fn generator(m: &Value) -> Option<String> {
    m.get("claim_generator")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| m.pointer("/claim_generator_info/0").and_then(name_version))
}

fn terms(codes: &[String]) -> String {
    codes
        .iter()
        .map(|c| match lookup(c) {
            Some(t) => format!("{} ({})", t.code, t.label),
            None => format!("{c} (not in the known vocabulary)"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn steps_back(depth: usize) -> String {
    match depth {
        1 => "one step back".into(),
        n => format!("{n} steps back"),
    }
}

/// Rationale sentences for the store's source-type declarations.
///
/// For a single manifest with nothing behind it this is exactly
/// [`source_type::describe`], so single-manifest files read as before. Otherwise
/// it says which manifest declares what: the active one, the nearest generative
/// step in the parent lineage, and each input.
pub fn describe(active: &SourceTypeSummary, lin: &Lineage, ingredient_failures: bool) -> String {
    if lin.is_single() {
        return source_type::describe(active);
    }
    let mut out = String::new();

    let own_actions = lin
        .steps
        .first()
        .map(|s| s.actions.join(", "))
        .unwrap_or_default();
    // "Active manifest" is C2PA's term for the record signed with this file.
    if active.codes.is_empty() {
        out.push_str(" This file's own manifest (the active one) declares no digitalSourceType.");
    } else if active.declares_ai {
        out.push_str(&format!(
            " This file's own manifest (the active one) declares digitalSourceType = {}: \
             the signer asserts the content is {}.",
            terms(&active.codes),
            ai_phrase(&active.codes)
        ));
    } else if own_actions.is_empty() {
        out.push_str(&format!(
            " This file's own manifest (the active one) declares digitalSourceType = {} \
             for its own step.",
            terms(&active.codes)
        ));
    } else if lin.steps.first().is_some_and(is_reencode_only) {
        out.push_str(&format!(
            " This file's own manifest (the active one) declares digitalSourceType = {} \
             for its own step, a re-encode ({own_actions}).",
            active.codes.join(", ")
        ));
    } else {
        out.push_str(&format!(
            " This file's own manifest (the active one) declares digitalSourceType = {} \
             for its own step ({own_actions}).",
            terms(&active.codes)
        ));
    }

    if !active.declares_ai {
        match lin.steps.iter().skip(1).find(|s| s.source_type.declares_ai) {
            Some(s) => out.push_str(&format!(
                " Its parent lineage in the same store goes further: the manifest {}{} \
                 declares digitalSourceType = {}{}, so this file's content derives from \
                 content the signer asserts is {}.",
                steps_back(s.depth),
                s.issuer
                    .as_deref()
                    .map(|i| format!(", signed by {i},"))
                    .unwrap_or_default(),
                terms(&s.source_type.codes),
                if s.actions.is_empty() {
                    String::new()
                } else {
                    format!(" on {}", s.actions.join(", "))
                },
                ai_phrase(&s.source_type.codes),
            )),
            None if lin.steps.len() > 1 => out.push_str(&format!(
                " No manifest in its parent lineage ({} more) declares a generative term.",
                lin.steps.len() - 1
            )),
            None => {}
        }
    }

    for input in &lin.inputs {
        let mut seen: Vec<String> = Vec::new();
        let mut codes: Vec<String> = Vec::new();
        for (code, shown) in input.chain.iter().flat_map(step_terms) {
            if !seen.contains(&code) {
                seen.push(code);
                codes.push(shown);
            }
        }
        if codes.is_empty() {
            out.push_str(&format!(
                " The store also records what went into that step: an ingredient used as \
                 {} whose provenance declares no digitalSourceType.",
                input.relationship
            ));
        } else {
            out.push_str(&format!(
                " The store also records what went into that step: an ingredient used as \
                 {} whose provenance declares {}. That input is part of how this file was \
                 made; its declaration describes the input, not this file's final content.",
                input.relationship,
                codes.join(", ")
            ));
        }
    }

    if !lin.missing.is_empty() {
        out.push_str(&format!(
            " {} referenced ingredient manifest(s) are not in the store, so the lineage \
             may be incomplete.",
            lin.missing.len()
        ));
    }
    if lin.truncated {
        out.push_str(&format!(
            " The lineage walk stopped at {MAX_DEPTH} steps or at a repeated manifest."
        ));
    }
    if ingredient_failures {
        out.push_str(
            " c2pa reported validation failures for at least one ingredient manifest, so \
             declarations behind the active manifest are reported as the store states them, \
             not as verified (see validation_results.ingredientDeltas).",
        );
    }
    out
}

/// The provenance chain for a person, oldest first, from the `lineage` object in
/// the c2pa source's details (the serialized [`Lineage`]). Works on the JSON so
/// that anything holding a report can render it; returns an empty string when
/// there is no chain to show.
///
/// Inputs are printed just before the step that recorded them, indented, so the
/// order reads as it happened: the camera capture, its upload, the generation
/// that took it as input, and whatever came after. Every line is what that
/// manifest's signer asserts.
pub fn render_timeline(lineage: &Value) -> String {
    let steps = lineage
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if steps.is_empty() {
        return String::new();
    }
    let inputs = lineage
        .get("inputs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let count = steps.len()
        + inputs
            .iter()
            .filter_map(|i| i.get("chain").and_then(Value::as_array))
            .map(Vec::len)
            .sum::<usize>();
    let mut out = format!(
        "  provenance: {count} signed record(s) (C2PA manifests) in this file, oldest \
         first; each line is what that record's signer states, not a check of the image\n"
    );
    if let Some(b) = lineage.get("bottom_line").and_then(Value::as_str) {
        out.push_str(&format!("    {b}\n"));
    }
    // What the signatures can and cannot establish, right under the bottom line,
    // so a signed statement is not read as a proven history.
    // One "Not checked" sentence: the history, and revocation unless the file
    // carries a stapled response (Halftone never looks a certificate up online).
    let revoked_clause = if lineage.get("revocation_checked").and_then(Value::as_bool) == Some(true)
    {
        ". A revocation response stapled in the file says the signing certificate was not revoked."
    } else {
        ", or whether any signing certificate was revoked (no online lookup, and the file \
         carries no revocation response)."
    };
    const NOT_CHECKED: &str = "Not checked: whether the recorded steps happened as stated.";
    const NOT_STATED: &str = "Not checked: whether the recorded steps happened as stated";
    let checked = match lineage.get("validation_state").and_then(Value::as_str) {
        Some("Trusted") => format!(
            "checked: the signatures validate, this file's signer is on the trust list, and \
             the file is unchanged since its record was signed. {NOT_STATED}{revoked_clause}"
        ),
        Some("Valid") => format!(
            "checked: the signatures validate and the file is unchanged since its record was \
             signed; this file's signer is not on the configured trust lists. \
             {NOT_STATED}{revoked_clause}"
        ),
        Some("Invalid") => "checked: this file's record does not validate (the c2pa line says \
             why; an expired certificate is not tampering). Treat every line below as \
             unverified."
            .to_string(),
        _ => format!("{NOT_CHECKED} The c2pa line gives the signature result."),
    };
    out.push_str(&format!("    {checked}\n"));
    let earlier = count - 1;
    if earlier > 0 {
        if lineage.get("ingredient_failures").and_then(Value::as_bool) == Some(true) {
            out.push_str(&format!(
                "    earlier records: c2pa reported validation failures in at least one of the \
                 {earlier} earlier record(s); their lines are shown as stated, not as verified.\n"
            ));
        } else {
            out.push_str(&format!(
                "    earlier records: c2pa found no validation failures in the {earlier} \
                 earlier record(s); their revocation status was not checked.\n"
            ));
        }
    }
    if let Some(s) = lineage.get("summary").and_then(Value::as_str) {
        out.push_str(&format!("    in short: {s}\n"));
    }
    for step in steps.iter().rev() {
        let depth = step.get("depth").and_then(Value::as_u64).unwrap_or(0);
        for input in inputs
            .iter()
            .filter(|i| i.get("recorded_at").and_then(Value::as_u64) == Some(depth))
        {
            let rel = input
                .get("relationship")
                .and_then(Value::as_str)
                .unwrap_or("componentOf");
            out.push_str(&format!("    ┌ {}\n", plain_relationship(rel)));
            let chain = input
                .get("chain")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for s in chain.iter().rev() {
                out.push_str(&step_lines(s, "    │ ", None));
            }
            out.push_str("    └\n");
        }
        let place = match depth {
            0 => "this file: the record signed with it (active manifest)".to_string(),
            1 => "one step before this file (its parent)".to_string(),
            n => format!("{n} steps before this file"),
        };
        out.push_str(&step_lines(step, "    ", Some(&place)));
    }
    let flag = |k: &str| lineage.get(k).and_then(Value::as_bool) == Some(true);
    if lineage
        .get("missing")
        .and_then(Value::as_array)
        .is_some_and(|m| !m.is_empty())
    {
        out.push_str("    ! referenced ingredient manifests are missing from the store\n");
    }
    if flag("truncated") {
        out.push_str(&format!(
            "    ! chain cut at {MAX_DEPTH} steps or at a repeated manifest\n"
        ));
    }
    out
}

fn step_lines(step: &Value, indent: &str, place: Option<&str>) -> String {
    let text = |k: &str| step.get(k).and_then(Value::as_str);
    let mut head: Vec<String> = Vec::new();
    if let Some(p) = place {
        head.push(p.to_string());
    }
    head.push(match text("issuer") {
        Some(i) => format!("signed by {i}"),
        None => "signer unknown".to_string(),
    });
    if let Some(g) = text("generator_name").or_else(|| text("generator")) {
        head.push(format!("with {g}"));
    }
    if let Some(t) = text("signed_at") {
        head.push(t.to_string());
    }
    let bullet = if place.is_some() { "●" } else { "○" };
    let mut out = format!("{indent}{bullet} {}\n", head.join(" · "));
    for e in step
        .get("events")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mut parts: Vec<String> = Vec::new();
        if let Some(a) = e.get("action").and_then(Value::as_str) {
            parts.push(match plain_action(a) {
                Some(p) => format!("{p} ({a})"),
                None => a.to_string(),
            });
        }
        if let Some(c) = e.get("source_type").and_then(Value::as_str) {
            let reencode = e
                .get("action")
                .and_then(Value::as_str)
                .is_some_and(|a| REENCODES.contains(&a));
            parts.push(match lookup(c) {
                // On a re-encode the label ("algorithmically enhanced capture") reads
                // like an edit of the picture; show the term as declared.
                _ if reencode => match lookup(c).map(|t| t.kind) {
                    Some(TermKind::NonGenerative) => format!("declared non-generative ({c})"),
                    Some(TermKind::Generative) => format!("declared generative AI ({c})"),
                    _ => format!("declared {c}"),
                },
                // Plain words first, the declared term in brackets for anyone checking
                // it against the IPTC list.
                Some(t) => format!("{} ({})", t.label, t.code),
                None => c.to_string(),
            });
        }
        if let Some(d) = e.get("description").and_then(Value::as_str) {
            parts.push(format!("\"{d}\""));
        }
        if let Some(sa) = e.get("software_agent").and_then(Value::as_str) {
            parts.push(format!("by {sa}"));
        }
        out.push_str(&format!("{indent}    {}\n", parts.join(" · ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DST: &str = "http://cv.iptc.org/newscodes/digitalsourcetype/";

    fn action(name: &str, code: Option<&str>) -> Value {
        match code {
            Some(c) => json!({"action": name, "digitalSourceType": format!("{DST}{c}")}),
            None => json!({"action": name}),
        }
    }

    fn described(name: &str, code: Option<&str>, description: &str) -> Value {
        let mut a = action(name, code);
        a["description"] = json!(description);
        a
    }

    fn manifest(actions: Vec<Value>, ingredients: Value) -> Value {
        json!({
            "signature_info": {"issuer": "Google LLC", "time": "2026-10-14T05:55:00+00:00"},
            "claim_generator_info": [{"name": "Google C2PA Core Generator Library", "version": "1"}],
            "assertions": [{"label": "c2pa.actions.v2", "data": {"actions": actions}}],
            "ingredients": ingredients,
        })
    }

    fn camera() -> Value {
        manifest(
            vec![described(
                "c2pa.created",
                Some("computationalCapture"),
                "Created by Pixel Camera.",
            )],
            json!([]),
        )
    }

    fn upload() -> Value {
        manifest(
            vec![
                action("c2pa.opened", None),
                action("c2pa.transcoded", Some("algorithmicallyEnhanced")),
            ],
            json!([{"relationship": "parentOf", "active_manifest": "cam"}]),
        )
    }

    fn generation() -> Value {
        manifest(
            vec![
                described(
                    "c2pa.created",
                    Some("trainedAlgorithmicMedia"),
                    "Created by Google Generative AI.",
                ),
                described(
                    "c2pa.edited",
                    Some("trainedAlgorithmicMedia"),
                    "Applied imperceptible SynthID watermark.",
                ),
            ],
            json!([{"relationship": "inputTo", "active_manifest": "up"}]),
        )
    }

    /// Flow E1 and the E2 re-download: the generation is the active manifest.
    fn flow_e1() -> Value {
        json!({
            "active_manifest": "gen",
            "manifests": {"gen": generation(), "up": upload(), "cam": camera()},
        })
    }

    /// Flow E2, first download: one more transcode on top of the generation.
    fn flow_e2() -> Value {
        let mut store = flow_e2_store();
        store["validation_state"] = json!("Trusted");
        store
    }

    fn flow_e2_store() -> Value {
        let top = manifest(
            vec![
                action("c2pa.opened", None),
                action("c2pa.transcoded", Some("algorithmicallyEnhanced")),
            ],
            json!([{"relationship": "parentOf", "active_manifest": "gen"}]),
        );
        json!({
            "active_manifest": "top",
            "manifests": {"top": top, "gen": generation(), "up": upload(), "cam": camera()},
        })
    }

    fn active_summary(store: &Value) -> SourceTypeSummary {
        let label = store["active_manifest"].as_str().unwrap();
        source_type::find(&store["manifests"][label])
    }

    fn labels(steps: &[Step]) -> Vec<&str> {
        steps.iter().map(|s| s.label.as_str()).collect()
    }

    #[test]
    fn e2_generative_step_is_found_one_parent_back() {
        let store = flow_e2();
        let lin = walk(&store);
        assert_eq!(labels(&lin.steps), ["top", "gen"]);
        assert!(lin.declares_ai());
        assert_eq!(lin.nearest_generative().unwrap().depth, 1);
        assert_eq!(lin.inputs.len(), 1);
        assert_eq!(lin.inputs[0].relationship, "inputTo");
        assert_eq!(lin.inputs[0].recorded_at, 1);
        assert_eq!(labels(&lin.inputs[0].chain), ["up", "cam"]);
    }

    #[test]
    fn summary_fields_for_the_batch_row() {
        let e2 = walk(&flow_e2());
        assert_eq!(e2.generative_depth(), Some(1));
        assert_eq!(e2.parent_codes(), ["trainedAlgorithmicMedia"]);
        let e1 = walk(&flow_e1());
        assert_eq!(e1.generative_depth(), Some(0));
        // The camera capture is an input, not lineage.
        assert!(e1.parent_codes().is_empty());
        let plain = walk(&json!({"active_manifest": "cam", "manifests": {"cam": camera()}}));
        assert_eq!(plain.generative_depth(), None);
    }

    #[test]
    fn e2_rationale_names_the_layer_and_does_not_say_non_generative() {
        let store = flow_e2();
        let d = describe(&active_summary(&store), &walk(&store), false);
        assert!(d.contains("This file's own manifest (the active one)"));
        assert!(d.contains(
            "declares digitalSourceType = algorithmicallyEnhanced for its own step, a re-encode \
             (c2pa.opened, c2pa.transcoded)"
        ));
        assert!(!d.contains("algorithmically enhanced capture"));
        assert!(d.contains("one step back, signed by Google LLC,"));
        assert!(d.contains("trainedAlgorithmicMedia (created using generative AI)"));
        assert!(d.contains("content the signer asserts is created by generative AI."));
        assert!(!d.contains("generated or composited"));
        assert!(d.contains("That input is part of how this file was made"));
        assert!(!d.contains("non-generative source"));
    }

    #[test]
    fn e1_and_e2_agree_that_the_content_is_generative() {
        for store in [flow_e1(), flow_e2()] {
            let active = active_summary(&store);
            assert!(active.declares_ai || walk(&store).declares_ai());
        }
    }

    #[test]
    fn camera_input_is_context_not_the_files_source_type() {
        let store = flow_e1();
        let lin = walk(&store);
        assert_eq!(labels(&lin.steps), ["gen"]);
        assert_eq!(labels(&lin.inputs[0].chain), ["up", "cam"]);
        let d = describe(&active_summary(&store), &lin, false);
        assert!(d.starts_with(
            " This file's own manifest (the active one) declares digitalSourceType = \
             trainedAlgorithmicMedia"
        ));
        assert!(d.contains("used as inputTo"));
        assert!(d.contains("computationalCapture"));
    }

    #[test]
    fn capture_behind_an_input_never_makes_a_file_generative() {
        // A transcode whose only ingredient is an input carrying a generative
        // declaration: inputs are not lineage.
        let store = json!({
            "active_manifest": "top",
            "manifests": {
                "top": manifest(
                    vec![action("c2pa.transcoded", Some("algorithmicallyEnhanced"))],
                    json!([{"relationship": "componentOf", "active_manifest": "gen"}]),
                ),
                "gen": manifest(vec![action("c2pa.created", Some("trainedAlgorithmicMedia"))], json!([])),
            }
        });
        let lin = walk(&store);
        assert!(!lin.declares_ai());
        assert_eq!(lin.inputs.len(), 1);
    }

    #[test]
    fn single_manifest_reads_exactly_as_before() {
        let store = json!({"active_manifest": "cam", "manifests": {"cam": camera()}});
        let active = active_summary(&store);
        assert_eq!(
            describe(&active, &walk(&store), false),
            source_type::describe(&active)
        );
    }

    #[test]
    fn non_generative_lineage_says_so() {
        let store = json!({
            "active_manifest": "up",
            "manifests": {"up": upload(), "cam": camera()},
        });
        let lin = walk(&store);
        assert!(!lin.declares_ai());
        let d = describe(&active_summary(&store), &lin, false);
        assert!(
            d.contains("No manifest in its parent lineage (1 more) declares a generative term.")
        );
    }

    #[test]
    fn missing_relationship_is_component_of() {
        let store = json!({
            "active_manifest": "a",
            "manifests": {
                "a": manifest(vec![], json!([{"active_manifest": "cam"}])),
                "cam": camera(),
            }
        });
        let lin = walk(&store);
        assert_eq!(lin.steps.len(), 1);
        assert_eq!(lin.inputs[0].relationship, "componentOf");
    }

    #[test]
    fn cycle_is_cut_and_reported() {
        let store = json!({
            "active_manifest": "a",
            "manifests": {
                "a": manifest(vec![], json!([{"relationship": "parentOf", "active_manifest": "b"}])),
                "b": manifest(vec![], json!([{"relationship": "parentOf", "active_manifest": "a"}])),
            }
        });
        let lin = walk(&store);
        assert_eq!(labels(&lin.steps), ["a", "b"]);
        assert!(lin.truncated);
        assert!(describe(&SourceTypeSummary::default(), &lin, false).contains("stopped"));
    }

    #[test]
    fn missing_manifest_is_reported() {
        let store = json!({
            "active_manifest": "a",
            "manifests": {
                "a": manifest(vec![], json!([{"relationship": "parentOf", "active_manifest": "gone"}])),
            }
        });
        let lin = walk(&store);
        assert_eq!(lin.missing, ["gone"]);
        assert!(describe(&SourceTypeSummary::default(), &lin, false).contains("not in the store"));
    }

    #[test]
    fn timeline_reads_oldest_first_with_the_input_before_its_generation() {
        let lin = walk(&flow_e2());
        let t = render_timeline(&serde_json::to_value(&lin).unwrap());
        assert!(t.starts_with("  provenance: 4 signed record(s) (C2PA manifests)"));
        let at = |needle: &str| t.find(needle).unwrap_or_else(|| panic!("{needle} in\n{t}"));
        // bottom line and summary first, then the steps oldest first
        let bottom = at("As the signers state it:");
        let short = at("in short: ");
        let input = at("went into the next step as its input (inputTo)");
        let camera = at("\"Created by Pixel Camera.\"");
        let generation = at("\"Created by Google Generative AI.\"");
        let synthid = at("\"Applied imperceptible SynthID watermark.\"");
        let this_file = at("this file: the record signed with it (active manifest)");
        assert!(bottom < short && short < input);
        assert!(input < camera && camera < generation && generation < synthid);
        assert!(synthid < this_file);
        assert!(t.contains("one step before this file (its parent) · signed by Google LLC"));
        assert!(t.contains("with Google C2PA Core Generator Library · 2026-10-14T05:55:00+00:00"));
        assert!(t.contains(
            "created (c2pa.created) · created using generative AI (trainedAlgorithmicMedia)"
        ));
        assert!(t.contains("re-encoded (c2pa.transcoded)"));
    }

    #[test]
    fn timeline_says_what_is_checked_and_what_is_not() {
        let lin = walk(&flow_e2());
        assert_eq!(lin.validation_state.as_deref(), Some("Trusted"));
        let t = render_timeline(&serde_json::to_value(&lin).unwrap());
        let at = |needle: &str| t.find(needle).unwrap_or_else(|| panic!("{needle} in\n{t}"));
        let bottom = at("As the signers state it:");
        let checked =
            at("checked: the signatures validate, this file's signer is on the trust list");
        let not_checked = at("Not checked: whether the recorded steps happened as stated");
        let earlier = at(
            "earlier records: c2pa found no validation failures in the 3 earlier record(s); their \
             revocation status was not checked.",
        );
        assert!(t.contains(
            "Not checked: whether the recorded steps happened as stated, or whether any signing \
             certificate was revoked (no online lookup, and the file carries no revocation response)."
        ));
        let short = at("in short: ");
        assert!(
            bottom < checked && checked < not_checked && not_checked < earlier && earlier < short
        );
        // e1: 2 earlier records (the upload and the camera capture, both inputs)
        let t1 = render_timeline(&serde_json::to_value(walk(&flow_e1())).unwrap());
        assert!(t1.contains("in the 2 earlier record(s);"));
    }

    #[test]
    fn checked_line_follows_the_validation_state() {
        let line = |state: Option<&str>| {
            let mut store = flow_e2_store();
            if let Some(s) = state {
                store["validation_state"] = json!(s);
            }
            render_timeline(&serde_json::to_value(walk(&store)).unwrap())
        };
        assert!(
            line(Some("Valid")).contains("this file's signer is not on the configured trust lists")
        );
        let invalid = line(Some("Invalid"));
        assert!(invalid.contains("does not validate"));
        assert!(invalid.contains("an expired certificate is not tampering"));
        assert!(
            line(None).contains(
                "Not checked: whether the recorded steps happened as stated. The c2pa line"
            )
        );
    }

    #[test]
    fn a_stapled_revocation_response_is_named() {
        let mut store = flow_e2();
        store["validation_results"] = json!({"activeManifest": {"success": [
            {"code": "signingCredential.ocsp.notRevoked"}
        ]}});
        let lin = walk(&store);
        assert!(lin.revocation_checked);
        let t = render_timeline(&serde_json::to_value(&lin).unwrap());
        assert!(t.contains(
            "A revocation response stapled in the file says the signing certificate was not revoked."
        ));
        assert!(!t.contains("whether any signing certificate was revoked"));
        assert!(t.contains("happened as stated. A revocation response stapled"));
        // skipped (offline) is not a check
        let mut skipped = flow_e2();
        skipped["validation_results"] = json!({"activeManifest": {"informational": [
            {"code": "signingCredential.ocsp.skipped"}
        ]}});
        assert!(!walk(&skipped).revocation_checked);
    }

    #[test]
    fn reencodes_show_the_declared_term_without_the_capture_label() {
        let store = flow_e2();
        let lin = walk(&store);
        let t = render_timeline(&serde_json::to_value(&lin).unwrap());
        assert!(t.contains(
            "re-encoded (c2pa.transcoded) · declared non-generative (algorithmicallyEnhanced)"
        ));
        assert!(!t.contains("algorithmically enhanced capture"));
        // a real capture keeps its label
        assert!(t.contains("multi-frame computational capture (computationalCapture)"));
        let d = describe(&active_summary(&store), &lin, false);
        assert!(d.contains("declares algorithmicallyEnhanced, computationalCapture (multi-frame computational capture)"));
    }

    #[test]
    fn summary_and_bottom_line_for_the_flow_edits() {
        let e2 = walk(&flow_e2());
        assert_eq!(
            e2.summary.as_deref(),
            Some(
                "(camera capture → re-encode) was the input to: generative AI creation \
                 (watermark declared) → re-encode = this file"
            )
        );
        let b = e2.bottom_line.clone().unwrap();
        assert!(b.contains(
            "By the signer's own term, created by generative AI (trainedAlgorithmicMedia), this \
             is an AI generation that used the photo as input, not an AI-edited photo"
        ));
        assert!(b.contains(
            "No record says how much of the camera capture remains in the image; only comparing \
             the two can tell."
        ));
        assert!(b.contains("a camera capture was used as input to generative AI"));
        assert!(b.contains("an AI-derived image, not an unedited camera capture"));
        let e1 = walk(&flow_e1());
        assert_eq!(
            e1.summary.as_deref(),
            Some(
                "(camera capture → re-encode) was the input to: generative AI creation \
                 (watermark declared) = this file"
            )
        );
        assert_eq!(e1.bottom_line, walk(&flow_e2()).bottom_line);
    }

    #[test]
    fn summary_and_bottom_line_for_single_records() {
        let cam = walk(&json!({"active_manifest": "cam", "manifests": {"cam": camera()}}));
        assert_eq!(cam.summary.as_deref(), Some("camera capture = this file"));
        assert_eq!(
            cam.bottom_line.as_deref(),
            Some("As the signers state it: a camera capture, with no generative AI step recorded.")
        );
        let canva = walk(&json!({"active_manifest": "c", "manifests": {"c": manifest(
            vec![action("c2pa.created", Some("compositeWithTrainedAlgorithmicMedia"))],
            json!([]),
        )}}));
        assert_eq!(
            canva.summary.as_deref(),
            Some("composite including generative AI = this file")
        );
        assert!(
            canva
                .bottom_line
                .unwrap()
                .contains("no camera or scan capture is recorded")
        );
        let bare =
            walk(&json!({"active_manifest": "x", "manifests": {"x": manifest(vec![], json!([]))}}));
        assert_eq!(bare.bottom_line, None);
    }

    #[test]
    fn capture_on_the_lineage_edited_with_ai() {
        // A camera photo opened in an editor and changed with generative fill: the
        // capture is the parent, not an input.
        let store = json!({
            "active_manifest": "edit",
            "manifests": {
                "edit": manifest(
                    vec![action("c2pa.opened", None), action("c2pa.edited", Some("compositeWithTrainedAlgorithmicMedia"))],
                    json!([{"relationship": "parentOf", "active_manifest": "cam"}]),
                ),
                "cam": camera(),
            }
        });
        let lin = walk(&store);
        let b = lin.bottom_line.unwrap();
        assert!(b.contains("a camera capture was edited with generative AI"));
        assert!(b.contains("this is an AI-edited image, not an AI generation"));
        assert!(b.contains("No record says how much of the camera capture remains"));
        assert_eq!(
            lin.summary.as_deref(),
            Some("camera capture → composite including generative AI = this file")
        );
    }

    #[test]
    fn timeline_flags_unverified_ingredients_and_is_empty_without_a_chain() {
        let mut lin = walk(&flow_e1());
        lin.ingredient_failures = true;
        let t = render_timeline(&serde_json::to_value(&lin).unwrap());
        assert!(t.contains("not as verified"));
        assert_eq!(render_timeline(&json!({})), "");
        assert_eq!(render_timeline(&json!({"steps": []})), "");
    }

    #[test]
    fn software_agent_string_or_object() {
        assert_eq!(
            name_version(&json!("Photoshop")).as_deref(),
            Some("Photoshop")
        );
        assert_eq!(
            name_version(&json!({"name": "Lightroom", "version": "9.1"})).as_deref(),
            Some("Lightroom 9.1")
        );
        assert_eq!(name_version(&json!({"version": "1"})), None);
    }

    #[test]
    fn no_active_manifest_is_empty() {
        assert_eq!(walk(&json!({})), Lineage::default());
    }

    #[test]
    fn ingredient_failures_are_detected_and_named() {
        let ok = json!({"validation_results": {"ingredientDeltas": [
            {"validationDeltas": {"success": [{"code": "claimSignature.validated"}], "failure": []}}
        ]}});
        let bad = json!({"validation_results": {"ingredientDeltas": [
            {"validationDeltas": {"failure": [{"code": "assertion.dataHash.mismatch"}]}}
        ]}});
        assert!(!ingredient_failures(&ok));
        assert!(ingredient_failures(&bad));
        assert!(!ingredient_failures(&json!({})));
        let store = flow_e2();
        let d = describe(&active_summary(&store), &walk(&store), true);
        assert!(d.contains("not as verified"));
    }
}

/// The D-013 acceptance test on the real files. Needs the private 04 stratum on
/// disk, like the corpus and differential tests:
/// `cargo test -p halftone-c2pa --features c2pa -- --ignored lineage`
#[cfg(all(test, feature = "c2pa"))]
mod corpus_tests {
    use super::*;

    fn store(name: &str) -> Option<Value> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/differential/04-generators")
            .join(name);
        let file = std::fs::File::open(&path).ok()?;
        let reader = c2pa::Reader::from_context(c2pa::Context::new())
            .with_stream("image/jpeg", file)
            .expect("c2pa reads the file");
        Some(serde_json::from_str(&reader.json()).expect("reader json"))
    }

    #[test]
    #[ignore = "needs corpus/differential/04-generators on disk"]
    fn flow_edits_are_generative_whichever_download() {
        let files = [
            ("google-flow__nano-banana-2.1__edit-photo__e1__1.jpg", 0),
            ("google-flow__nano-banana-2.1__edit-photo__e2__2.jpg", 1),
            (
                "google-flow__nano-banana-2.1__edit-photo-redownload__e2__2.jpg",
                0,
            ),
        ];
        for (name, depth) in files {
            let Some(s) = store(name) else {
                eprintln!("skipped, not on disk: {name}");
                continue;
            };
            let lin = walk(&s);
            let g = lin
                .nearest_generative()
                .unwrap_or_else(|| panic!("{name}: no generative step in the lineage"));
            assert_eq!(g.depth, depth, "{name}");
            assert!(
                lin.inputs.iter().any(|i| i.relationship == "inputTo"),
                "{name}: the uploaded photo should be an inputTo ingredient"
            );
        }
    }
}
