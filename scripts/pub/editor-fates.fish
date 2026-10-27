#!/usr/bin/env fish
# Frame for the seven-editor post: one signed camera photo (00-base photo_bowl),
# one prompt (E1), every editor in 04-generators. Each cell is computed from the
# files, never typed: what happened to the camera's signed record, and which
# GenAI digitalSourceType the editor declared.
#
#   fish scripts/pub/editor-fates.fish                  # print the frame (offline)
#   fish scripts/pub/editor-fates.fish --fetch          # also read remote manifests (Firefly)
#   fish scripts/pub/editor-fates.fish --fetch --png    # render scripts/pub/out/editor-fates.png
#
# A row covers every E1 file of that editor (all models and variants). If the
# files of one row disagree, the script prints each file's result and stops:
# the table only states what all of them agree on. --png refuses to render
# while any cell is unverified (a remote manifest that was not fetched).
#
# Requires: c2patool, jq, exiftool; freeze for --png

argparse fetch png -- $argv; or exit 2

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l c corpus/differential/04-generators
set -l base corpus/differential/00-base/photo_bowl.jpg
set -l out scripts/pub/out/editor-fates.png
set -l prompt "Replace the green apples in the bowl with lemons"

# editor|row label|glob[,glob]  (globs match file names in $c; E1 only).
# One editor can need several rows: Firefly's two Fill models disagree.
set -l rows \
    "firefly|Adobe Firefly, Fill|adobe-firefly__firefly-fill__edit-photo__e1__*" \
    "firefly|Adobe Firefly, Image 4 Ultra Fill|adobe-firefly__firefly-image-4-ultra-fill__edit-photo__e1__*" \
    "flow|Google Flow|google-flow__*__edit-photo__e1__*" \
    "gemini|Gemini (app, API)|gemini-app__*__edit-photo__e1__*,gemini-api__*__e1__*" \
    "canva|Canva|canva__*__edit-photo__e1__*" \
    "ideogram|Ideogram|ideogram__*__edit-photo__e1__*" \
    "openai|ChatGPT (web, API)|chatgpt__*__edit-photo__e1__*,openai-api__*__e1__*" \
    "meta|Meta AI|meta-ai__*__edit-photo__e1__*"
# Not edits: Flow's upload-only control shares the E1 slot.
set -l skip '*__upload-unedited__*'

# Under freeze the script runs again without arguments; --fetch reaches it here.
set -q EDITOR_FATES_FETCH; and set _flag_fetch 1

# --- render mode: check the frame first, then re-run it under freeze ---------
if set -q _flag_png
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
    set -l fetchargs
    set -q _flag_fetch; and set fetchargs --fetch
    set -l frame (fish $self $fetchargs | string collect); or begin
        echo "the frame failed; nothing rendered" >&2
        exit 1
    end
    if string match -q '*not fetched*' -- $frame
        echo "a remote manifest was not fetched; add --fetch before rendering" >&2
        exit 1
    end
    set -q _flag_fetch; and set -gx EDITOR_FATES_FETCH 1
    mkdir -p (dirname $out)
    freeze --execute "fish $self" \
        --execute.timeout 120s \
        --output $out \
        --background '#1c1e26' \
        --font.size 15 \
        --line-height 1.35 \
        --wrap 150 \
        --padding 24,32 \
        --margin 0 \
        --border.radius 8 \
        --window
    or begin
        echo "freeze failed, $out not written" >&2
        exit 1
    end
    echo "wrote $out"
    exit 0
end

# --- frame ------------------------------------------------------------------
for tool in c2patool jq exiftool
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end
test -f $base; or begin
    echo "missing $base" >&2
    exit 1
end

# c2patool with no user settings; remote fetching only when asked, OCSP never.
set -l tmp (mktemp -d)
mkdir -p $tmp/xdg
printf '[verify]\nremote_manifest_fetch = false\nocsp_fetch = false\n' >$tmp/offline.toml
printf '[verify]\nremote_manifest_fetch = true\nocsp_fetch = false\n' >$tmp/fetch.toml
function __c2p --inherit-variable tmp
    set -l mode $argv[1]
    env -u C2PATOOL_SETTINGS -u C2PATOOL_TRUST_ANCHORS XDG_CONFIG_HOME=$tmp/xdg \
        c2patool --settings $tmp/$mode.toml $argv[2..] 2>/dev/null
end

set -l cam (__c2p offline $base | jq -r '.active_manifest // empty')
test -n "$cam"; or begin
    echo "no manifest in $base; cannot identify the camera record" >&2
    rm -rf $tmp
    exit 1
end

# Per file: every ingredient path from the active manifest to the camera's
# record, and the GenAI source types declared anywhere in the chain.
set -l chain_jq '
  def dsts: [.assertions[]? | select(.label | startswith("c2pa.actions")) | .data.actions[]?
             | .digitalSourceType // empty | split("/") | last];
  .active_manifest as $a | .manifests as $ms
  | def paths($l; $rels; $seen):
      if $l == $cam then $rels
      elif ($seen | index([$l])) then empty
      else $ms[$l].ingredients[]? | select(.active_manifest and $ms[.active_manifest])
           | paths(.active_manifest; $rels + [.relationship // "componentOf"]; $seen + [$l])
      end;
    [paths($a; []; [])] as $p
  | (if ($p | length) == 0 then "none"
     elif any($p[][]; . == "inputTo") then "input"
     else "parent" end),
    ([$ms[] | dsts[] | select(test("(?i)trainedalgorithmic"))] | unique | join(" "))'

function __term
    switch $argv[1]
        case trainedAlgorithmicMedia
            echo "created by AI"
        case compositeWithTrainedAlgorithmicMedia
            echo "composite with AI"
        case '*'
            echo $argv[1]
    end
end

# One file -> "camera cell|label cell"
function __fate --inherit-variable chain_jq --inherit-variable cam
    set -l f $argv[1]
    set -l fetch $argv[2]
    set -l xmp (exiftool -s3 -XMP-iptcExt:DigitalSourceType $f | string replace -r '.*/' '')
    set -l js (__c2p offline $f | string collect)
    set -l where "in the file"
    if not printf '%s' $js | jq -e '.manifests | length > 0' >/dev/null 2>&1
        set -l remote (exiftool -s3 -XMP-dcterms:Provenance $f)
        if test -z "$remote"
            set -l label "none declared"
            test -n "$xmp"; and set label (__term $xmp)" (unsigned XMP only)"
            echo "dropped|$label"
            return
        end
        if test "$fetch" != 1
            echo "linked remotely, not fetched|not fetched"
            return
        end
        set js (__c2p fetch $f | string collect)
        printf '%s' $js | jq -e '.manifests | length > 0' >/dev/null 2>&1; or begin
            echo "remote fetch failed|not fetched"
            return
        end
        set where "at "(string replace -r '^https?://([^/]+).*' '$1' -- $remote)
    end
    set -l r (printf '%s' $js | jq -r --arg cam $cam $chain_jq)
    set -l labels
    for t in (string split -n ' ' -- $r[2])
        set -a labels (__term $t)
    end
    set -l label (string join ' + ' -- $labels)
    test -z "$label"; and set label "none declared"
    set -l verb kept
    test "$where" != "in the file"; and set verb linked
    switch $r[1]
        case none
            echo "replaced|$label"
        case input
            echo "$verb $where, as an input|$label"
        case parent
            echo "$verb $where, as the parent|$label"
    end
end

set -l fetch 0
set -q _flag_fetch; and set fetch 1
set -l names (path basename $c/*)
set -l col1 editor
set -l col2 "the camera's signed record"
set -l col3 "what the edit is declared as"
set -l col4 files
set -l failed 0
set -l claimed
set -l editors

for row in $rows
    set -l kv (string split -m2 '|' -- $row)
    set -l files
    for g in (string split , -- $kv[3])
        for n in (string match -- $g $names)
            string match -q -- $skip $n; and continue
            set -a files $n
        end
    end
    set -a claimed $files
    contains -- $kv[1] $editors; or set -a editors $kv[1]
    set kv $kv[2..3]
    if test (count $files) -eq 0
        echo "no E1 files for $kv[1] ($kv[2])" >&2
        set failed 1
        continue
    end
    set -l results
    for n in $files
        set -a results (__fate $c/$n $fetch)
    end
    if test (count (printf '%s\n' $results | sort -u)) -ne 1
        echo "$kv[1]: the files disagree, so the row would be a guess:" >&2
        for i in (seq (count $files))
            echo "  $files[$i]: $results[$i]" >&2
        end
        set failed 1
        continue
    end
    set -l cells (string split -m1 '|' -- $results[1])
    set -a col1 $kv[1]
    set -a col2 $cells[1]
    set -a col3 $cells[2]
    set -a col4 (count $files)
end
rm -rf $tmp
# Every E1 file must be in some row, so a new model can't silently drop out.
for n in (string match -- '*__e1__*' $names)
    string match -q -- $skip $n; and continue
    contains -- $n $claimed; and continue
    echo "E1 file in no row: $n" >&2
    set failed 1
end
test $failed = 0; or exit 1

set -l D (set_color brblack)
set -l H (set_color --bold brcyan)
set -l R (set_color normal)
set -l w1 (math 3 + (string length -- $col1 | sort -n | tail -1))
set -l w2 (math 3 + (string length -- $col2 | sort -n | tail -1))
set -l w3 (math 3 + (string length -- $col3 | sort -n | tail -1))

echo "$D# One signed Pixel photo, one prompt (\"$prompt\"), "(count $editors)" AI editors$R"
echo
for i in (seq (count $col1))
    set -l line (string pad -r -w $w1 -- $col1[$i])(string pad -r -w $w2 -- $col2[$i])(string pad -r -w $w3 -- $col3[$i])$col4[$i]
    if test $i -eq 1
        echo "$H$line$R"
    else
        echo $line
    end
end
echo
echo "$D# Every E1 file per editor, all models; a row is shown only if its files agree. \"declared as\" = the GenAI$R"
echo "$D# digitalSourceType in the signed records (XMP only where there are none). Validity not shown. Rendered "(date -u +%Y-%m-%d)"$R"
