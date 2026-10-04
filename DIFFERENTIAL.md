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
Update 2026-09-27. Measured, not only designed: `scripts/offline-docker.fish` at
`75e33fb` ran ht over 236 files (all strata plus the C2PA v2.2 samples) in a
container with no network interface, with every `connect()` traced under a
positive control. Result: 0 attempts, and identical output with the network
available. Linux build of this checkout; the macOS binary is not exercised.
Log: `target/versiondiff/offline-20260927T192556Z-75e33fb.log`.

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
Follow-up 2026-09-25: c2pa 0.91 requires c2pa_cbor 0.78.0, above the floor, so the
direct dependency and the `use c2pa_cbor as _` pin are removed (D-011).
`ocsp_with_assertion.jpg` in 01 re-checks the decode on every run.

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
- The differential test reports a Valid/Trusted → Invalid change whose only new
  code is `signingCredential.expired` (nothing broken) as `aged since
  <collected_at>: re-collect`, not as a disagreement; same split as the Layer 1
  rationale.

### D-011 · 2026-09-25 · c2pa 0.91: trust anchors silently dropped; Bing Invalid

c2pa 0.91.0 (c2patool 0.28.0) replaces `trust.trust_anchors` / `trust.user_anchors`
with typed `trust.anchors` entries (kind manifest / tsa / cawg). Validation reads only
the new field; the old keys still load through `Settings::with_value`, which Halftone
used, without migration and without error. On 0.91 Halftone therefore validated with
no anchors: 10 Google / aggregator rows went Trusted → Valid. Bing (7 rows) went
Valid → Invalid with `signingCredential.invalid` + `claim.malformed`; c2patool 0.27
(c2pa 0.90) still says Valid.
Resolution: `trust.rs` keeps each bundle separate with its kind; `context()` sets
typed anchors, and a unit test reads them back from the `Context`. The collector
passes the same two lists as typed anchors (`--tsa-anchors`) and refuses a c2patool
older than 0.28. The old keys are removed in 0.92 (scheduled mid-November).
Bing (7 rows) fails two independent 0.91 checks. (1) `signingCredential.invalid`,
"certificate missing required EKU": the signer carries only 1.3.6.1.4.1.311.76.59.1.9
(MS C2PA Signing, critical). c2pa's built-in EKU allow-list includes it, but 0.91's
`Store::from_context` clears the list and re-adds only `trust.trust_config`.
(2) `claim.malformed`, "soft binding assertion could not be decoded": new in 0.91;
Microsoft's `com.microsoft.invismark.1` block `value` is a text string where the CDDL
declares a byte string (c2pa-rs #2689), the likely cause, unconfirmed because c2pa
discards the underlying decode error.
Resolution: Halftone passes an explicit EKU policy (`trust/C2PA-EKU-CONFIG.cfg`, the
six OIDs of c2pa-rs's default list, recorded in `details.trust.eku_config`); the
collector passes the same file (`--trust-config`). With it, (1) disappears in both
tools; (2) stands and is reported as the reference reports it. The `Invalid`
rationale now separates conformance failures from broken hashes or signatures.
Open: whether a vendor-private EKU is conformant under the C2PA signer profile.
Evidence: `scripts/d011-evidence.fish`.

Addendum 2026-09-27 · `ocsp.notRevoked` moved buckets. c2pa 0.90.22 logs a
verified stapled OCSP response as informational; 0.91.0 logs the same code as
success (`crypto/cose/ocsp.rs`). Our comparison tooling read only failure and
informational codes, so on 0.91 the code looked dropped on the 23 files
carrying stapled responses. `scripts/ocsp-extract.py` confirms those responses
pass every check 0.91 applies. Tooling now records all three buckets.

Addendum 2026-09-29 · Bing's signing certificate conforms to C2PA 2.4. Closes the
"Open" question above: is a vendor-private EKU conformant? Check (1) above,
`signingCredential.invalid` "certificate missing required EKU", came from 0.91
dropping c2pa's built-in EKU list, and Halftone now passes that list explicitly.
This addendum records whether Microsoft's certificate is acceptable under the spec
itself, independent of any library default. All seven Bing manifests declare
`specVersion` 2.4.0 [wrong: that is the validator's version; see 2026-10-02]. The
leaf carries one critical EKU, 1.3.6.1.4.1.311.76.59.1.9 (MS C2PA Signing); Key
Usage is critical with digitalSignature and nonRepudiation; Basic Constraints
`CA:FALSE`. Under C2PA 2.4 this conforms: §14.5.1.1 requires a non-empty EKU and
digitalSignature, forbids anyExtendedKeyUsage, and names no required claim-signing
OID; §14.5.1.2 allows at most one purpose. The certificate lacks
c2pa-kp-claimSigning, so it is outside the C2PA Trust List, which covers only that
EKU since 2.2 (§14.4.1). It reaches `Trusted` only through an anchor configured for
Microsoft's OID, so `Valid` is the correct ceiling with our vendored bundles. Our
EKU config is a flat list applied to every anchor, broader than the per-anchor
association in §14.5.1.2. Checked on 7/7 Bing files (`c2patool --detailed` for the
spec version; `c2patool --certs` into `openssl x509 -ext` for the certificate).

Addendum 2026-09-30 · Bing's `claim.malformed` is a misspelled unit in Microsoft's
soft-binding assertion; the #2689 explanation in check (2) above is wrong. Check
(2) is the second of Bing's two 0.91 failures: `claim.malformed`, "soft binding
assertion could not be decoded", on all seven Bing files. This entry named
Microsoft's text-typed `value` (c2pa-rs #2689) as the likely cause, unconfirmed
because c2pa discards the decode error. `tools/d011-softbinding` extracts the raw
assertion bytes, runs the same decode c2pa does (`SoftBinding::from_assertion`, i.e.
`c2pa_cbor::from_slice`), and prints the error `Claim::verify_soft_binding_alg`
throws away: `blocks[0].scope.region.region[0].shape.unit`: unknown variant
`percentage`, expected `pixel` or `percent`. Microsoft scopes the binding to the
whole image (rectangle, origin 0,0, 100×100) with unit "percentage"; the C2PA 2.4
region-of-interest CDDL allows only "pixel" and "percent", and the manifest
declares 2.4.0 [wrong: Bing declares no version; see 2026-10-02], so the assertion
is non-conformant against the version it claims and c2pa is right to reject it.
The text-typed `value` is not the cause: c2pa_cbor passes text to serde_bytes,
which accepts it, and re-encoding the assertion with `value` as a byte string fails
with the same error. The failure is new in 0.91 because 0.91 is the first version
to decode soft-binding assertions during validation, not because a field was
retyped. Checked on 7/7 Bing files.

Addendum 2026-10-02 · Correction: Bing declares no spec version. The 2.4.0 cited
in the 2026-09-29 and 2026-09-30 addenda is `validation_results.specVersion`,
c2patool's own version, not a field of the manifest. OpenAI's claim, by contrast,
declares 2.2.0 at `manifests["urn:c2pa:2b45b18d-1a6d-4c3a-9c7f-fbb47bf22a43"].claim.specVersion` and in its
claim_generator_info, while its validation_results also say 2.4.0. Bing's manifest
is claim v2 with fields alg, claim_generator_info, claim_version,
created_assertions, gathered_assertions, instanceID, signature, and no
specVersion; generator "Microsoft 1.0" built with c2pa-rs 0.84.1; assertions
c2pa.actions.v2, c2pa.hash.data, c2pa.soft-binding. Read with a jq path query over
`c2patool --detailed`, which shows where each version string sits;
`scripts/d011-evidence.fish` now reads the claim's own field and prints "none
declared" when it is absent.
Soft binding: in c2pa-rs 0.84.1, the library Microsoft built with, the
region-of-interest unit enum has two variants, `Pixel` and `Percent`
(`src/assertions/region_of_interest.rs`); "percentage" appears only in a doc
comment ("Use percentage."). Microsoft's "percentage" therefore does not come from
its own toolkit's types, provided 0.84.1 serialises `Percent` as "percent" as 0.91
does (open item d). The 2026-09-30 conclusion that c2pa is right to reject the
assertion rests on that, not on a declared version.
Open: (a) which rules govern a claim v2 manifest with no declared version (the
Versioning chapter); (b) whether any 2.x version's region-of-interest CDDL allows
"percentage"; (c) whether Bing's certificate meets every 2.x signer profile, which
the 2026-09-29 conclusion "conforms under 2.4" assumed; (d) the serialised names
of 0.84.1's unit variants.

Addendum 2026-10-02 · Check (1) again, for OpenAI, from an incomplete
re-collection; headers must now carry the vendored EKU policy. On 2026-10-01
04-generators was re-collected with `--trust-anchors` only, without
`--tsa-anchors` and `--trust-config`. c2patool 0.28.0 then reported the first
OpenAI-signed file in the corpus
(`adobe-firefly__gpt-image-2.5-flare__web-download__p1__1.png`, claim v2,
"OpenAI Media Service API", declaring specVersion 2.2.0, passed through by Firefly
unchanged) as `Invalid`, `signingCredential.invalid`, "certificate missing
required EKU"; Halftone, with the vendored policy, reported `Trusted`. Same
mechanism as check (1): without `trust.trust_config`, 0.91 accepts only
emailProtection, timeStamping and OCSPSigning, and the leaf carries neither. The
leaf (CN=OpenAI Media Service, O=OpenAI OpCo, LLC; valid 2026-03-23 to
2027-03-24) has Basic Constraints critical `CA:FALSE`, Key Usage critical
digitalSignature and nonRepudiation, and two EKUs: 1.3.6.1.4.1.62558.2.1
(c2pa-kp-claimSigning) and 1.3.6.1.5.5.7.3.36 (id-kp-documentSigning), both in
`trust/C2PA-EKU-CONFIG.cfg`. Halftone on c2pa 0.91.1 (scratch branch) also says
`Trusted`, so the patch version is not involved. Unlike Bing, the certificate
carries c2pa-kp-claimSigning, so it is within the C2PA Trust List's scope.
Resolution: 04 re-collected 2026-10-02 with all three trust inputs; the OpenAI
file reads `Trusted` in both tools and 214/214 files agree. Only 04 was affected;
01–03 (collected 2026-09-26) already carried the EKU file. `differential.rs` now
reports a header row when c2patool ran without `trust_config` set to
`crates/halftone-c2pa/trust/C2PA-EKU-CONFIG.cfg`, or with manifest anchors but
without the TSA list (`header_disagreements`, unit tests in `header_rule`). The
OpenAI file stays as the regression guard.
Open: whether two EKUs conform to the C2PA 2.2 signer profile, the version the
claim declares. The 2026-09-29 addendum reads 2.4 §14.5.1.2 as "at most one
purpose"; whether that binds the certificate or the validator's per-anchor
association is to be checked against the 2.2 text.
