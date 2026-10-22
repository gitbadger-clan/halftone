# edit-chain.fish: for edit rows, which base photo went in (if its record survived),
# how many C2PA records the file holds, and what each one states.
# Offline by default: c2patool runs with remote fetching off and no user settings.
# --fetch fetches remote manifests (e.g. Adobe's cai-manifests) and says so.
# usage: fish scripts/edit-chain.fish [--fetch] <files…>
argparse fetch -- $argv; or exit 1
set -l fetch false
set -q _flag_fetch; and set fetch true

set -l tmp (mktemp -d)
printf '[verify]\nremote_manifest_fetch = %s\nocsp_fetch = false\n' $fetch >$tmp/c2pa.toml
mkdir -p $tmp/xdg
function __c2p --inherit-variable tmp
    env -u C2PATOOL_SETTINGS -u C2PATOOL_TRUST_ANCHORS XDG_CONFIG_HOME=$tmp/xdg \
        c2patool --settings $tmp/c2pa.toml $argv 2>/dev/null
end
test $fetch = true; and echo "note: fetching remote manifests over the network"

set -l base (git rev-parse --show-toplevel)/corpus/differential/00-base
set -l ids
for p in $base/photo_*.jpg
    set -a ids (__c2p $p | jq -r '.active_manifest[-12:]')":"(basename $p .jpg)
end
for f in $argv
    set -l dst (exiftool -s3 -XMP-iptcExt:DigitalSourceType $f | string replace -r '.*/' '')
    echo "== "(basename $f)"  size=$(exiftool -s3 -ImageSize $f)  xmp-dst=$dst  make=$(exiftool -s3 -Make $f)"
    set -l js (__c2p $f | string collect)
    if test -z "$js"; or not printf '%s' $js | jq -e '.manifests' >/dev/null 2>&1
        set -l remote (exiftool -s3 -XMP-dcterms:Provenance $f)
        if test -n "$remote"
            echo "  no embedded manifest; remote reference: $remote"
            test $fetch = false; and echo "  (run with --fetch to read it)"
        else
            echo "  no C2PA manifest"
        end
        continue
    end
    set -l keys (printf '%s' $js | jq -r '.manifests | keys[] | .[-12:]')
    set -l found none
    for pair in $ids
        set -l kv (string split : -- $pair)
        contains -- $kv[1] $keys; and set found $kv[2]
    end
    echo "  records: "(count $keys)"   base photo in chain: $found"
    printf '%s' $js | jq -r '
      .active_manifest as $a
      | .manifests | to_entries[]
      | "  \(.key[-12:])\(if .key == $a then " (active)" else "" end)  signer=\(.value.signature_info.issuer // "-")",
        (.value.assertions[]? | select(.label | startswith("c2pa.actions")) | .data.actions[]
          | "      \(.action)  dst=\((.digitalSourceType // "-") | split("/") | last)  \(.description // "")"),
        (.value.ingredients[]? | "      ingredient \(.relationship // "componentOf") -> \((.active_manifest // "none")[-12:])")'
end
rm -rf $tmp
