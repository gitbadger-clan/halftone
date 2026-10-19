#!/usr/bin/env fish
# Frame for the Canva-vs-Flow post: the same signed camera photo (00-base
# photo_bowl, E1 "replace the apples with lemons"), edited by Canva and by
# Google Flow. Shows each file's manifest chain from the active manifest down.
# Canva's output has one manifest and no ingredients; Flow's chain reaches the
# camera capture. Structure only: validity is left out on purpose (Canva's
# October certificate has since expired, which is a separate finding).
#
#   fish scripts/pub/edit-chains.fish          # print the frame
#   fish scripts/pub/edit-chains.fish --png    # render scripts/pub/out/edit-chains.png
#
# Requires: c2patool, jq; freeze for --png
#   brew install charmbracelet/tap/freeze

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l c corpus/differential
set -l out scripts/pub/out/edit-chains.png
set -l rows \
    "the original, straight from the camera:00-base/photo_bowl.jpg" \
    "Canva, edited by prompt:04-generators/canva__canva-ai__edit-photo__e1__1.png" \
    "Google Flow, edited by prompt:04-generators/google-flow__nano-banana-2.1__edit-photo__e1__1.jpg"

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
for tool in c2patool jq
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

echo "$D# One signed photo, one prompt (\"Replace the green apples in the bowl with lemons\"), two editors, "(date -u '+%Y-%m-%d')$R

for row in $rows
    set -l kv (string split : $row)
    set -l f $c/$kv[2]
    test -f $f; or begin
        echo "missing $f" >&2
        exit 1
    end

    echo
    echo "$H== $kv[1]$R  $D$kv[2]$R"
    set -l json (c2patool $f 2>/dev/null | string collect)
    printf '%s' $json | jq -r $chain_jq
    printf '%s' $json | jq -r '
      [.manifests[] | .assertions[]? | select(.label | startswith("c2pa.actions")) | .data.actions[]
       | .digitalSourceType // empty | split("/") | last] as $dst
      | "\(.manifests | length) manifest(s) in the file; camera capture recorded: \(if ($dst | index("computationalCapture")) then "yes" else "no" end)"'
end

echo
echo "$D# active = the manifest on top; indented lines are the manifests of its ingredients. Validity not shown.$R"
