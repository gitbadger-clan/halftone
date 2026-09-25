#!/usr/bin/env fish
# Pre-push sanity check for the Halftone workspace. Run from anywhere in the repo.
#
#   scripts/preflight.fish            # everything, including one network fetch
#   scripts/preflight.fish --offline  # skip the step that contacts Adobe
#
# Stops at the first failing step and says which one. Order: cheapest first.

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
step "lockfile: one c2pa_cbor (0.77.x, D-007), one ureq"
set -l tree (cargo tree --workspace --all-features -e normal --prefix none 2>/dev/null)
or fail "cargo tree"
set -l cbor (printf '%s\n' $tree | string match -r '^c2pa_cbor v[^ ]+' | sort -u)
test (count $cbor) -eq 1; or fail "c2pa_cbor versions: $cbor"
string match -q 'c2pa_cbor v0.77.*' $cbor; or fail "expected 0.77.x (D-007), got $cbor"
set -l ureq (printf '%s\n' $tree | string match -r '^ureq v[^ ]+' | sort -u)
test (count $ureq) -eq 1; or fail "ureq versions: $ureq"
set -l c2pa (printf '%s\n' $tree | string match -r '^c2pa v[^ ]+' | sort -u)
ok "$cbor · $ureq · $c2pa"

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

# --- 9 ---------------------------------------------------------------------------
step "remote-manifest fetch: off by default, embedded files never fetch"
cargo build -q -p halftone-cli --features c2pa; or fail "cargo build"
set -l ht ./target/debug/ht
set -l gen corpus/differential/04-generators
set -l ff $gen/adobe-firefly__firefly-image-5__web-download__p3__3.png
set -l goog $gen/google-flow__nano-banana-2__web-download-thumb__p1__1.jpeg
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

# --- 10 --------------------------------------------------------------------------
step "docs and working tree"
grep -q 'Follow-up 2026-09-25' DIFFERENTIAL.md; or fail "D-004 follow-up missing from DIFFERENTIAL.md"
grep -q 'aged since' DIFFERENTIAL.md; or fail "D-010 ageing bullet missing from DIFFERENTIAL.md"
grep -q fetch-remote-manifests README.md; or fail "README does not mention --fetch-remote-manifests"
ok "DIFFERENTIAL.md and README.md carry today's changes"
git status --short

set_color --bold green
printf '\nall %d steps passed\n' $n
set_color normal
