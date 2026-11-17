#!/usr/bin/env fish
# Frame for the identifiers post: what one Canva AI export carries besides the
# picture (its vendor AI flag and the account or tracking records next to it), and
# how many files per generator in 04-generators carry each kind of identifier.
#
#   fish scripts/pub/export-identifiers.fish          # print the frame
#   fish scripts/pub/export-identifiers.fish --png    # render scripts/pub/out/export-identifiers.png
#
# Values are never shown: the frame runs `ht inspect` without --reveal, so every
# identifier appears as its field name, kind and length only, and it stops if any
# identifier value is present in ht's output. Counts are computed at run time.
#
# Override the binary with an exported HT (set -x, so freeze's re-run sees it).
# Requires: ht, jq; freeze for --png
#   brew install charmbracelet/tap/freeze

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -q HT; or set -g HT ./target/release/ht
# Not with a binary older than the source: the frame would show the old code's
# output (scripts/lib/htbin.py). `HT_STALE_OK=1 <this script>` runs anyway. Checked
# here, before the render mode, so --png stops too and does not draw the error.
uv run scripts/ht-check.py -q --ht $HT; or exit 2

set -l out scripts/pub/out/export-identifiers.png

# --- render mode: re-run this script under freeze and stop ------------------
if contains -- --png $argv
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
    mkdir -p (dirname $out)
    freeze --execute "fish $self" \
        --execute.timeout 2m \
        --output $out \
        --background '#1c1e26' \
        --font.size 15 \
        --line-height 1.35 \
        --wrap 104 \
        --padding 24,32 \
        --margin 0 \
        --border.radius 8 \
        --window
    or exit $status
    echo "wrote $out"
    exit 0
end

# --- setup --------------------------------------------------------------------
set -l g corpus/differential/04-generators
set -l one canva__canva-ai__web-download__p1__2.png

function die
    echo "error: $argv" >&2
    exit 1
end

type -q jq; or die "jq not found on PATH"
test -x $HT; or die "ht not executable at $HT (cargo build --release -p halftone-cli --features c2pa, or set -x HT)"
test -f $g/$one; or die "missing $g/$one"

set -l files
for f in $g/*__*
    set -a files $f
end

set -l tmp (mktemp -d)
function cleanup --on-event fish_exit --inherit-variable tmp
    rm -rf $tmp
end

# The container layer only: these fields are read without the manifest.
# `ht inspect` exits 2 when any source reports Present (its CI convention), so 2 is
# a normal result here; anything else is a failure.
$HT inspect --batch --json --only container $files >$tmp/batch.json 2>$tmp/err.txt
set -l rc $status
set -l err (head -n 3 $tmp/err.txt | string join ' | ')
contains -- $rc 0 2; or die "ht inspect exited $rc: $err"
jq -e '.inspections | type == "array"' $tmp/batch.json >/dev/null 2>&1; or die "ht inspect did not print a batch document"
test (jq '.inspections | length' $tmp/batch.json) -eq (count $files); or die "ht did not report on every file"

# Never publish a value: stop if any identifier in the output is not masked.
set -l open (jq '[.inspections[].evidence[] | select(.source.name == "embedded_identifiers")
                  | .details.identifiers[]? | select(has("value"))] | length' $tmp/batch.json)
test "$open" = 0; or die "$open identifier values are not masked in ht's output: refusing to print"

# --- frame ------------------------------------------------------------------------
set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l R (set_color normal)

echo "$D# What an AI image export carries besides the picture, checked "(date -u '+%Y-%m-%d %H:%M')" UTC$R"
echo "$D# "($HT --version 2>&1 | string join ' ')"  ·  values masked (ht's default): field, kind and length only$R"

echo
echo "$H== one Canva AI export, as downloaded$R  $D$one$R"
jq -r --arg one $one '
    def pad(n): tostring as $s | $s + (if n > ($s | length) then " " * (n - ($s | length)) else "" end);
    .inspections[] | select((.asset.path | split("/") | last) == $one)
    | (.evidence | map({key: .source.name, value: .details}) | from_entries) as $d
    | ($d.vendor_ai_flag.flags // []) as $flags
    | ($d.embedded_identifiers.identifiers // []) as $ids
    | ([$ids[] | "\(.field) (\(.kind))" | length] | max // 0) as $w
    | (if ($flags | length) > 0 then "vendor AI flag" else "vendor AI flag: none" end),
      ($flags[] | "  \(.prefix):\(.field) = \(.value)" + (if .namespace then "   (namespace \(.namespace))" else "" end)),
      (if ($ids | length) > 0 then "identifiers" else "identifiers: none" end),
      ($ids[] | "  " + ("\(.field) (\(.kind))" | pad($w)) + "  [masked, \(.length) character" + (if .length == 1 then "" else "s" end) + "]")
' $tmp/batch.json

echo
echo "$H== 04-generators ("(count $files)" files): files per generator that carry each$R"
# Columns appear only for kinds found somewhere in the stratum.
jq -r '
    def pad(n): tostring as $s | $s + (if n > ($s | length) then " " * (n - ($s | length)) else "" end);
    [ ["account", "account or tracking"], ["person", "person name"], ["contact", "contact detail"],
      ["device", "device identifier"], ["document", "document identifier"], ["location", "location"] ] as $kinds
    | [ .inspections[]
        | (.evidence | map({key: .source.name, value: .}) | from_entries) as $e
        | { gen: (.asset.path | split("/") | last | split("__")[0]),
            flag: ($e.vendor_ai_flag.status == "present"),
            kinds: ([$e.embedded_identifiers.details.identifiers[]?.kind] | unique) } ] as $rows
    | [ $kinds[] | select(.[0] as $k | any($rows[]; .kinds | index($k))) ] as $shown
    | ($rows | group_by(.gen)
       | map({ gen: .[0].gen, files: length, flag: (map(select(.flag)) | length),
               per: [ . as $grp | $shown[] | .[0] as $k | ($grp | map(select(.kinds | index($k))) | length) ] })) as $gens
    | ($gens | map(select(.flag > 0 or (.per | add // 0) > 0))) as $with
    | ($gens | map(select(.flag == 0 and (.per | add // 0) == 0))) as $without
    | ("generator" | pad(18)) + ("files" | pad(7)) + ("vendor AI flag" | pad(16))
        + ([$shown[] | .[1] | pad(21)] | join("")),
      ($with[] | (.gen | pad(18)) + (.files | pad(7)) + (.flag | pad(16)) + ([.per[] | pad(21)] | join(""))),
      "",
      "# \($without | length) other generators, \($without | map(.files) | add // 0) files: none of these"
' $tmp/batch.json | while read -l line
    switch $line
        case 'generator*' '#*'
            echo "$D$line$R"
        case '*'
            echo $line
    end
end
echo "$D# unsigned metadata, read as written: this lists what a file discloses, not who put it there or why$R"
