#!/usr/bin/env fish
# Frame for the "four verdicts, no score" LinkedIn post: every source's verdict
# and rationale for the expired-certificate Canva file. No score anywhere.
#
#   fish scripts/pub/four-verdicts.fish          # print the frame
#   fish scripts/pub/four-verdicts.fish --png    # render scripts/pub/out/four-verdicts.png
#
# Rows are unedited ht output. Lines starting with # are captions added here.
#
# Requires: ht, jq, fold; freeze for --png

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l f corpus/differential/04-generators/canva__canva-ai__web-download__p1__2.png
set -l out scripts/pub/out/four-verdicts-(date -u +%Y-%m-%d).png
set -l width 64 # rationale wrap; keeps the image portrait for the LinkedIn feed

# --- render mode ------------------------------------------------------------
if contains -- --png $argv
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
    mkdir -p (dirname $out)
    freeze --execute "fish $self" \
        --output $out \
        --background '#1B3A5C' \
        --font.size 16 \
        --line-height 1.4 \
        --padding 36,44 \
        --margin 0 \
        --border.radius 8 \
        --window
    echo "wrote $out"
    exit 0
end

# --- frame ------------------------------------------------------------------
for tool in ht jq fold
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end
test -f $f; or begin
    echo "missing $f" >&2
    exit 1
end

set -l B (set_color --bold 9CC0E8) # brand light cyanotype: glyph + verdict
set -l S (set_color 9CC0E8) # source name
set -l D (set_color brblack) # captions
set -l R (set_color normal)

echo "$D# "(basename $f)"  ·  evaluated "(date -u +%Y-%m-%d)$R
echo

# Same filter as the CLI's text view: drop sources that don't apply to the format.
# ht exits 2 when any source is Present; that's CI signalling, not an error here.
set -l rows (ht inspect --json --only manifest,container $f | jq -r '
    .evidence[]
    | select(.status != "not_applicable" or .rationale != "source does not support this asset")
    | [.status, .source.name, .rationale] | @tsv')

# `status` is a read-only fish variable (last exit code), hence `verdict`.
for row in $rows
    set -l cols (string split \t -- $row)
    set -l verdict $cols[1]
    set -l glyph
    switch $verdict
        case present
            set glyph ●
        case absent
            set glyph ○
        case inconclusive
            set glyph ◌
        case '*'
            set glyph –
    end
    set -l label (string replace _ ' ' $verdict)
    echo "$B$glyph $label$R  $S$cols[2]$R"
    echo $cols[3] | fold -s -w $width | string replace -r '^' '    '

    # Caption, not tool output: plain-language gloss for a row a LinkedIn
    # reader can't parse. Styled like the header/footer so it reads as a note.
    switch $cols[2]
        case pixel_lattice
            echo "    $D# uncalibrated measurement: it reports a number and"
            echo "    # makes no call either way$R"
    end
    echo
end

echo "$D# "(count $rows)" sources, "(count $rows)" explanations, no score$R"
