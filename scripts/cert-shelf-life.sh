#!/usr/bin/env bash
# cert-shelf-life.sh: signing-certificate window and time-stamp status of one
# file's active C2PA manifest, under Halftone's vendored trust snapshot.
#
#   bash scripts/cert-shelf-life.sh [--json] [--trust-dir DIR] FILE
#
# Prints facts only: validation state, the signer's serial and validity window,
# and whether a time-stamp fixes the signing time under the loaded TSA list. Expiry
# is judged by the verifier at the time it checks, so the result depends on the
# date of checking and on the trust lists; both are printed with every result.
#
# Trust: the same inputs the differential collector passes to c2patool
# (scripts/differential.py; DIFFERENTIAL.md D-008, D-011, D-012): manifest and
# TSA anchors as typed [[trust.anchors]] tables, the vendored EKU policy as
# trust.trust_config, no remote manifest fetch, no OCSP, and an isolated
# XDG_CONFIG_HOME with C2PATOOL_SETTINGS / C2PATOOL_TRUST_ANCHORS unset.
# The anchor files are checked against the hashes in DIR/SNAPSHOT.
#
# Requires: bash 3.2+, c2patool >= 0.28 (c2pa 0.91), jq 1.6+, openssl or
# LibreSSL, bc, shasum or sha256sum.
#
# Exit: 0 reported; 1 no manifest, remote-only manifest, unreadable, or the
# certificate chain does not match the signer; 2 usage, environment, or trust
# configuration error.

set -euo pipefail

usage() {
  echo "usage: bash scripts/cert-shelf-life.sh [--json] [--trust-dir DIR] FILE" >&2
  exit 2
}
die() {
  echo "cert-shelf-life: $*" >&2
  exit 2
}
fail() {
  echo "cert-shelf-life: $*" >&2
  exit 1
}

# --- arguments ---------------------------------------------------------------
json=0
trust_dir=""
file=""
while [ $# -gt 0 ]; do
  case "$1" in
  --json) json=1 ;;
  --trust-dir)
    [ $# -ge 2 ] || usage
    trust_dir="$2"
    shift
    ;;
  -h | --help) usage ;;
  -*)
    echo "unknown option: $1" >&2
    usage
    ;;
  *)
    [ -z "$file" ] || usage
    file="$1"
    ;;
  esac
  shift
done
[ -n "$file" ] || usage
[ -f "$file" ] || die "not a file: $file"

if [ -z "$trust_dir" ]; then
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  trust_dir="$here/../crates/halftone-c2pa/trust"
fi
[ -d "$trust_dir" ] || die "trust directory not found: $trust_dir (pass --trust-dir)"
trust_dir="$(cd "$trust_dir" && pwd)"

# --- tools -------------------------------------------------------------------
for t in c2patool jq openssl bc; do
  command -v "$t" >/dev/null 2>&1 || die "$t not found on PATH"
done
if command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
elif command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
else
  die "neither shasum nor sha256sum found"
fi

# Typed [[trust.anchors]] arrived in c2patool 0.28 (c2pa 0.91). An older c2patool
# ignores the table and validates with no anchors at all, silently (D-011).
c2pa_ver="$(c2patool --version 2>/dev/null | sed -n 's/.*[^0-9]\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' | head -n 1)"
[ -n "$c2pa_ver" ] || die "could not read c2patool --version"
maj="${c2pa_ver%%.*}"
rest="${c2pa_ver#*.}"
min="${rest%%.*}"
if [ "$maj" -eq 0 ] && [ "$min" -lt 28 ]; then
  die "c2patool $c2pa_ver: typed trust anchors need c2patool >= 0.28"
fi

# --- trust snapshot ----------------------------------------------------------
anchors="$trust_dir/C2PA-TRUST-LIST.pem"
tsa="$trust_dir/C2PA-TSA-TRUST-LIST.pem"
eku="$trust_dir/C2PA-EKU-CONFIG.cfg"
snapshot="$trust_dir/SNAPSHOT"
for f in "$anchors" "$tsa" "$eku" "$snapshot"; do
  [ -f "$f" ] || die "missing $f"
done
for f in "$anchors" "$tsa"; do
  name="$(basename "$f")"
  want="$(awk -v n="$name" '$1 == n && $2 == "sha256" { print $3 }' "$snapshot")"
  [ -n "$want" ] || die "$snapshot has no sha256 for $name"
  got="$(sha256 "$f")"
  [ "$got" = "$want" ] || die "$name does not match $snapshot (sha256 $got, expected $want)"
done
trust_id="$(head -n 1 "$snapshot")"

# --- isolated c2patool environment -------------------------------------------
tmp="$(mktemp -d "${TMPDIR:-/tmp}/cert-shelf-life.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/xdg"
settings="$tmp/settings.toml"
{
  printf '[verify]\nremote_manifest_fetch = false\nocsp_fetch = false\nverify_trust = true\n'
  # [trust] keys before any [[trust.anchors]] table.
  printf '\n[trust]\ntrust_config = """\n'
  cat "$eku"
  printf '"""\n'
  printf '\n[[trust.anchors]]\ntrust_kind = "manifest"\ntrust_uri = "cert-shelf-life:manifest:%s"\ntrust_anchors = """\n' "$(basename "$anchors")"
  cat "$anchors"
  printf '"""\n'
  printf '\n[[trust.anchors]]\ntrust_kind = "tsa"\ntrust_uri = "cert-shelf-life:tsa:%s"\ntrust_anchors = """\n' "$(basename "$tsa")"
  cat "$tsa"
  printf '"""\n'
} >"$settings"

c2p() {
  env -u C2PATOOL_SETTINGS -u C2PATOOL_TRUST_ANCHORS XDG_CONFIG_HOME="$tmp/xdg" \
    c2patool --settings "$settings" "$@"
}

# --- manifest ----------------------------------------------------------------
rc=0
c2p "$file" >"$tmp/out.json" 2>"$tmp/err.txt" || rc=$?
both="$(cat "$tmp/out.json" "$tmp/err.txt")"
lower="$(printf '%s' "$both" | tr '[:upper:]' '[:lower:]')"
case "$lower" in
*"no claim found"* | *"no manifest"* | *jumbfnotfound*)
  fail "no C2PA manifest in $file"
  ;;
esac
remote="$(printf '%s\n' "$both" | sed -n 's/.*must fetch remote manifests from url \([^[:space:]]*\).*/\1/p' | head -n 1)"
[ -z "$remote" ] || fail "$file carries only a reference to a remote manifest ($remote); not fetched"
jq -e 'type == "object"' "$tmp/out.json" >/dev/null 2>&1 ||
  fail "c2patool output is not JSON (exit $rc): $(head -c 400 "$tmp/err.txt")"

# One jq pass into shell variables. Time-stamp codes come from validation_results:
# timeStamp.validated / .trusted are success codes and timeStamp.untrusted is
# informational, so none of them appear in the failures-only validation_status.
eval "$(jq -r '
    (.manifests[.active_manifest // ""] // {}) as $m
    | (.validation_results.activeManifest // null) as $vr
    | ($vr | if . == null then [] else [(.success, .informational, .failure)[]?.code] end) as $all
    | {
        state: (.validation_state // ""),
        has_results: (if $vr == null then "0" else "1" end),
        failures: ([.validation_status[]?.code] | unique | join(" ")),
        ts_codes: ([$all[] | select(startswith("timeStamp."))] | unique | join(" ")),
        issuer: ($m.signature_info.issuer // ""),
        ts_time: ($m.signature_info.time // ""),
        serial_dec: ($m.signature_info.cert_serial_number // "")
      }
    | to_entries[] | "\(.key)=\(.value | tostring | @sh)"
' "$tmp/out.json")"

[ "$has_results" = 1 ] || die "c2patool output has no validation_results; time-stamp status cannot be read"

has_code() { case " $1 " in *" $2 "*) return 0 ;; *) return 1 ;; esac }

if has_code "$ts_codes" timeStamp.trusted; then
  ts_status="trusted"
elif has_code "$ts_codes" timeStamp.untrusted; then
  ts_status="untrusted"
elif [ -n "$ts_codes" ]; then
  other="$(printf '%s\n' $ts_codes | grep -v '^timeStamp\.validated$' | tr '\n' ' ' | sed 's/ $//')"
  if [ -n "$other" ]; then ts_status="failed: $other"; else ts_status="validated, trust not reported"; fi
else
  ts_status="none"
fi

# --- signing certificate -----------------------------------------------------
c2p "$file" --certs >"$tmp/chain.pem" 2>/dev/null || fail "c2patool --certs failed for $file"
# openssl x509 reads the first certificate in the chain: the signer (leaf).
leaf="$(openssl x509 -noout -serial -startdate -enddate -subject <"$tmp/chain.pem" 2>/dev/null)" ||
  fail "no readable certificate in the manifest's chain"
field() { printf '%s\n' "$leaf" | sed -n "s/^$1=[[:space:]]*//p" | head -n 1; }
serial_hex="$(field serial | tr '[:lower:]' '[:upper:]')"
nb_raw="$(field notBefore)"
na_raw="$(field notAfter)"
subject="$(field subject)"

# c2patool reports the serial in decimal, openssl in hex. If they differ, the
# certificate read here is not the one that signed the active manifest.
serial_from_hex="$(printf 'ibase=16; %s\n' "$serial_hex" | BC_LINE_LENGTH=0 bc | tr -d '\\\n')"
if [ -n "$serial_dec" ] && [ "$serial_from_hex" != "$serial_dec" ]; then
  fail "leaf serial $serial_hex does not match the signer serial c2patool reports ($serial_dec)"
fi

iso() { jq -rn --arg d "$1" '$d | sub(" GMT$"; "") | gsub(" +"; " ") | strptime("%b %d %H:%M:%S %Y") | mktime | todate'; }
not_before="$(iso "$nb_raw")"
not_after="$(iso "$na_raw")"
checked_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
if openssl x509 -noout -checkend 0 <"$tmp/chain.pem" >/dev/null 2>&1; then
  window="inside"
else
  window="passed"
fi
file_sha="$(sha256 "$file")"

# --- note: what the facts above mean for a later check ------------------------
expired=0
has_code "$failures" signingCredential.expired && expired=1
note=""
case "$state" in
Valid | Trusted)
  case "$ts_status" in
  none)
    note="The signature carries no time-stamp, so nothing fixes when it was made; a conformant verifier checking after $not_after reports signingCredential.expired."
    ;;
  untrusted)
    note="The time-stamp is not on the loaded TSA list, so under this list it does not fix the signing time; a check after $not_after reports signingCredential.expired. A verifier that trusts this TSA judges the certificate as of $ts_time."
    ;;
  trusted)
    note="A time-stamp on the loaded TSA list fixes the signing time at $ts_time; the certificate window is judged at that time, not at the time of checking."
    ;;
  *)
    note="The time-stamp did not validate ($ts_status), so it does not fix the signing time; a check after $not_after reports signingCredential.expired."
    ;;
  esac
  ;;
Invalid)
  if [ "$expired" = 1 ]; then
    case "$ts_status" in
    none) why="the signature carries no time-stamp" ;;
    untrusted) why="its time-stamp is not on the loaded TSA list" ;;
    *) why="no time-stamp fixes the signing time ($ts_status)" ;;
    esac
    note="The certificate's validity ended $not_after and $why, so this check, at $checked_at, reports signingCredential.expired. The file may have validated before that date."
    if [ "$ts_status" = untrusted ]; then
      note="$note A verifier that trusts this TSA judges the certificate as of $ts_time."
    fi
  else
    note="Validation fails for reasons other than certificate expiry (see failures); the certificate window is reported as read."
  fi
  ;;
esac

# --- output ------------------------------------------------------------------
if [ "$json" = 1 ]; then
  jq -n \
    --arg file "$file" --arg sha256 "$file_sha" --arg checked_at "$checked_at" \
    --arg c2patool "$c2pa_ver" --arg trust "$trust_id" \
    --arg state "$state" --arg failures "$failures" \
    --arg issuer "$issuer" --arg subject "$subject" \
    --arg serial_hex "$serial_hex" --arg serial_dec "$serial_dec" \
    --arg not_before "$not_before" --arg not_after "$not_after" --arg window "$window" \
    --arg ts_status "$ts_status" --arg ts_codes "$ts_codes" --arg ts_time "$ts_time" \
    --arg note "$note" \
    '{
            file: $file, sha256: $sha256, checked_at: $checked_at,
            c2patool: $c2patool, trust_snapshot: $trust,
            validation_state: $state,
            failures: ($failures | split(" ") | map(select(. != ""))),
            signer: { issuer: $issuer, subject: $subject,
                      serial_hex: $serial_hex, serial_dec: $serial_dec,
                      not_before: $not_before, not_after: $not_after,
                      window_at_check: $window },
            time_stamp: { status: $ts_status,
                          codes: ($ts_codes | split(" ") | map(select(. != ""))),
                          time: (if $ts_time == "" then null else $ts_time end) },
            note: (if $note == "" then null else $note end)
        }'
else
  row() { printf '%-12s %s\n' "$1" "$2"; }
  row file "$file"
  row sha256 "$file_sha"
  row checked_at "$checked_at"
  row c2patool "$c2pa_ver"
  row trust "$trust_id"
  echo
  row validation "$state"
  row failures "${failures:-none}"
  row signer "$subject"
  row serial "$serial_hex (decimal $serial_dec)"
  row not_before "$not_before"
  row not_after "$not_after ($window the window at checked_at)"
  row time_stamp "$ts_status${ts_codes:+ [$ts_codes]}"
  [ -z "$ts_time" ] || row ts_time "$ts_time (as stated by the token)"
  if [ -n "$note" ]; then
    echo
    row note "$note"
  fi
fi
