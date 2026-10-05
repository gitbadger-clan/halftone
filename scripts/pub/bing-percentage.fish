#!/usr/bin/env fish
# Frame for the Bing post: Microsoft's soft-binding region unit and c2pa's decode error.
#
#   fish scripts/pub/bing-percentage.fish          # print the frame
#   fish scripts/pub/bing-percentage.fish --png    # render scripts/pub/out/bing-percentage.png
#
# Requires: cargo, rg; freeze for --png

set -l self (realpath (status filename))
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l f corpus/differential/04-generators/bing-image__mai-image-2.5-flash__web-download__p1__1.jpg
set -l out scripts/pub/out/bing-percentage.png

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

test -f $f; or begin
    echo "missing $f" >&2
    exit 1
end
echo (set_color brblack)"# Bing Image Creator download, c2pa-rs 0.91.1  "(date -u +%Y-%m-%d)(set_color normal)
cargo run -q --manifest-path tools/d011-softbinding/Cargo.toml -- $f \
    | rg -v '^\s*(control|#2689)' \
    | rg --passthru --color=always percentage

set -l n 0
set -l bad 0
for x in corpus/differential/04-generators/bing-image__*__web-download__*.jpg
    set n (math $n + 1)
    set -l o (cargo run -q --manifest-path tools/d011-softbinding/Cargo.toml -- $x)
    string match -q -- '*FAILS: unknown variant `percentage`*' $o; and set bad (math $bad + 1)
end
echo
echo (set_color brblack)"# $bad of $n Bing downloads fail the same way"(set_color normal)
