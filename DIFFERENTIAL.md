# Differential log

Halftone's deterministic sources are checked against ExifTool and c2patool on a
corpus of real files (`corpus/differential/`, ground truth in `expectations.json`,
collected by `uv run scripts/differential.py`, asserted by
`crates/halftone-cli/tests/differential.rs`). Every disagreement class ever seen is
recorded here with its cause and resolution, whether the fix landed in Halftone, in
the expectations, or in how a reference tool is invoked.

Regenerate ground truth:

    uv run scripts/differential.py corpus/differential \
        --trust-anchors crates/halftone-c2pa/trust/C2PA-TRUST-LIST.pem

Run:

    cargo test -p halftone-cli --features c2pa --test differential -- --nocapture --ignored

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
Follow-up 2026-09-25 (the online mode D-009 called for): ht inspect
--fetch-remote-manifests fetches a remote-only manifest on request; off by
default, and survive, bench and this test never pass it. Fetching needs c2pa's
fetch_remote_manifests compile feature (the runtime setting alone does nothing),
now enabled with Halftone's c2pa feature. The file is read offline first; the
reference must be https:// to a host name; the fetch uses a 20 s timeout and at
most 3 HTTPS-only redirects (c2pa's default agent has no timeout and follows 10
redirects to any scheme). A fetched manifest reports details.fetched_from and
details.fetched_at, never remote_manifest_url, so the batch row keeps "remote
reference, not fetched" and "fetched" apart.

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

### D-008 · 2026-09-09 · c2patool ignored the trust anchors; `Valid` vs `Trusted` on Google-signed files

Files: every Google-signed file in 04-generators (Flow downloads, and the
aggregator download, which turned out to be Google-signed).
The collector passed the vendored trust list through `$C2PATOOL_TRUST_ANCHORS`,
which c2patool only reads under its `trust` sub-command; with a plain
`c2patool <file>` it is ignored, so c2patool reported `Valid` while Halftone,
with the same list configured, reported `Trusted`. Resolution: the collector now
writes the PEM into the settings TOML (`[trust] trust_anchors`, `verify.verify_trust
= true`), verified against the c2pa-rs test root (`Valid` → `Trusted`). Re-collect
every stratum that was collected with `--trust-anchors` (01, 04).
Note for the trust picture: with the 2026-08-14 vendored list, Google LLC chains
(`Trusted`), Microsoft Corporation does not (`Valid`), and the Firefly files come
back `Inconclusive` because they carry only a remote manifest reference (D-009).
Update 2026-09-24: c2patool also reads a settings file it does not mention when
it uses one, `$XDG_CONFIG_HOME/c2pa/c2pa.toml` by default or the path in
`$C2PATOOL_SETTINGS` (`c2patool --help`, `--settings`). An operator with the
trust list in either gets `Trusted` from a plain `c2patool <file>`, and this
entry would never have surfaced on that machine. The collector now runs every
c2patool call with both variables unset and `XDG_CONFIG_HOME` pointed at an
empty directory (`c2patool_env()` in `scripts/differential.py`), so the only
settings in play are the ones it writes and records in `expectations.json`.
Reproduction of the original disagreement: `scripts/pub/d008.fish`.

### D-009 · 2026-09-10 · Adobe Firefly downloads carry only a remote manifest reference

Files: every Firefly download in 04-generators (Image 5 PNG, Image 4 Ultra JPEG,
Image 5 upscaled JPEG).
No embedded manifest. The file's XMP carries a `dcterms:provenance` link to
`https://cai-manifests.adobe.com/manifests/urn-c2pa-…-adobe`, which is where the
signed manifest lives. Halftone (offline by construction since D-004) reports
`Inconclusive` with the URL in `details.remote_manifest_url`; c2patool with the
same settings refuses with "must fetch remote manifests"; the two agree.
Consequences:
- Offline verification of a Firefly file is impossible by design; the reference
  is only as durable as the XMP packet (the same fragility as an IPTC field), and
  every copy/save path in the batch dropped it.
- The report must show this as its own state, "remote reference, not fetched",
  distinct from Absent and from a broken manifest; the batch row now carries
  `manifest.remote_manifest_url` and the matrix prints
  "manifest remote at <host>, not fetched".
- An explicit, logged online mode (`--fetch-remote-manifests`, off by default,
  recorded in details with URL and time) is the only way to validate Adobe output
  in a client scan. Agencies are Adobe-heavy; decide before week 4's report.

### D-010 · 2026-09-20 · Canva's Content Credentials expire after eight days

Files: every Canva export in 04-generators (11 files, PNG and JPG, plain and
edited variants).
On 2026-09-10 both tools read `Valid` (`signingCredential.untrusted` only). On
2026-09-20 both read `Invalid` with `signingCredential.expired` +
`signingCredential.untrusted`. The signing certificate (`O=Canva, CN=Canva
Signing`) is valid 2026-09-09T23:52:03Z to 2026-09-17T23:52:03Z — eight days —
and the manifest carries a single `c2pa.actions.v2` assertion and no
`c2pa.time-stamp`, so nothing fixes the signing time inside that window. The
content hash still matches; the file is unchanged. Google, Microsoft and Ideogram
rows are unchanged on the same day.
Not a tool disagreement (Halftone and c2patool agree on both dates) but a
time-dependent ground truth: the differential test correctly fails against
expectations collected on the 10th.
Resolutions:
- Layer 1 now distinguishes expiry from breakage: `Inconclusive` with a rationale
  naming the expired certificate, the missing time-stamp, and that the file may
  have validated when made; the generic "does not validate" stays for hash and
  signature failures.
- The collector records `collected_at` in the expectations header and the test
  prints it per stratum; re-collected 2026-09-20; the committed state is the
  2026-09-24 re-collect after the D-008 isolation.
- Every validation state in a report carries the date it was evaluated; the
  methodology says why.
For a provider relying on Content Credentials for Article 50 evidence, this is
the finding of the corpus so far: the evidence has a shelf life the provider is
unlikely to know about, and a regulator checking a file a month later sees
`Invalid`.
