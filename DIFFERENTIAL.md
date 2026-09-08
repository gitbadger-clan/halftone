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

### D-004 · 2026-09-08 · Remote manifests make results depend on the network

Files: `cloud.jpg`, `cloudx.jpg`, `libpng-test_with_url.png` (c2pa-rs fixtures).
These carry only a URL to a manifest. c2patool with default settings fetched
`cloud.jpg`'s manifest from Adobe and reported `Valid`, signer Adobe Inc.; on a
machine without that route it errored. Halftone (c2pa-rs default
`verify.remote_manifest_fetch = true`) would have made the same outbound request
during an inspection, which an offline, privacy-preserving tool must never do.
Resolution: Halftone sets `verify.remote_manifest_fetch = false` and
`verify.ocsp_fetch = false` in its validation context and reports a remote-only
manifest as `Inconclusive` with the URL in `details.remote_manifest_url`; the
collector runs c2patool with the same settings and records `remote_manifest`; the
test asserts `Inconclusive`. Expectations no longer depend on the network.

### D-005 · 2026-09-08 · Duplicate bytes under two names

`thumbnail.jpg` and `IMG_0003.jpg` in the c2pa-rs fixtures are byte-identical
(same SHA-256). The test matches by hash, so both names compare against one row.
Not a bug; noted so a count mismatch is not chased.

### D-006 · 2026-09-08 · Leading whitespace in a declared digitalSourceType

`C_with_CAWG_data.jpg` declares `" http://cv.iptc.org/…/digitalCapture"` with a
leading space inside the signed assertion. Both the collector and Halftone's
`code_of` trim before taking the last path segment, so both read `digitalCapture`.
Kept as a fixture of the "trim, then match exactly" rule: whitespace is tolerated,
case is not.

### D-007 · 2026-09-08 · Stapled OCSP assertion fails to decode with c2pa_cbor 0.77.2

File: `ocsp_with_assertion.jpg` (c2pa-rs fixture; Photoshop manifest with an OCSP
response stapled as `c2pa.certificate-status`, plus an ingredient manifest).
Halftone built from the workspace `Cargo.lock` (c2pa 0.90.20, c2pa_cbor 0.77.2)
errored at `Reader::from_stream`: "could not decode assertion
c2pa.certificate-status (content type application/cbor): invalid value: byte array,
expected a string", and reported `Inconclusive` / "could not be read". The same
source built by `cargo install` (fresh resolve, c2pa_cbor 0.77.4) read it as `Valid`,
matching c2patool 0.27.20. Isolated by `cargo update --dry-run`: `c2pa_cbor` is the
only crate on the decode path that differed. Cause: a byte-string deserialisation
bug in c2pa_cbor 0.77.2, a transitive dependency of c2pa-rs. Resolution:
`cargo update -p c2pa_cbor`, lockfile committed, shipped binaries built with
`cargo install --locked`; optionally `c2pa_cbor = ">=0.77.4"` as a direct dependency
of `halftone-c2pa` to make the bad range a build error. The file stays in the corpus
as the regression guard. A stapled OCSP response is what a careful signer adds so
validators need no network; failing the read would have told a client their
manifest was broken.
