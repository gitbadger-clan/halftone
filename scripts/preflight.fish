#!/usr/bin/env fish
# Pre-push sanity check for the Halftone workspace. Run from anywhere in the repo.
#
#   fish scripts/preflight.fish            # everything, including one network fetch
#   fish scripts/preflight.fish --offline  # skip the step that contacts Adobe
#
# Stops at the first failing step and says which one. Order: cheapest first.
# Steps 9-11 need the private corpus strata on disk.

set -g offline 0
contains -- --offline $argv; and set -g offline 1

set -l root (git rev-parse --show-toplevel); or exit 1
cd $root

set -g n 0
function step
    set -g n (math $n + 1)
    set_color --bold
    printf '\n== %d. %s\n' $n $argv[1]
    set_color normal
end
function ok
    set_color green
    printf 'ok  %s\n' $argv[1]
    set_color normal
end
function fail
    set_color red
    printf 'FAIL (step %d): %s\n' $n $argv[1] >&2
    set_color normal
    exit 1
end

set -l trust crates/halftone-c2pa/trust
set -l gen corpus/differential/04-generators

# --- 1 ---------------------------------------------------------------------------
step "cargo check, all targets and features: zero warnings (manifest lints included)"
set -l out (cargo check --workspace --all-targets --all-features 2>&1)
or begin
    printf '%s\n' $out
    fail "cargo check"
end
set -l warns (printf '%s\n' $out | string match -r '^warning.*')
test (count $warns) -eq 0
or begin
    printf '%s\n' $out | grep -A6 '^warning'
    fail (count $warns)" warning line(s)"
end
ok "no warnings"

# --- 2 ---------------------------------------------------------------------------
step "lockfile: one c2pa (0.91.x, D-011), one c2pa_cbor, one ureq; probe pins match"
set -l tree (cargo tree --workspace --all-features -e normal --prefix none 2>/dev/null)
or fail "cargo tree"
set -l c2pa (printf '%s\n' $tree | string match -r '^c2pa v[^ ]+' | sort -u)
test (count $c2pa) -eq 1; or fail "c2pa versions: $c2pa"
string match -q 'c2pa v0.91.*' $c2pa; or fail "expected c2pa 0.91.x (D-011), got $c2pa"
set -l cbor (printf '%s\n' $tree | string match -r '^c2pa_cbor v[^ ]+' | sort -u)
test (count $cbor) -eq 1; or fail "c2pa_cbor versions: $cbor"
set -l ureq (printf '%s\n' $tree | string match -r '^ureq v[^ ]+' | sort -u)
test (count $ureq) -eq 1; or fail "ureq versions: $ureq"
# The soft-binding probe reproduces what Halftone's c2pa discards, so it must run
# the same c2pa and c2pa_cbor (a scratch-branch bump once leaked past it).
set -l probe tools/d011-softbinding/Cargo.toml
set -l pin (string match -r 'c2pa = "=([^"]+)"' < $probe)[2]
test "$c2pa" = "c2pa v$pin"; or fail "$probe pins c2pa '$pin', workspace has $c2pa"
set -l cpin (string match -r 'c2pa_cbor = "=([^"]+)"' < $probe)[2]
test "$cbor" = "c2pa_cbor v$cpin"; or fail "$probe pins c2pa_cbor '$cpin', workspace has $cbor"
ok "$c2pa · $cbor · $ureq · probe pinned to the same"

# --- 3 ---------------------------------------------------------------------------
step rustfmt
cargo fmt --all --check; or fail "cargo fmt --check (run: cargo fmt --all)"
ok formatted

# --- 4, 5 ------------------------------------------------------------------------
step "clippy, no features (-D warnings)"
cargo clippy -q --workspace --all-targets --no-default-features -- -D warnings
or fail "clippy --no-default-features"
ok clean

step "clippy, all features (-D warnings)"
cargo clippy -q --workspace --all-targets --all-features -- -D warnings
or fail "clippy --all-features"
ok clean

# --- 6 ---------------------------------------------------------------------------
step "tests, all features"
cargo test -q --workspace --all-features; or fail "cargo test"
ok passed

# --- 7 ---------------------------------------------------------------------------
step "rustdoc (-D warnings): missing docs, broken intra-doc links"
env RUSTDOCFLAGS="-D warnings" cargo doc -q --workspace --no-deps --all-features
or fail "cargo doc"
ok clean

# --- 8 ---------------------------------------------------------------------------
step "ground truth: collected with the vendored EKU policy (by hash) and both anchor lists"
set -l want (shasum -a 256 $trust/C2PA-EKU-CONFIG.cfg | string split -f1 ' ')
for e in corpus/differential/*/expectations.json
    jq -e --arg h $want '.tools.c2patool == null
        or (.trust_config_sha256 == $h and .trust_anchors != null and .tsa_anchors != null)' $e >/dev/null
    or fail "$e: collected under other trust inputs; re-collect with --trust-anchors, --tsa-anchors and --trust-config"
end
# The corpus rules are reviewed like code: they must be tracked, not ignored.
set -l rules $gen/corpus-rules.json
git check-ignore -q $rules; and fail "$rules is ignored (corpus/.gitignore patterns are relative to corpus/)"
git ls-files --error-unmatch $rules >/dev/null 2>&1; or fail "$rules is not tracked"
ok "headers match the vendored trust inputs; corpus rules tracked"

# --- 9 ---------------------------------------------------------------------------
step "differential harness vs exiftool / c2patool"
set -l diff (cargo test -p halftone-cli --features c2pa --test differential -- --ignored --nocapture 2>&1)
set -l diff_status $status
printf '%s\n' $diff | string match -r '^differential.*'
test $diff_status -eq 0
or begin
    printf '%s\n' $diff | string match -r '^\|.*'
    fail "differential disagreements"
end
ok "0 disagreements"

# --- 10 --------------------------------------------------------------------------
step "corpus: files are what their names say"
set -l corp (cargo test --release -p halftone-cli --test corpus -- --ignored --nocapture 2>&1)
set -l corp_status $status
printf '%s\n' $corp | string match -r '^corpus.*'
test $corp_status -eq 0
or begin
    printf '%s\n' $corp | string match -r '^\|.*'
    fail "corpus integrity (HALFTONE_CORPUS_REPORT=1 shows every score; scripts/corpus-inspect.py shows the images)"
end
ok "names, formats and images consistent"

# --- 11 --------------------------------------------------------------------------
step "remote-manifest fetch: off by default, embedded files never fetch"
cargo build -q -p halftone-cli --features c2pa; or fail "cargo build"
set -l ht ./target/debug/ht
# Picked by pattern, so a path-token rename in the corpus does not break this step.
set -l ff (path filter $gen/adobe-firefly__firefly-image-5__web-download*__p3__3.png)[1]
set -l goog (path filter $gen/google-flow__nano-banana-2__web-download*__p1__1.jpeg)[1]
test -n "$ff"; or fail "no Firefly Image 5 p3 download in $gen"
test -n "$goog"; or fail "no Flow p1 download in $gen"
set -l c2pa_ev '.evidence[] | select(.source.name == "c2pa")'

$ht inspect --json --only manifest $ff \
    | jq -e "$c2pa_ev"' | .status == "inconclusive"
        and .details.remote_manifest_url != null
        and .details.fetched_from == null
        and (.rationale | contains("--fetch-remote-manifests"))' >/dev/null
or fail "default run on a remote-only file should be offline Inconclusive naming the flag"
ok "remote-only file, no flag: Inconclusive, URL reported, nothing fetched"

# The flag on a file with an embedded manifest must not reach the fetch path. Both
# streams are searched, so this holds whether tracing logs to stdout or stderr.
env RUST_LOG=halftone_c2pa=info $ht inspect --json --only manifest --fetch-remote-manifests $goog 2>&1 \
    | string match -q '*fetching remote C2PA manifest*'
and fail "an embedded manifest triggered a fetch"
$ht inspect --json --only manifest --fetch-remote-manifests $goog \
    | jq -e "$c2pa_ev | .details.fetched_from == null" >/dev/null
or fail "embedded manifest reported fetched_from"
ok "embedded manifest + flag: no fetch"

if test $offline -eq 1
    printf 'skip the online fetch (--offline)\n'
else
    $ht inspect --json --only manifest --fetch-remote-manifests $ff \
        | jq -e "$c2pa_ev"' | .details.fetched_from != null
            and .details.fetched_at != null
            and .details.validation_state != null
            and .details.remote_manifest_url == null
            and (.rationale | startswith("Fetched on request"))' >/dev/null
    or fail "fetch from cai-manifests.adobe.com (network down? rerun with --offline)"
    ok "remote-only file + flag: fetched, dated, validated"
end

# --- 12 --------------------------------------------------------------------------
step "docs and working tree"
grep -q 'Follow-up 2026-09-25' DIFFERENTIAL.md; or fail "D-004 follow-up missing from DIFFERENTIAL.md"
grep -q 'aged since' DIFFERENTIAL.md; or fail "D-010 ageing bullet missing from DIFFERENTIAL.md"
grep -q 'Correction: Bing declares no spec version' DIFFERENTIAL.md; or fail "D-011 Bing spec-version correction missing"
grep -q 'headers must now carry the vendored EKU policy' DIFFERENTIAL.md; or fail "D-011 OpenAI/EKU-header addendum missing"
grep -q fetch-remote-manifests README.md; or fail "README does not mention --fetch-remote-manifests"
ok "DIFFERENTIAL.md and README.md carry the recorded changes"
git status --short

set_color --bold green
printf '\nall %d steps passed\n' $n
set_color normal
