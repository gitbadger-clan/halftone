#!/usr/bin/env fish
# Frame for the Canva post: a generation and its generative inpaint carry the same manifest.
#
#   fish scripts/pub/canva-label.fish          # print the frame
#   fish scripts/pub/canva-label.fish --png    # render scripts/pub/out/canva-label.png
#
# Requires: c2patool, jq; freeze for --png

set -l self (realpath (status filename))
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l g corpus/differential/04-generators
set -l out scripts/pub/out/canva-label.png

if contains -- --png $argv
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
    mkdir -p (dirname $out)
    freeze --execute "fish $self" \
        --output $out \
        --background '#1c1e26' \
        --font.size 15 \
        --line-height 1.35 \
        --wrap 100 \
        --padding 24,32 \
        --margin 0 \
        --border.radius 8 \
        --window
    echo "wrote $out"
    exit 0
end

for tool in c2patool jq
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end

# The manifest's shape: ingredient count and each action with its source type.
function shape
    c2patool $argv[1] --detailed 2>/dev/null | jq -c '.manifests[]
        | {ingredients: (.ingredients | length),
           actions: [.assertion_store["c2pa.actions.v2"].actions[]?
                     | {action, digitalSourceType: ((.digitalSourceType // "") | split("/")[-1])}]}'
end

set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l R (set_color normal)

echo "$D# Canva AI, one generation before and after Magic Edit  "(date -u +%Y-%m-%d)$R
for row in "generation p2:canva__canva-ai__web-download__p2__2.png" \
    "Magic Edit (generative inpaint) of p2:canva__canva-ai-magic-edit__web-download__p2__2.png"
    set -l kv (string split -m1 : $row)
    echo
    echo "$H== $kv[1]$R  $D$kv[2]$R"
    shape $g/$kv[2] | jq .
end

set -l ref (shape $g/canva__canva-ai__web-download__p2__2.png)
set -l n 0
set -l same 0
for f in $g/canva__*
    set -l s (shape $f)
    test -n "$s"; or continue # the free-tier file has no manifest
    set n (math $n + 1)
    test "$s" = "$ref"; and set same (math $same + 1)
end
echo
echo "$D# $same of $n signed Canva exports carry this exact manifest shape$R"

set -l db (magick compare -metric PSNR $g/canva__canva-ai__web-download__p2__2.png \
    $g/canva__canva-ai-magic-edit__web-download__p2__2.png null: 2>&1 | string split ' ')[1]
echo
echo "$D# pixels: generation vs Magic Edit, PSNR $db dB (the edit changed the image)$R"
