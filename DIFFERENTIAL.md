# Differential log

Halftone's deterministic sources are checked against ExifTool and c2patool on a
corpus of real files (`corpus/differential/`, ground truth in `expectations.json`,
collected by `scripts/differential.py`, asserted by
`crates/halftone-cli/tests/differential.rs`). Every disagreement class ever seen is
recorded here with its cause and resolution, whether the fix landed in Halftone, in
the expectations, or in how a reference tool is invoked.

Regenerate ground truth:

    scripts/differential.py corpus/differential \
        --trust-anchors crates/halftone-c2pa/trust/C2PA-TRUST-LIST.pem

Run:

    cargo test -p halftone-cli --features c2pa --test differential -- --nocapture

## Entries

### D-001 · 2026-09-08 · ChatGPT PNG carries no XMP copy of DigitalSourceType

Files: ChatGPT image download (PNG), two samples.
ExifTool: no XMP at all. c2patool: `validation_state = Trusted` with the vendored
trust list, `claim_generator_info[0].name = "OpenAI Media Service API"`, one
`c2pa.actions.v2` assertion, `c2pa.created` with
`digitalSourceType = trainedAlgorithmicMedia`.
Halftone: agrees on every field.
Finding, not a bug: the only source-type declaration is inside the signed manifest.
Contradicts published claims that OpenAI also writes the IPTC field in XMP. The
marking survives exactly as far as the `caBX` chunk does.

### D-002 · 2026-09-08 · c2patool reports `Valid`, Halftone `Trusted`

Cause: c2patool ships no trust anchors; without `--trust-anchors` it cannot chain
the signer. Resolution: the collector records `trust_anchors: null` and the test
accepts `Trusted` where c2patool said `Valid`; pass the vendored PEM for an exact
comparison.

### D-003 · 2026-09-08 · Claim v2 generator name

Cause: claim v2 manifests carry `claim_generator_info: [{name, version, …}]` and no
`claim_generator` string; Halftone showed `null`. Resolution: fallback to
`name` + `version` of the first entry, matching what the collector extracts from
c2patool's JSON. Note the extra `org.contentauth.c2pa_rs` key c2pa-rs adds to that
object; it is not part of the name.
