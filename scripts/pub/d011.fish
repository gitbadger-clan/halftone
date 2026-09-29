#!/usr/bin/env fish
# D-011 media: what validating against zero trust anchors does to the corpus.
#
#   fish scripts/pub/d011.fish                 # count + one-file block, printed
#   fish scripts/pub/d011.fish --png           # also render both blocks with freeze
#   fish scripts/pub/d011.fish --repro [--png] # also run tools/d011-repro (the with_value no-op)
#   fish scripts/pub/d011.fish --file PATH     # use this file for the one-file block
#
# The bug is reproduced with the reference tool, not re-created from memory: c2patool
# 0.28 (c2pa 0.91) runs every corpus file twice, once with the vendored anchors (what
# Halftone does now) and once with none (what Halftone did after the bump). Same
# files, same library, only the anchors differ.
#
# Outputs (scripts/pub/out/):
#   d011-flips.tsv   file, signer, state with anchors, state with zero anchors
#   d011-flips.png   flipped signers by name, all others as one row, fail-closed check (--png)
#   d011-file.png    one Google file: zero anchors / anchors / ht HEAD    (--png)
#   d011-repro.png   cargo test output of the minimal repro               (--repro --png)

set -l root (git rev-parse --show-toplevel 2>/dev/null; or pwd)
set root (path resolve -- $root) # physical path, same form path resolve gives $pick
set -l png 0
set -l repro 0
set -l c28 ~/.local/c2patool-0.28/bin/c2patool
set -l ht ""
set -l pick ""
while set -q argv[1]
    switch $argv[1]
        case --png
            set png 1
        case --repro
            set repro 1
        case --c2patool
            set c28 $argv[2]
            set -e argv[1]
        case --ht
            set ht $argv[2]
            set -e argv[1]
        case --file
            set pick $argv[2]
            set -e argv[1]
        case '*'
            echo "unknown argument: $argv[1]" >&2
            exit 2
    end
    set -e argv[1]
end
if test -n "$pick"
    test -f "$pick"; or begin
        echo "--file: no such file: $pick" >&2
        exit 2
    end
    set -l abs (path resolve -- $pick)
    # The .tsv lists repo-relative paths. A file outside the repo stays absolute
    # and gets the "not a Google-signed file that flipped" warning, which is correct.
    if string match -q -- "$root/*" $abs
        set pick (string replace -- "$root/" '' $abs)
    else
        set pick $abs
    end
end
cd $root
# Halftone is built from this checkout unless --ht is given: a stale `ht` on PATH
# reproduced the very bug this post describes (2026-09-28).
set -l commit (git rev-parse --short HEAD)
test -n "$(git status --porcelain -- crates Cargo.toml Cargo.lock)"; and set commit $commit-dirty
if test -z "$ht"
    cargo build -q --release -p halftone-cli --features c2pa; or exit 1
    set ht target/release/ht
    set -g ht_label "ht built from $commit"
else
    set -g ht_label "ht at $ht (not built here)"
end
for tool in $c28 $ht jq uv
    type -q $tool; or begin
        echo "$tool not found" >&2
        exit 2
    end
end
if test $png = 1; and not type -q freeze
    echo "freeze not found on PATH (brew install charmbracelet/tap/freeze)" >&2
    exit 2
end
mkdir -p scripts/pub/out
set -l out scripts/pub/out

# Settings: typed anchors exactly as versiondiff/differential use them, and a zero-anchor
# file that differs only by the missing [trust] section. Isolated env (D-008 update).
set -l tmp (mktemp -d)
uv run python -c "import sys; sys.path.insert(0, 'scripts'); import versiondiff; print(versiondiff.settings_toml('typed'))" >$tmp/anchors.toml
or exit 1
printf '[verify]\nremote_manifest_fetch = false\nocsp_fetch = false\nverify_trust = true\n' >$tmp/none.toml
mkdir -p $tmp/empty
set -e C2PATOOL_SETTINGS C2PATOOL_TRUST_ANCHORS
set -gx XDG_CONFIG_HOME $tmp/empty

function c2 -a exe settings file
    # state and signer; "none" when there is no manifest or c2patool refuses the file
    set -l r ($exe --settings $settings $file 2>/dev/null \
        | jq -r '[.validation_state // "none", (.manifests[.active_manifest].signature_info.issuer // "none")] | @tsv' 2>/dev/null)
    if test -n "$r"
        echo $r
    else
        printf 'none\tnone\n'
    end
end

# 1. every file, twice
set -l tsv $out/d011-flips.tsv
printf 'file\tsigner\twith_anchors\tzero_anchors\n' >$tsv
for f in (string match -rv '\.(json|txt|md|toml)$|/\.' corpus/differential/0*/**)
    test -f $f; or continue
    set -l a (c2 $c28 $tmp/anchors.toml $f | string split \t)
    set -l z (c2 $c28 $tmp/none.toml $f | string split \t)
    test "$a[1]" = none; and test "$z[1]" = none; and continue
    printf '%s\t%s\t%s\t%s\n' $f $a[2] $a[1] $z[1] >>$tsv
end

# 2. the counts block
set -l dim (printf '\e[90m')
set -l acc (printf '\e[36m')
set -l rst (printf '\e[0m')
set -l b1 $tmp/flips.txt
begin
    echo "$dim# same files, same library (c2pa 0.91 via c2patool 0.28), only the trust anchors differ$rst"
    printf '%-34s %6s %18s %18s\n' signer files "Trusted, anchors" "Trusted, zero"
    # Signers whose files flipped are named; every other signer is folded into one row,
    # so the image makes no claim about any other vendor. The .tsv keeps every signer.
    # Names contain spaces: emit sort keys first, sort, then format.
    tail -n +2 $tsv | awk -F'\t' '
        { n[$2]++; if ($3=="Trusted") a[$2]++; if ($4=="Trusted") z[$2]++
          if ($3=="Trusted" && $4=="Valid") fl[$2]++ }
        END {
          for (s in n) {
            if (fl[s] > 0) printf "1\t%d\t%s\t%d\t%d\n", n[s], s, a[s]+0, z[s]+0
            else { on += n[s]; oa += a[s]; oz += z[s]; k++ }
          }
          if (k) printf "0\t%d\tall other signers (%d)\t%d\t%d\n", on, k, oa, oz
        }' \
        | sort -t (printf '\t') -k1,1nr -k2,2nr \
        | awk -F'\t' '{ printf "%-34s %6d %18d %18d\n", substr($3,1,34), $2, $4, $5 }'
    set -l flips (tail -n +2 $tsv | awk -F'\t' '$3=="Trusted" && $4=="Valid"' | count)
    set -l zt (tail -n +2 $tsv | awk -F'\t' '$4=="Trusted"' | count)
    set -l other (tail -n +2 $tsv | awk -F'\t' '$3!=$4 && !($3=="Trusted" && $4=="Valid")' | count)
    echo
    echo "$acc Trusted → Valid with zero anchors:$rst $flips files"
    echo "$acc Trusted with zero anchors (fail-closed check, want 0):$rst $zt"
    echo "$acc any other state change (want 0):$rst $other"
end >$b1

# 3. one Google file, three commands (the flip, the fix, Halftone today)
# Default pick: a flipped Google-signed file whose name says Google. Aggregators
# resell Google-signed images under their own names and sort first, and an
# "aggregator-…" path under "# one Google-signed file" invites the wrong question
# (2026-09-28). Falls back to any flipped Google-signed file.
set -l flipped (tail -n +2 $tsv | awk -F'\t' '$2 ~ /Google/ && $3=="Trusted" && $4=="Valid" {print $1}')
set -l g $pick
if test -z "$g"
    set -l named (string match -r '.*/google[^/]*$' -- $flipped)
    set g $named[1]
    test -n "$g"; or set g $flipped[1]
else if not contains -- $g $flipped
    echo "warning: --file $g is not a Google-signed file that flipped; the block's heading will be wrong" >&2
end
set -l b2 $tmp/file.txt
if test -n "$g"
    begin
        echo "$dim# one Google-signed file$rst"
        echo "$acc❯$rst c2patool --settings zero-anchors.toml $g | jq -r .validation_state"
        $c28 --settings $tmp/none.toml $g 2>/dev/null | jq -r .validation_state
        echo "$acc❯$rst c2patool --settings anchors.toml $g | jq -r .validation_state"
        $c28 --settings $tmp/anchors.toml $g 2>/dev/null | jq -r .validation_state
        echo "$acc❯$rst ht inspect --json $g | jq … c2pa validation_state   $dim# $ht_label$rst"
        $ht inspect --json $g | jq -r '.evidence[] | select(.source.name=="c2pa") | .details.validation_state'
    end >$b2
end

cat $b1
echo
test -f $b2; and cat $b2
echo
echo "record -> $tsv"

# 4. the minimal repro of the no-op itself
if test $repro = 1
    set -l b3 $tmp/repro.txt
    begin
        echo "$acc❯$rst cargo test --manifest-path tools/d011-repro/Cargo.toml"
        cargo test -q --manifest-path tools/d011-repro/Cargo.toml 2>&1 \
            | string match -rv '^\s*(Compiling|Downloaded|Downloading|Locking|Updating|Adding)\b'
    end >$b3
    echo
    cat $b3
end

if test $png = 1
    set -l opts --theme dracula --window --padding 20,28 --font.family "JetBrains Mono" --font.size 14
    freeze --execute "cat $b1" --output $out/d011-flips.png $opts; and echo "image -> $out/d011-flips.png"
    test -f $b2; and freeze --execute "cat $b2" --output $out/d011-file.png $opts; and echo "image -> $out/d011-file.png"
    test $repro = 1; and freeze --execute "cat $tmp/repro.txt" --output $out/d011-repro.png $opts; and echo "image -> $out/d011-repro.png"
end
rm -rf $tmp
