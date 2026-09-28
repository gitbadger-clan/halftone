#!/bin/sh
# Runs inside the halftone-offline container (docker --network none). Output goes to
# the run log written by scripts/offline-docker.fish.
#
#   controls   strace records a connect() here, and wraps ht itself; else stop
#   connect    every connect() of ht, c2patool 0.27 and 0.28 over the corpus
#   ocsp       files where c2patool 0.27 reports ocsp.notRevoked with no network,
#              with stapled-OCSP markers and what 0.28 reports for the same file
#
# Exit: 0 ok · 1 a control failed (nothing below means anything) · 3 ht tried the network
set -u
cd /work

python3 -c 'import sys; sys.path.insert(0, "scripts"); import versiondiff; print(versiondiff.settings_toml("legacy"))' >/tmp/legacy.toml
python3 -c 'import sys; sys.path.insert(0, "scripts"); import versiondiff; print(versiondiff.settings_toml("typed"))' >/tmp/typed.toml
mkdir -p /tmp/empty
export XDG_CONFIG_HOME=/tmp/empty
unset C2PATOOL_SETTINGS C2PATOOL_TRUST_ANCHORS 2>/dev/null || true
C27="/opt/c2patool-0.27/bin/c2patool --settings /tmp/legacy.toml"
C28="/opt/c2patool-0.28/bin/c2patool --settings /tmp/typed.toml"

cat >/tmp/codes.py <<'PY'
import json, sys
try:
    j = json.load(sys.stdin)
except Exception:
    print("ERR"); sys.exit()
def walk(o, key):
    if isinstance(o, dict):
        for k, v in o.items():
            if k == key and isinstance(v, list):
                yield from (e.get("code") for e in v if isinstance(e, dict))
            else:
                yield from walk(v, key)
    elif isinstance(o, list):
        for v in o:
            yield from walk(v, key)
print(",".join(sorted({c for c in walk(j, sys.argv[1]) if c})))
PY

echo "## environment"
echo "kernel:        $(uname -srm)"
echo "ht:            $(ht --version)"
echo "c2patool 0.27: $(/opt/c2patool-0.27/bin/c2patool --version)"
echo "c2patool 0.28: $(/opt/c2patool-0.28/bin/c2patool --version)"

files=$(find corpus/differential/0*/ target/conformance-v22/2.2 -type f \
  ! -name '*.json' ! -name '*.txt' ! -name '*.md' ! -name '.*' | sort)
echo "files:         $(echo "$files" | wc -l)"
probe=$(echo "$files" | head -1)

echo
echo "## controls"
strace -f -qq -e trace=connect -o /tmp/ctl python3 -c '
import socket
try:
    socket.create_connection(("1.1.1.1", 443), 3)
except OSError:
    pass'
n=$(grep -c 'AF_INET' /tmp/ctl || true)
if [ "$n" -lt 1 ]; then
  echo "FAIL strace did not record a deliberate connect(); every count below would read 0"
  exit 1
fi
echo "ok   strace records connect() with no network ($n line(s))"
strace -f -qq -e trace=openat -o /tmp/ctl ht inspect --json "$probe" >/dev/null 2>&1
n=$(grep -c -F "$probe" /tmp/ctl || true)
if [ "$n" -lt 1 ]; then
  echo "FAIL strace did not see ht open its input ($probe)"
  exit 1
fi
echo "ok   strace wraps ht itself (opened $probe)"

echo
echo "## connect() attempts to an IP address, no network"
ht_attempts=0
for tool in ht c2patool-0.27 c2patool-0.28; do
  case $tool in
  ht) cmd="ht inspect --json" ;;
  c2patool-0.27) cmd=$C27 ;;
  c2patool-0.28) cmd=$C28 ;;
  esac
  out=target/versiondiff/connect-$tool.txt
  : >"$out"
  for f in $files; do
    strace -f -qq -e trace=connect -o /tmp/t $cmd "$f" >/dev/null 2>&1
    grep -E 'AF_INET6?' /tmp/t | sed "s|^|$f: |" >>"$out"
  done
  n=$(wc -l <"$out")
  nf=$(cut -d: -f1 "$out" | sort -u | grep -c . || true)
  echo "$tool: $n attempts from $nf files -> $out"
  head -3 "$out" | cut -c1-220 | sed 's/^/  /'
  [ "$tool" = ht ] && ht_attempts=$n
done

echo
echo "## OCSP: c2patool 0.27 reports ocsp.notRevoked with no network"
echo "stapled = markers found in the file bytes (rVals, ocspVals, c2pa.certificate-status)"
total=0
still=0
for f in $files; do
  i27=$($C27 "$f" 2>/dev/null | python3 /tmp/codes.py informational)
  case ",$i27," in *,signingCredential.ocsp.notRevoked,*) ;; *) continue ;; esac
  j28=$($C28 "$f" 2>/dev/null)
  i28=$(echo "$j28" | python3 /tmp/codes.py informational)
  s28=$(echo "$j28" | python3 /tmp/codes.py success | tr ',' '\n' | grep -i ocsp | paste -sd, -)
  marks=0
  for m in rVals ocspVals c2pa.certificate-status; do
    grep -q -a -F "$m" "$f" && marks=$((marks + 1))
  done
  total=$((total + 1))
  case ",$i28,$s28," in *,signingCredential.ocsp.notRevoked,*) still=$((still + 1)) ;; esac
  echo "$f"
  echo "  stapled markers: $marks"
  echo "  0.27 info: $i27"
  echo "  0.28 info: ${i28:-none}"
  echo "  0.28 success (ocsp): ${s28:-none}"
done

echo
echo "## summary"
echo "ht connect attempts:                         $ht_attempts"
echo "files with ocsp.notRevoked offline on 0.27:  $total"
echo "  of those, notRevoked on 0.28 (any bucket): $still"
[ "$ht_attempts" -eq 0 ] || exit 3
