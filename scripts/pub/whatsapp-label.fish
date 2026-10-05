#!/usr/bin/env fish
# Frame for the WhatsApp post: one assistant, three delivery surfaces. Meta AI's
# web download and Android app save carry Meta's XMP label; the images Meta AI
# delivers inside WhatsApp carry nothing.
#
#   fish scripts/pub/whatsapp-label.fish          # print the frame
#   fish scripts/pub/whatsapp-label.fish --png    # render scripts/pub/out/whatsapp-label.png
#
# Requires: ht, exiftool, jq; freeze for --png
#   brew install charmbracelet/tap/freeze

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l g corpus/differential/04-generators
set -l out scripts/pub/out/whatsapp-label.png

# label:file. Different generations on each surface, so sizes are not before/after.
set -l rows \
    "web download:meta-ai__instant__web-download-thumb__p1__1.jpg" \
    "Android app save:meta-ai__instant__app-save-open__p1__2.webp" \
    "inside WhatsApp:meta-ai__base__whatsapp-save__p1__1.jpg" \
    "inside WhatsApp:meta-ai__base__whatsapp-save__p5__5.jpg"

# --- render mode ------------------------------------------------------------
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

# --- frame ------------------------------------------------------------------
for tool in ht exiftool jq
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end
for row in $rows
    set -l kv (string split -m1 : $row)
    test -f $g/$kv[2]; or begin
        echo "missing $g/$kv[2]" >&2
        exit 1
    end
end

set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l P (set_color green)
set -l A (set_color yellow)
set -l R (set_color normal)

echo "$D# Meta AI, three surfaces  "(date -u +%Y-%m-%d)$R
for row in $rows
    set -l kv (string split -m1 : $row)
    set -l f $g/$kv[2]
    echo
    echo "$H== $kv[1]$R  $D$kv[2]  "(stat -f %z $f)" B  sha256 "(shasum -a 256 $f | cut -c1-12)$R
    set -l dst (exiftool -s3 -XMP-iptcExt:DigitalSourceType $f | string replace -r '.*/' '')
    test -n "$dst"; or set dst "(none)"
    echo "   exiftool XMP DigitalSourceType: $dst"
    ht inspect --batch --json $f | jq -r '
        .inspections[0].evidence[]
        | select(.source.name | test("^(marking_metadata|c2pa|jpeg_quant)$"))
        | [.source.name, .status, (.rationale // "")] | @tsv' \
        | while read -l -d \t name verdict why
        set -l c $A
        test $verdict = present; and set c $P
        # The marking verdict is the claim: show its rationale in full.
        test $name = marking_metadata; or set why (string shorten -m 64 -- $why)
        printf '   %-17s %s%-8s%s %s\n' $name $c $verdict $R $why
    end
end
