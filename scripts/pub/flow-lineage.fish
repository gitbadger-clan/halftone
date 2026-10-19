#!/usr/bin/env fish
# Frame for the D-013 post: two downloads of the same Google Flow edit. In the
# first, the active manifest records only a re-encode and the generative step
# sits one parentOf layer down; in the re-download, the generative manifest is
# on top. Halftone's declares_ai_active is the old top-manifest-only answer,
# declares_ai the lineage-aware one, so one run shows before and after.
#
#   fish scripts/pub/flow-lineage.fish          # print the frame
#   fish scripts/pub/flow-lineage.fish --png    # render scripts/pub/out/flow-lineage.png
#
# Requires: ht (with the D-013 lineage fix), c2patool, jq; freeze for --png
#   brew install charmbracelet/tap/freeze

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l g corpus/differential/04-generators
set -l out scripts/pub/out/flow-lineage.png
set -l rows \
    "first download:google-flow__nano-banana-2.1__edit-photo__e2__2.jpg" \
    "re-download, same edit:google-flow__nano-banana-2.1__edit-photo-redownload__e2__2.jpg"

# --- render mode: re-run this script under freeze and stop ------------------
if contains -- --png $argv
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
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
for tool in ht c2patool jq
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end

# Walk the manifest chain from the active manifest down through ingredients that
# carry their own manifest: relationship, signer, actions (with source type).
# Embedded manifests only, so c2patool never fetches anything here.
set -l chain_jq '
  .active_manifest as $a | .manifests as $m
  | def step($label; $depth; $rel):
      $m[$label] as $x
      | ("  " * $depth) + ($rel | . + (" " * (9 - length)))
        + ([$x.assertions[]? | select(.label | startswith("c2pa.actions")) | .data.actions[]
            | .action + (if .digitalSourceType then " (" + (.digitalSourceType | split("/") | last) + ")" else "" end)
                      + (if .action == "c2pa.edited" and .description then " \"" + .description + "\"" else "" end)]
           | join(", "))
        + "  [" + ($x.signature_info.issuer // "-") + "]",
        ($x.ingredients[]? | select(.active_manifest and $m[.active_manifest])
          | step(.active_manifest; $depth + 1; .relationship));
    step($a; 0; "active")'

set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l R (set_color normal)

echo "$D# One Google Flow edit, downloaded twice, "(date -u '+%Y-%m-%d')$R

for row in $rows
    set -l kv (string split : $row)
    set -l f $g/$kv[2]
    test -f $f; or begin
        echo "missing $f" >&2
        exit 1
    end

    echo
    echo "$H== $kv[1]$R  $D$kv[2]$R"
    c2patool $f 2>/dev/null | jq -r $chain_jq
    echo
    ht inspect --json $f 2>/dev/null | jq -r '.evidence[] | select(.source.name=="c2pa") | .details
        | "top manifest only:  declares_ai_active = \(.declares_ai_active)",
          "whole chain:        declares_ai        = \(.declares_ai)   generative_depth = \(.generative_depth)"'
end

echo
echo "$D# active = the manifest a top-only reader sees; indented lines are its ingredients' manifests$R"
