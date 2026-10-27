//! Multi-asset hash fallback, C2PA 2.4 §15.12.4 (DIFFERENTIAL.md D-014).
//!
//! Some files are several files in one: an Ultra HDR JPEG carries a gain map after
//! the primary image, an Android motion photo also an MP4 clip. A camera that signs
//! such a file may add a `c2pa.hash.multi-asset` assertion next to the whole-file
//! hard binding: a table of the parts (byte offset, length, optional or not), each
//! with its own `c2pa.hash.data.part…` hash. §15.12.4 says that when the hard
//! binding fails and that assertion is present, the validator shall validate the
//! multi-asset hash instead. c2pa 0.91 (and so c2patool) never does; Credentio does.
//!
//! This module is the procedure only. It reads the assertions from c2pa's detailed
//! JSON (`Reader::detailed_json()`, `manifests.<label>.assertion_store`), and hashes
//! each part through a [`PartHasher`], so the c2pa-backed hasher in `lib.rs` uses
//! c2pa's own data-hash code and the tests here use a plain SHA-256 one. Nothing in
//! here depends on the c2pa crate.
//!
//! Two gates keep the fallback from trusting anything c2pa did not verify
//! ([`eligible`]): the claim signature validated, the only failures are the
//! whole-file `c2pa.hash.data` mismatch (and an untrusted signer, which is a trust
//! question, not an integrity one), and the multi-asset assertion and every part
//! hash assertion it names carry `assertion.hashedURI.match`, so their contents are
//! the ones the signer signed.
//!
//! The procedure follows §15.12.4 in order: structure checks
//! (`assertion.multiAssetHash.malformed`), locating each part (a missing required
//! part is `missingPart`; a missing optional part is skipped), coverage (the located
//! parts must tile the file, else `malformed`), then each located part's hash (a
//! part shorter than its locator, or one whose hash differs, is `mismatch`). Part
//! hashes are still computed after a `malformed` or `missingPart` result, for the
//! report only: knowing which part changed is the forensic point, and the outcome
//! does not change.
//!
//! Not supported, reported as [`Outcome::NotEvaluated`] rather than guessed: `bmffBox`
//! locators, part hashes other than data hashes (`c2pa.hash.boxes.part`,
//! `c2pa.hash.bmff…part`), and remote data hashes.

use serde::Serialize;
use serde_json::Value;

/// The rule this module implements, as cited in rationales and `details.multi_asset`.
pub const RULE: &str = "C2PA 2.4 §15.12.4";

/// Label of the multi-asset hash assertion.
pub const LABEL: &str = "c2pa.hash.multi-asset";

/// Label of the whole-file data hash, the hard binding the fallback replaces.
pub const HARD_BINDING: &str = "c2pa.hash.data";

/// Where a part sits in the file, as its locator states it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Locator {
    /// `byteOffset` and `length`, from the physical start of the file.
    Bytes {
        /// Offset of the part's first byte.
        offset: u64,
        /// Length of the part in bytes.
        length: u64,
    },
    /// A BMFF box path (e.g. `mpvd` in a HEIC motion photo). Not evaluated here.
    BmffBox(String),
}

/// What happened to one part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PartResult {
    /// Located, full length, and its hash matches.
    Match,
    /// Located and full length, but its hash differs: the part changed after signing.
    Mismatch,
    /// Located but shorter than its locator's length (the file ends inside it).
    /// §15.12.4 treats this as `assertion.multiAssetHash.mismatch`.
    Truncated,
    /// Not in the file and marked optional: skipped, as §15.12.4 allows.
    Absent,
    /// Not in the file and not optional: `assertion.multiAssetHash.missingPart`.
    Missing,
    /// Located, but its hash could not be checked here (see the reason).
    NotChecked(String),
}

/// One row of the part table, with its result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PartReport {
    /// 0-based position in the signed `parts` array.
    pub index: usize,
    /// Label of the part's hash assertion, e.g. `c2pa.hash.data.part__2`.
    pub hash_assertion: String,
    /// The signed locator.
    pub locator: Locator,
    /// Whether the signer marked the part optional (may be discarded).
    pub optional: bool,
    /// What the part's first bytes look like (`jpeg`, `mp4`, …), when it is located.
    /// A hint for rationales, read from the bytes, never from the manifest.
    pub kind: Option<&'static str>,
    /// How many of the part's bytes are in the file, when it is located: its full
    /// length, or fewer when the file ends inside it (a part cut short). `None`
    /// when the part is not in the file at all.
    pub present_bytes: Option<u64>,
    /// What the file itself declares at exactly this part's signed place (offset
    /// and length), e.g. `MotionPhoto` from its container directory: set by
    /// [`name_from_declared`], `None` when the file declares nothing there. A name
    /// for rationales, unsigned; it never changes a result.
    pub declared_as: Option<String>,
    /// The result for this part.
    pub result: PartResult,
}

/// Overall result of the procedure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Every located part matches and nothing required is missing:
    /// `assertion.multiAssetHash.match`. The manifest's content binding holds.
    Match,
    /// A located part changed or was cut short: `assertion.multiAssetHash.mismatch`.
    Mismatch,
    /// A required part is missing: `assertion.multiAssetHash.missingPart`.
    MissingPart,
    /// The table or the located parts break the rules: `assertion.multiAssetHash.malformed`.
    Malformed,
    /// The fallback did not run or could not finish; nothing is concluded from it.
    NotEvaluated,
}

impl Outcome {
    /// The C2PA status code for this outcome, `None` when nothing was evaluated.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Outcome::Match => Some("assertion.multiAssetHash.match"),
            Outcome::Mismatch => Some("assertion.multiAssetHash.mismatch"),
            Outcome::MissingPart => Some("assertion.multiAssetHash.missingPart"),
            Outcome::Malformed => Some("assertion.multiAssetHash.malformed"),
            Outcome::NotEvaluated => None,
        }
    }
}

/// The full result: outcome, why, and the per-part table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// The rule applied ([`RULE`]).
    pub rule: &'static str,
    /// Overall outcome.
    pub outcome: Outcome,
    /// The outcome's status code, when there is one.
    pub code: Option<&'static str>,
    /// Plain-language reason for `Malformed`, `MissingPart` and `NotEvaluated`.
    pub reason: Option<String>,
    /// One entry per signed part, in signed order. Empty when the table was unusable.
    pub parts: Vec<PartReport>,
    /// Whether the file declares a part at exactly every signed part's place: the
    /// signed table and the file's own (unsigned) description of itself agree.
    /// `None` when the file declares no parts or no signed parts were read.
    pub declared_parts_agree: Option<bool>,
}

impl Report {
    fn new(outcome: Outcome, reason: Option<String>, parts: Vec<PartReport>) -> Self {
        let code = outcome.code();
        Report {
            rule: RULE,
            outcome,
            code,
            reason,
            parts,
            declared_parts_agree: None,
        }
    }

    /// A report that concludes nothing, with the reason.
    pub fn not_evaluated(reason: impl Into<String>) -> Self {
        Report::new(Outcome::NotEvaluated, Some(reason.into()), Vec::new())
    }
}

/// A data-hash assertion for one part, as signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartHash {
    /// Hash algorithm; `None` means the claim's default (sha256 in practice).
    pub alg: Option<String>,
    /// The signed hash value.
    pub hash: Vec<u8>,
    /// Exclusion ranges `(start, length)`, relative to the part's first byte (§18.9).
    pub exclusions: Vec<(u64, u64)>,
}

/// Checks one part's bytes against its signed data hash.
pub trait PartHasher {
    /// `Ok(true)` when the hash of `bytes` (minus the exclusions) equals the signed
    /// value, `Ok(false)` when it differs, `Err` when it cannot be computed (unknown
    /// algorithm, exclusion out of range).
    fn verify(&self, part: &PartHash, bytes: &[u8]) -> Result<bool, String>;
}

/// One validation-status entry: code and the URL of the assertion it is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The status code, e.g. `assertion.dataHash.mismatch`.
    pub code: String,
    /// The JUMBF URL it refers to, when it has one.
    pub url: Option<String>,
}

impl Status {
    /// Convenience constructor.
    pub fn new(code: &str, url: Option<&str>) -> Self {
        Status {
            code: code.to_string(),
            url: url.map(str::to_string),
        }
    }

    fn is_about(&self, label: &str) -> bool {
        self.url
            .as_deref()
            .is_some_and(|u| u.ends_with(&format!("c2pa.assertions/{label}")))
    }
}

/// Whether c2pa reported the whole-file hard binding (`c2pa.hash.data`) as failed.
/// Without that there is nothing to fall back from, and the fallback stays silent.
pub fn hard_binding_failed(failures: &[Status]) -> bool {
    failures
        .iter()
        .any(|f| f.code == "assertion.dataHash.mismatch" && f.is_about(HARD_BINDING))
}

/// Whether the fallback may run on this manifest, given c2pa's validation results
/// for the active manifest. `Ok(())` to proceed; `Err(reason)` explains why not.
///
/// Requires: the claim signature validated; the whole-file hard binding failed;
/// no failure other than that mismatch or `signingCredential.untrusted`; and a
/// `hashedURI.match` for the multi-asset assertion and for every label in
/// `part_labels`, so their contents are bound to the signed claim.
pub fn eligible(
    failures: &[Status],
    successes: &[Status],
    part_labels: &[String],
) -> Result<(), String> {
    if !successes
        .iter()
        .any(|s| s.code == "claimSignature.validated")
    {
        return Err("the claim signature did not validate".into());
    }
    if !hard_binding_failed(failures) {
        return Err(
            "the whole-file hard binding did not fail, so there is nothing to fall back from"
                .into(),
        );
    }
    let other: Vec<&str> = failures
        .iter()
        .filter(|f| {
            !(f.code == "signingCredential.untrusted"
                || (f.code == "assertion.dataHash.mismatch" && f.is_about(HARD_BINDING)))
        })
        .map(|f| f.code.as_str())
        .collect();
    if !other.is_empty() {
        let mut other = other;
        other.sort_unstable();
        other.dedup();
        return Err(format!(
            "the manifest also fails {}, which the fallback does not replace",
            other.join(", ")
        ));
    }
    let bound = |label: &str| {
        successes
            .iter()
            .any(|s| s.code == "assertion.hashedURI.match" && s.is_about(label))
    };
    if !bound(LABEL) {
        return Err(format!(
            "{LABEL} is not confirmed as part of the signed claim"
        ));
    }
    for l in part_labels {
        if !bound(l) {
            return Err(format!("{l} is not confirmed as part of the signed claim"));
        }
    }
    Ok(())
}

/// The part hash-assertion labels a multi-asset assertion names, for [`eligible`].
/// Empty when the table cannot be read; [`evaluate`] then reports it as malformed.
pub fn part_labels(multi_asset: &Value) -> Vec<String> {
    multi_asset
        .get("parts")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.pointer("/hashAssertion/url").and_then(Value::as_str))
                .map(label_of)
                .collect()
        })
        .unwrap_or_default()
}

/// The assertion label at the end of a JUMBF URL (`…/c2pa.assertions/<label>`).
fn label_of(url: &str) -> String {
    url.rsplit_once("c2pa.assertions/")
        .map_or(url, |(_, l)| l)
        .to_string()
}

/// Whether the assertion store holds more than one multi-asset assertion
/// (`c2pa.hash.multi-asset__1`, …), which §15.12.4 calls malformed.
fn multiple_multi_asset(store: &Value) -> bool {
    store
        .as_object()
        .is_some_and(|m| m.keys().any(|k| k.starts_with(&format!("{LABEL}__"))))
}

/// A first-bytes hint for rationales. Read from the file, not from the manifest.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpeg")
    } else if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
        Some("mp4")
    } else {
        None
    }
}

/// Decode standard base64 (with or without padding). c2pa's detailed JSON writes
/// byte strings this way; an array of byte values is accepted too ([`bytes_of`]).
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let clean: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    let body: &[u8] = match clean.iter().position(|&c| c == b'=') {
        Some(i) if clean[i..].iter().all(|&c| c == b'=') && clean[i..].len() <= 2 => &clean[..i],
        Some(_) => return None,
        None => &clean,
    };
    if body.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    for chunk in body.chunks(4) {
        let mut acc: u32 = 0;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= val(c)? << (18 - 6 * i);
        }
        let n = chunk.len() - 1; // 2 chars -> 1 byte, 3 -> 2, 4 -> 3
        for i in 0..n {
            out.push((acc >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

/// A byte string from c2pa's JSON: base64 text, or an array of byte values.
fn bytes_of(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::String(s) => base64_decode(s),
        Value::Array(a) => a
            .iter()
            .map(|x| x.as_u64().and_then(|n| u8::try_from(n).ok()))
            .collect(),
        _ => None,
    }
}

/// Read one part's data-hash assertion from the store. `Err` says why it cannot be
/// checked here.
fn part_hash(store: &Value, label: &str) -> Result<PartHash, String> {
    if !label.starts_with("c2pa.hash.data.part") {
        return Err(format!(
            "{label} is not a data hash; only data-hash parts are checked"
        ));
    }
    let a = store
        .get(label)
        .ok_or_else(|| format!("{label} is not in the manifest's assertion store"))?;
    if a.get("url").is_some_and(|u| !u.is_null()) {
        return Err(format!("{label} is a remote data hash"));
    }
    let hash = a
        .get("hash")
        .and_then(bytes_of)
        .filter(|h| !h.is_empty())
        .ok_or_else(|| format!("{label} has no readable hash value"))?;
    let alg = a.get("alg").and_then(Value::as_str).map(str::to_string);
    let mut exclusions = Vec::new();
    if let Some(ex) = a.get("exclusions").and_then(Value::as_array) {
        for e in ex {
            let start = e.get("start").and_then(Value::as_u64);
            let length = e.get("length").and_then(Value::as_u64);
            match (start, length) {
                (Some(s), Some(l)) => exclusions.push((s, l)),
                _ => return Err(format!("{label} has an unreadable exclusion range")),
            }
        }
    }
    Ok(PartHash {
        alg,
        hash,
        exclusions,
    })
}

/// Name each signed part after the part the file declares at exactly the same
/// place, and record whether the two agree. The match is on declarations (signed
/// offset and length against declared offset and length), so a part whose bytes
/// are gone is still named. Naming only: declarations are unsigned and never
/// change an outcome.
pub fn name_from_declared(report: &mut Report, declared: &[halftone_formats::parts::DeclaredPart]) {
    if report.parts.is_empty() || declared.is_empty() {
        return;
    }
    let mut all = true;
    for p in &mut report.parts {
        let Locator::Bytes { offset, length } = p.locator else {
            all = false;
            continue;
        };
        p.declared_as = declared
            .iter()
            .find(|d| d.offset == offset && d.length == length)
            .map(|d| d.name.clone());
        all &= p.declared_as.is_some();
    }
    report.declared_parts_agree = Some(all);
}

/// Run §15.12.4 on `file` (the whole asset's bytes) with the multi-asset assertion
/// `multi_asset` and the manifest's `store` (`assertion_store` from c2pa's detailed
/// JSON, where the part hash assertions live).
///
/// Call it only after [`eligible`] passed: this function checks structure, location,
/// coverage and hashes, not whether the assertions were signed.
pub fn evaluate(
    multi_asset: &Value,
    store: &Value,
    file: &[u8],
    hasher: &dyn PartHasher,
) -> Report {
    let len = file.len() as u64;
    if multiple_multi_asset(store) {
        return Report::new(
            Outcome::Malformed,
            Some("the manifest carries more than one multi-asset hash assertion".into()),
            Vec::new(),
        );
    }
    let Some(raw) = multi_asset
        .get("parts")
        .and_then(Value::as_array)
        .filter(|p| !p.is_empty())
    else {
        return Report::new(
            Outcome::Malformed,
            Some("the parts list is missing or empty".into()),
            Vec::new(),
        );
    };

    // ---- structure: every part has a valid locator and hash assertion ------------
    let mut parts: Vec<PartReport> = Vec::with_capacity(raw.len());
    for (index, p) in raw.iter().enumerate() {
        let Some(url) = p.pointer("/hashAssertion/url").and_then(Value::as_str) else {
            return Report::new(
                Outcome::Malformed,
                Some(format!("part {index} has no hashAssertion")),
                Vec::new(),
            );
        };
        let loc = p.get("location");
        let locator = match (
            loc.and_then(|l| l.get("byteOffset")),
            loc.and_then(|l| l.get("length")),
            loc.and_then(|l| l.get("bmffBox")).and_then(Value::as_str),
        ) {
            (Some(o), Some(l), None) => match (o.as_u64(), l.as_u64()) {
                (Some(offset), Some(length)) => Locator::Bytes { offset, length },
                _ => {
                    return Report::new(
                        Outcome::Malformed,
                        Some(format!(
                            "part {index}: byteOffset and length must be non-negative integers"
                        )),
                        Vec::new(),
                    );
                }
            },
            (None, None, Some(b)) => Locator::BmffBox(b.to_string()),
            _ => {
                return Report::new(
                    Outcome::Malformed,
                    Some(format!("part {index} has no valid locator")),
                    Vec::new(),
                );
            }
        };
        if let Locator::BmffBox(b) = &locator {
            return Report::not_evaluated(format!(
                "part {index} is located by BMFF box ({b}), which Halftone does not evaluate"
            ));
        }
        parts.push(PartReport {
            index,
            hash_assertion: label_of(url),
            locator,
            optional: p.get("optional").and_then(Value::as_bool).unwrap_or(false),
            kind: None,
            present_bytes: None,
            declared_as: None,
            result: PartResult::Absent, // set below
        });
    }

    // ---- locate: a part is in the file when its first byte is ---------------------
    let mut missing_required: Vec<usize> = Vec::new();
    let mut located: Vec<(u64, u64, usize)> = Vec::new(); // (offset, declared length, index into parts)
    for (i, part) in parts.iter_mut().enumerate() {
        // Unreachable: a BMFF locator returned NotEvaluated above. Kept so a
        // refactor that lets one through skips it instead of mislocating it.
        let Locator::Bytes { offset, length } = part.locator else {
            continue;
        };
        if offset < len {
            let start = usize::try_from(offset).unwrap_or(usize::MAX);
            part.kind = file.get(start..).and_then(sniff);
            part.present_bytes = Some(length.min(len - offset));
            located.push((offset, length, i));
        } else if part.optional {
            part.result = PartResult::Absent;
        } else {
            part.result = PartResult::Missing;
            missing_required.push(part.index);
        }
    }

    // ---- coverage: located parts must tile [0, len) without gaps or overlaps ------
    // A last part that runs past the end is not a coverage failure: it is a part
    // shorter than its locator, which the hash step reports as a mismatch.
    let mut coverage: Option<String> = None;
    let mut expect: u64 = 0;
    for (k, &(offset, length, i)) in located.iter().enumerate() {
        if offset != expect {
            coverage = Some(format!(
                "part {} starts at byte {offset}, but the parts before it end at byte {expect}",
                parts[i].index
            ));
            break;
        }
        expect = offset.saturating_add(length);
        let last = k + 1 == located.len();
        if !last && expect > len {
            coverage = Some(format!(
                "part {} runs past the end of the file but is not the last part",
                parts[i].index
            ));
            break;
        }
    }
    if coverage.is_none() && expect < len {
        coverage = Some(format!(
            "the located parts end at byte {expect}, but the file has {len} bytes"
        ));
    }
    if located.is_empty() {
        coverage = Some("no part was found in the file".into());
    }

    // ---- hashes: every located part, for the report even after malformed ----------
    let mut changed = false;
    let mut unchecked: Option<String> = None;
    for &(offset, length, i) in &located {
        let end = offset.saturating_add(length);
        let result = if end > len {
            changed = true;
            PartResult::Truncated
        } else {
            match part_hash(store, &parts[i].hash_assertion) {
                Err(why) => {
                    unchecked.get_or_insert_with(|| why.clone());
                    PartResult::NotChecked(why)
                }
                Ok(ph) => {
                    let (s, e) = (
                        usize::try_from(offset).unwrap_or(usize::MAX),
                        usize::try_from(end).unwrap_or(usize::MAX),
                    );
                    match file.get(s..e).map(|b| hasher.verify(&ph, b)) {
                        Some(Ok(true)) => PartResult::Match,
                        Some(Ok(false)) => {
                            changed = true;
                            PartResult::Mismatch
                        }
                        Some(Err(why)) => {
                            unchecked.get_or_insert_with(|| why.clone());
                            PartResult::NotChecked(why)
                        }
                        // Unreachable: offset < len and end <= len were checked above.
                        // Kept as an error rather than a slice index that could panic.
                        None => {
                            let why = "part range outside the file".to_string();
                            unchecked.get_or_insert_with(|| why.clone());
                            PartResult::NotChecked(why)
                        }
                    }
                }
            }
        };
        parts[i].result = result;
    }

    // ---- outcome, in the order §15.12.4 checks things ------------------------------
    if !missing_required.is_empty() {
        let list: Vec<String> = missing_required.iter().map(|i| i.to_string()).collect();
        return Report::new(
            Outcome::MissingPart,
            Some(format!(
                "required part {} is not in the file",
                list.join(", ")
            )),
            parts,
        );
    }
    if let Some(why) = coverage {
        return Report::new(Outcome::Malformed, Some(why), parts);
    }
    if changed {
        return Report::new(Outcome::Mismatch, None, parts);
    }
    if let Some(why) = unchecked {
        return Report::new(Outcome::NotEvaluated, Some(why), parts);
    }
    Report::new(Outcome::Match, None, parts)
}

/// What must hold for any report [`evaluate`] returns on a file of `file_len`
/// bytes. `Err` names the first violation. Used by the unit tests and the fuzz
/// targets (`crates/halftone-c2pa/fuzz`), so the rules are written once.
pub fn check_invariants(report: &Report, file_len: u64) -> Result<(), String> {
    if report.code != report.outcome.code() {
        return Err(format!(
            "code {:?} does not match outcome {:?}",
            report.code, report.outcome
        ));
    }
    for p in &report.parts {
        let Locator::Bytes { offset, length } = p.locator else {
            return Err(format!(
                "part {} has a BMFF locator in an evaluated report",
                p.index
            ));
        };
        let located = offset < file_len;
        let want_present = located.then(|| length.min(file_len - offset));
        if p.present_bytes != want_present {
            return Err(format!(
                "part {}: present_bytes {:?}, expected {want_present:?}",
                p.index, p.present_bytes
            ));
        }
        let ok = match &p.result {
            PartResult::Absent => !located && p.optional,
            PartResult::Missing => !located && !p.optional,
            PartResult::Truncated => located && offset.saturating_add(length) > file_len,
            PartResult::Match | PartResult::Mismatch | PartResult::NotChecked(_) => {
                located && offset.saturating_add(length) <= file_len
            }
        };
        if !ok {
            return Err(format!(
                "part {} ({offset}+{length}, optional {}) cannot be {:?} in a file of {file_len} bytes",
                p.index, p.optional, p.result
            ));
        }
    }
    let any = |f: fn(&PartResult) -> bool| report.parts.iter().any(|p| f(&p.result));
    match report.outcome {
        Outcome::Match => {
            if any(|r| !matches!(r, PartResult::Match | PartResult::Absent)) {
                return Err("outcome match with a part that is not match or absent".into());
            }
            if !any(|r| *r == PartResult::Match) {
                return Err("outcome match with no matching part".into());
            }
        }
        Outcome::MissingPart if !any(|r| *r == PartResult::Missing) => {
            return Err("outcome missingPart with no missing part".into());
        }
        Outcome::Mismatch
            if !any(|r| matches!(r, PartResult::Mismatch | PartResult::Truncated)) =>
        {
            return Err("outcome mismatch with no changed or truncated part".into());
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sha2::{Digest, Sha256};

    /// SHA-256 over the part minus its exclusions: the data-hash rule, without c2pa.
    struct Plain;
    impl PartHasher for Plain {
        fn verify(&self, part: &PartHash, bytes: &[u8]) -> Result<bool, String> {
            Ok(digest(bytes, &part.exclusions) == part.hash)
        }
    }

    fn digest(bytes: &[u8], exclusions: &[(u64, u64)]) -> Vec<u8> {
        let mut h = Sha256::new();
        let mut pos = 0usize;
        let mut ex: Vec<(usize, usize)> = exclusions
            .iter()
            .map(|&(s, l)| (s as usize, (s + l) as usize))
            .collect();
        ex.sort_unstable();
        for (s, e) in ex {
            h.update(&bytes[pos..s]);
            pos = e;
        }
        h.update(&bytes[pos..]);
        h.finalize().to_vec()
    }

    fn b64(bytes: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in bytes.chunks(3) {
            let n = (u32::from(c[0]) << 16)
                | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
                | u32::from(*c.get(2).unwrap_or(&0));
            for i in 0..4 {
                if i <= c.len() {
                    s.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
                } else {
                    s.push('=');
                }
            }
        }
        s
    }

    /// A three-part file shaped like the Fold motion photo: JPEG, JPEG gain map, MP4.
    /// Part 0 has a 4-byte exclusion standing in for the manifest segment.
    struct Fixture {
        file: Vec<u8>,
        multi_asset: Value,
        store: Value,
    }

    fn fixture() -> Fixture {
        let mut p0 = vec![0xFF, 0xD8, 0xFF, 0xE1];
        p0.extend_from_slice(b"MANI"); // excluded, like the C2PA segment
        p0.extend(std::iter::repeat_n(0x7Bu8, 40));
        p0.extend_from_slice(&[0xFF, 0xD9]);
        let mut p1 = vec![0xFF, 0xD8, 0xFF, 0xE0];
        p1.extend(std::iter::repeat_n(0x22u8, 10));
        p1.extend_from_slice(&[0xFF, 0xD9]);
        let mut p2 = vec![0, 0, 0, 0x18];
        p2.extend_from_slice(b"ftyp");
        p2.extend(std::iter::repeat_n(0x22u8, 60));
        let (o1, o2) = (p0.len() as u64, (p0.len() + p1.len()) as u64);
        let excl = vec![(4u64, 4u64)];
        let store = json!({
            "c2pa.hash.data": {"alg": "sha256", "hash": b64(&[1, 2, 3])},
            "c2pa.hash.data.part": {"alg": "sha256", "hash": b64(&digest(&p0, &excl)),
                                    "exclusions": [{"start": 4, "length": 4}]},
            "c2pa.hash.data.part__1": {"alg": "sha256", "hash": b64(&digest(&p1, &[]))},
            // array form, as some c2pa JSON writes byte strings
            "c2pa.hash.data.part__2": {"alg": "sha256", "hash": digest(&p2, &[])},
        });
        let part = |o: u64, l: usize, label: &str, optional: bool| {
            json!({"location": {"byteOffset": o, "length": l},
                   "hashAssertion": {"url": format!("self#jumbf=c2pa.assertions/{label}"), "hash": "x"},
                   "optional": optional})
        };
        let multi_asset = json!({"parts": [
            part(0, p0.len(), "c2pa.hash.data.part", false),
            part(o1, p1.len(), "c2pa.hash.data.part__1", true),
            part(o2, p2.len(), "c2pa.hash.data.part__2", true),
        ]});
        let mut file = p0;
        file.extend(p1);
        file.extend(p2);
        Fixture {
            file,
            multi_asset,
            store,
        }
    }

    fn results(r: &Report) -> Vec<PartResult> {
        r.parts.iter().map(|p| p.result.clone()).collect()
    }

    fn offset(f: &Fixture, i: usize) -> usize {
        f.multi_asset["parts"][i]["location"]["byteOffset"]
            .as_u64()
            .unwrap() as usize
    }

    #[test]
    fn untouched_file_matches_part_by_part() {
        let f = fixture();
        let r = evaluate(&f.multi_asset, &f.store, &f.file, &Plain);
        assert_eq!(r.outcome, Outcome::Match);
        assert_eq!(r.code, Some("assertion.multiAssetHash.match"));
        assert_eq!(results(&r), vec![PartResult::Match; 3]);
        let kinds: Vec<_> = r.parts.iter().map(|p| p.kind).collect();
        assert_eq!(kinds, vec![Some("jpeg"), Some("jpeg"), Some("mp4")]);
    }

    #[test]
    fn clip_removed_still_matches() {
        let f = fixture();
        let r = evaluate(&f.multi_asset, &f.store, &f.file[..offset(&f, 2)], &Plain);
        assert_eq!(r.outcome, Outcome::Match);
        assert_eq!(
            results(&r),
            vec![PartResult::Match, PartResult::Match, PartResult::Absent]
        );
    }

    #[test]
    fn gain_map_and_clip_removed_still_matches() {
        let f = fixture();
        let r = evaluate(&f.multi_asset, &f.store, &f.file[..offset(&f, 1)], &Plain);
        assert_eq!(r.outcome, Outcome::Match);
        assert_eq!(
            results(&r),
            vec![PartResult::Match, PartResult::Absent, PartResult::Absent]
        );
    }

    #[test]
    fn one_bit_in_the_clip_is_a_mismatch_in_the_clip_only() {
        let mut f = fixture();
        let at = offset(&f, 2) + 20;
        f.file[at] ^= 1;
        let r = evaluate(&f.multi_asset, &f.store, &f.file, &Plain);
        assert_eq!(r.outcome, Outcome::Mismatch);
        assert_eq!(
            results(&r),
            vec![PartResult::Match, PartResult::Match, PartResult::Mismatch]
        );
    }

    #[test]
    fn one_bit_in_the_still_is_a_mismatch_in_the_still_only() {
        let mut f = fixture();
        f.file[20] ^= 1;
        let r = evaluate(&f.multi_asset, &f.store, &f.file, &Plain);
        assert_eq!(r.outcome, Outcome::Mismatch);
        assert_eq!(
            results(&r),
            vec![PartResult::Mismatch, PartResult::Match, PartResult::Match]
        );
    }

    #[test]
    fn a_change_inside_an_exclusion_is_not_a_change() {
        let mut f = fixture();
        f.file[5] ^= 1; // inside the excluded "MANI" bytes
        let r = evaluate(&f.multi_asset, &f.store, &f.file, &Plain);
        assert_eq!(r.outcome, Outcome::Match);
    }

    #[test]
    fn clip_cut_short_is_a_mismatch_not_a_missing_part() {
        // §15.12.4: a part shorter than its locator is multiAssetHash.mismatch.
        // Credentio reports missingPart + malformed here (DIFFERENTIAL.md D-015).
        let f = fixture();
        let cut = offset(&f, 2) + 10;
        let r = evaluate(&f.multi_asset, &f.store, &f.file[..cut], &Plain);
        assert_eq!(r.outcome, Outcome::Mismatch);
        assert_eq!(
            results(&r),
            vec![PartResult::Match, PartResult::Match, PartResult::Truncated]
        );
        assert_eq!(r.parts[2].present_bytes, Some(10));
        assert_eq!(check_invariants(&r, cut as u64), Ok(()));
    }

    #[test]
    fn bytes_after_the_last_part_are_malformed_with_per_part_results_kept() {
        // Like an ExifTool rewrite that grows the file: the parts no longer tile it.
        let mut f = fixture();
        f.file.extend_from_slice(b"extra");
        let r = evaluate(&f.multi_asset, &f.store, &f.file, &Plain);
        assert_eq!(r.outcome, Outcome::Malformed);
        assert_eq!(results(&r), vec![PartResult::Match; 3]);
    }

    #[test]
    fn inserted_bytes_in_the_still_shift_everything_after_it() {
        // ExifTool-style metadata rewrite: part 0 grows, later parts sit at new offsets.
        let mut f = fixture();
        f.file.splice(10..10, [0xAAu8; 3]);
        let r = evaluate(&f.multi_asset, &f.store, &f.file, &Plain);
        assert_eq!(r.outcome, Outcome::Malformed);
        assert_eq!(r.parts[0].result, PartResult::Mismatch);
    }

    #[test]
    fn a_missing_required_part_is_missing_part() {
        let mut f = fixture();
        f.multi_asset["parts"][2]["optional"] = json!(false);
        let r = evaluate(&f.multi_asset, &f.store, &f.file[..offset(&f, 2)], &Plain);
        assert_eq!(r.outcome, Outcome::MissingPart);
        assert_eq!(r.parts[2].result, PartResult::Missing);
    }

    #[test]
    fn optional_defaults_to_false() {
        let mut f = fixture();
        f.multi_asset["parts"][2]
            .as_object_mut()
            .unwrap()
            .remove("optional");
        let r = evaluate(&f.multi_asset, &f.store, &f.file[..offset(&f, 2)], &Plain);
        assert_eq!(r.outcome, Outcome::MissingPart);
    }

    #[test]
    fn structure_errors_are_malformed() {
        let f = fixture();
        let empty = evaluate(&json!({"parts": []}), &f.store, &f.file, &Plain);
        assert_eq!(empty.outcome, Outcome::Malformed);
        let no_hash = json!({"parts": [{"location": {"byteOffset": 0, "length": 3}}]});
        assert_eq!(
            evaluate(&no_hash, &f.store, &f.file, &Plain).outcome,
            Outcome::Malformed
        );
        let negative = json!({"parts": [{"location": {"byteOffset": -1, "length": 3},
            "hashAssertion": {"url": "self#jumbf=c2pa.assertions/c2pa.hash.data.part"}}]});
        assert_eq!(
            evaluate(&negative, &f.store, &f.file, &Plain).outcome,
            Outcome::Malformed
        );
        let mut two = f.store.clone();
        two[format!("{LABEL}__1")] = json!({});
        assert_eq!(
            evaluate(&f.multi_asset, &two, &f.file, &Plain).outcome,
            Outcome::Malformed
        );
    }

    #[test]
    fn unsupported_parts_conclude_nothing() {
        let mut f = fixture();
        f.multi_asset["parts"][1]["hashAssertion"]["url"] =
            json!("self#jumbf=c2pa.assertions/c2pa.hash.boxes.part");
        let r = evaluate(&f.multi_asset, &f.store, &f.file, &Plain);
        assert_eq!(r.outcome, Outcome::NotEvaluated);
        assert!(matches!(r.parts[1].result, PartResult::NotChecked(_)));
        let bmff = json!({"parts": [{"location": {"bmffBox": "/mpvd"},
            "hashAssertion": {"url": "self#jumbf=c2pa.assertions/c2pa.hash.bmff.v3.part"}}]});
        assert_eq!(
            evaluate(&bmff, &f.store, &f.file, &Plain).outcome,
            Outcome::NotEvaluated
        );
    }

    fn ok_results() -> (Vec<Status>, Vec<Status>, Vec<String>) {
        let u = |l: &str| format!("self#jumbf=/c2pa/urn:c2pa:x/c2pa.assertions/{l}");
        let labels: Vec<String> = ["c2pa.hash.data.part", "c2pa.hash.data.part__1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let failures = vec![Status::new(
            "assertion.dataHash.mismatch",
            Some(&u(HARD_BINDING)),
        )];
        let mut successes = vec![
            Status::new("claimSignature.validated", None),
            Status::new("assertion.hashedURI.match", Some(&u(LABEL))),
        ];
        for l in &labels {
            successes.push(Status::new("assertion.hashedURI.match", Some(&u(l))));
        }
        (failures, successes, labels)
    }

    #[test]
    fn eligible_when_only_the_hard_binding_failed() {
        let (f, s, l) = ok_results();
        assert_eq!(eligible(&f, &s, &l), Ok(()));
        let mut untrusted = f.clone();
        untrusted.push(Status::new("signingCredential.untrusted", None));
        assert_eq!(eligible(&untrusted, &s, &l), Ok(()));
    }

    #[test]
    fn not_eligible_without_a_validated_signature_or_with_other_failures() {
        let (f, s, l) = ok_results();
        let unsigned: Vec<Status> = s
            .iter()
            .filter(|x| x.code != "claimSignature.validated")
            .cloned()
            .collect();
        assert!(eligible(&f, &unsigned, &l).is_err());
        let mut more = f.clone();
        more.push(Status::new("signingCredential.expired", None));
        assert!(
            eligible(&more, &s, &l)
                .unwrap_err()
                .contains("signingCredential.expired")
        );
        assert!(
            eligible(&[], &s, &l).is_err(),
            "nothing failed: nothing to fall back from"
        );
    }

    #[test]
    fn not_eligible_when_a_part_hash_is_not_bound_to_the_claim() {
        let (f, s, mut l) = ok_results();
        l.push("c2pa.hash.data.part__2".into());
        assert!(eligible(&f, &s, &l).unwrap_err().contains("part__2"));
        let no_ma: Vec<Status> = s.iter().filter(|x| !x.is_about(LABEL)).cloned().collect();
        assert!(eligible(&f, &no_ma, &l[..2]).unwrap_err().contains(LABEL));
    }

    #[test]
    fn part_labels_reads_the_table() {
        let f = fixture();
        assert_eq!(
            part_labels(&f.multi_asset),
            vec![
                "c2pa.hash.data.part",
                "c2pa.hash.data.part__1",
                "c2pa.hash.data.part__2"
            ]
        );
    }

    #[test]
    fn base64_round_trips() {
        for n in 0..40u8 {
            let data: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(base64_decode(&b64(&data)).as_deref(), Some(data.as_slice()));
        }
        assert_eq!(
            base64_decode("iNRcOAEnYppOracYYrqJv8+SQEAHMxXIpq42ZoTQPJ8=").map(|v| v.len()),
            Some(32)
        );
        assert!(base64_decode("a=b=").is_none());
        assert!(base64_decode("A").is_none());
    }

    /// Fixed-seed random tables, run on every `cargo test`: the same invariants the
    /// fuzz targets check, without needing nightly or cargo-fuzz.
    #[test]
    fn random_tables_keep_the_invariants() {
        let mut seed: u64 = 0x005E_ED15_124A_55E7;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        // The part hash says "match" ([1]) or "mismatch" ([0]); anything else is unreadable.
        struct Oracle;
        impl PartHasher for Oracle {
            fn verify(&self, part: &PartHash, _bytes: &[u8]) -> Result<bool, String> {
                match part.hash.first() {
                    Some(1) => Ok(true),
                    Some(0) => Ok(false),
                    _ => Err("unreadable".into()),
                }
            }
        }
        for _ in 0..20_000 {
            let n = 1 + (next() % 4) as usize;
            let mut parts = Vec::new();
            let mut store = serde_json::Map::new();
            let mut spans = Vec::new(); // (offset, length) as written into the table
            let mut at = 0u64;
            for i in 0..n {
                let length = next() % 120;
                // mostly contiguous tables, sometimes gaps, overlaps or wild offsets
                let offset = match next() % 8 {
                    0 => next() % 400,
                    1 => at.saturating_sub(1 + next() % 5),
                    2 => at + 1 + next() % 5,
                    _ => at,
                };
                at = offset + length;
                spans.push((offset, length));
                let label = if i == 0 {
                    "c2pa.hash.data.part".to_string()
                } else {
                    format!("c2pa.hash.data.part__{i}")
                };
                let label = if next() % 40 == 0 {
                    label.replace("data", "boxes")
                } else {
                    label
                };
                match next() % 20 {
                    0 => {} // missing from the store
                    1 => {
                        store.insert(label.clone(), json!({"hash": "%%"})); // unreadable
                    }
                    k => {
                        // mostly "match", so whole-table matches are common
                        store.insert(label.clone(), json!({"hash": [u64::from(k > 4)]}));
                    }
                }
                let mut part = json!({"location": {"byteOffset": offset, "length": length},
                    "hashAssertion": {"url": format!("self#jumbf=c2pa.assertions/{label}")}});
                match next() % 3 {
                    0 => {}
                    k => part["optional"] = json!(k == 1),
                }
                parts.push(part);
            }
            // The file length decides most outcomes, so derive it from the table:
            // an exact fit, cut at a part boundary (parts removed), cut inside a
            // part (cut short), a little longer (uncovered bytes), or anything.
            let (o, l) = spans[(next() % n as u64) as usize];
            let len = match next() % 6 {
                0 | 1 => at,
                2 => o,
                3 => o + next() % (l + 1),
                4 => at + 1 + next() % 5,
                _ => next() % 400,
            } as usize;
            let file = vec![0u8; len];
            let ma = json!({"parts": parts});
            let r = evaluate(&ma, &Value::Object(store), &file, &Oracle);
            if let Err(e) = check_invariants(&r, len as u64) {
                panic!("{e}\ntable: {ma}\nfile length: {len}\nreport: {r:?}");
            }
        }
    }

    // ---- naming parts from what the file declares ----

    use halftone_formats::parts::DeclaredPart;

    fn declared(offset: u64, length: u64, name: &str) -> DeclaredPart {
        DeclaredPart {
            offset,
            length,
            name: name.into(),
            declared_by: "container directory",
        }
    }

    /// What the Fold motion photo's container directory declares.
    fn fold_declared() -> Vec<DeclaredPart> {
        vec![
            declared(0, 2_047_094, "Primary"),
            declared(2_047_094, 9_718, "GainMap"),
            declared(2_056_812, 3_296_847, "MotionPhoto"),
        ]
    }

    fn signed(index: usize, offset: u64, length: u64) -> PartReport {
        PartReport {
            index,
            hash_assertion: format!("c2pa.hash.data.part__{index}"),
            locator: Locator::Bytes { offset, length },
            optional: index > 0,
            kind: None,
            present_bytes: None,
            declared_as: None,
            result: PartResult::Match,
        }
    }

    /// The Fold's signed table: still, gain map, clip.
    fn fold_report() -> Report {
        Report::new(
            Outcome::Match,
            None,
            vec![
                signed(0, 0, 2_047_094),
                signed(1, 2_047_094, 9_718),
                signed(2, 2_056_812, 3_296_847),
            ],
        )
    }

    fn names(r: &Report) -> Vec<Option<&str>> {
        r.parts.iter().map(|p| p.declared_as.as_deref()).collect()
    }

    #[test]
    fn the_fold_table_and_its_declarations_agree_part_for_part() {
        let mut r = fold_report();
        name_from_declared(&mut r, &fold_declared());
        assert_eq!(
            names(&r),
            [Some("Primary"), Some("GainMap"), Some("MotionPhoto")]
        );
        assert_eq!(r.declared_parts_agree, Some(true));
    }

    #[test]
    fn a_part_whose_bytes_are_gone_is_still_named() {
        let mut r = fold_report();
        r.parts[2].result = PartResult::Absent;
        name_from_declared(&mut r, &fold_declared());
        assert_eq!(r.parts[2].declared_as.as_deref(), Some("MotionPhoto"));
        assert_eq!(r.declared_parts_agree, Some(true));
    }

    #[test]
    fn only_what_lines_up_exactly_is_named() {
        // The declared gain map is one byte longer than the signed part, so the
        // declared clip starts one byte later: neither lines up; the still does.
        let mut r = fold_report();
        let off_by_one = vec![
            declared(0, 2_047_094, "Primary"),
            declared(2_047_094, 9_719, "GainMap"),
            declared(2_056_813, 3_296_847, "MotionPhoto"),
        ];
        name_from_declared(&mut r, &off_by_one);
        assert_eq!(names(&r), [Some("Primary"), None, None]);
        assert_eq!(r.declared_parts_agree, Some(false));
    }

    #[test]
    fn nothing_declared_or_nothing_signed_leaves_the_report_alone() {
        let mut r = fold_report();
        name_from_declared(&mut r, &[]);
        assert_eq!(names(&r), [None, None, None]);
        assert_eq!(r.declared_parts_agree, None);
        let mut empty = Report::not_evaluated("x");
        name_from_declared(&mut empty, &fold_declared());
        assert_eq!(empty.declared_parts_agree, None);
    }

    #[test]
    fn naming_never_changes_an_outcome_or_a_result() {
        let mut r = fold_report();
        r.parts[1].result = PartResult::Mismatch;
        let (outcome, results): (Outcome, Vec<PartResult>) = (
            r.outcome.clone(),
            r.parts.iter().map(|p| p.result.clone()).collect(),
        );
        name_from_declared(&mut r, &fold_declared());
        assert_eq!(r.outcome, outcome);
        assert_eq!(
            r.parts.iter().map(|p| p.result.clone()).collect::<Vec<_>>(),
            results
        );
    }
}
